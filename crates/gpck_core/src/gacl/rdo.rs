// crates/gpck_core/src/gacl/rdo.rs
//! # Rate-Distortion Optimization (BLER) Engine with Perceptual YCoCg
//!
//! Provides Block-Level Entropy Reduction (BLER) using Lagrangian RDO,
//! SIMD-accelerated SAD metrics (SSE2 on x86_64 and NEON on aarch64),
//! edge-preserving classification, and perceptual variance clustering.

use crate::core::error::GpckResult;
use crate::graphics::bcn_decoder::*;
use crate::graphics::dxgi_format::D3D12FormatTable;

// ============================================================================
// SIMD Accelerated SAD (Sum of Absolute Differences)
// ============================================================================

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
#[inline]
unsafe fn exact_pixel_distance_sad_sse2(block_a: &[u8], block_b: &[u8]) -> u32 {
    use std::arch::x86_64::*;
    unsafe {
        let mut total_sad = _mm_setzero_si128();
        for i in (0..64).step_by(16) {
            let va = _mm_loadu_si128(block_a.as_ptr().add(i) as *const __m128i);
            let vb = _mm_loadu_si128(block_b.as_ptr().add(i) as *const __m128i);
            let sad = _mm_sad_epu8(va, vb);
            total_sad = _mm_add_epi64(total_sad, sad);
        }
        let sad_high = _mm_unpackhi_epi64(total_sad, total_sad);
        let final_sad = _mm_add_epi64(total_sad, sad_high);
        _mm_cvtsi128_si32(final_sad) as u32
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
#[inline]
unsafe fn exact_pixel_distance_sad_neon(block_a: &[u8], block_b: &[u8]) -> u32 {
    use std::arch::aarch64::*;
    unsafe {
        let mut total_sad = 0u32;
        for i in (0..64).step_by(16) {
            let va = vld1q_u8(block_a.as_ptr().add(i));
            let vb = vld1q_u8(block_b.as_ptr().add(i));
            let diff = vabdq_u8(va, vb);
            total_sad += vaddlvq_u8(diff) as u32;
        }
        total_sad
    }
}

#[inline(always)]
fn exact_pixel_distance_sad(block_a: &[u8], block_b: &[u8]) -> u32 {
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("sse2") && block_a.len() >= 64 && block_b.len() >= 64 {
            return unsafe { exact_pixel_distance_sad_sse2(block_a, block_b) };
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        if std::arch::is_aarch64_feature_detected!("neon")
            && block_a.len() >= 64
            && block_b.len() >= 64
        {
            return unsafe { exact_pixel_distance_sad_neon(block_a, block_b) };
        }
    }
    block_a
        .iter()
        .zip(block_b.iter())
        .map(|(a, b)| a.abs_diff(*b) as u32)
        .sum()
}

/// Identifies fine lines, specular highlights, and edge discontinuities.
/// A delta threshold of 35 prevents subtle gradients and bevels from being merged.
#[inline(always)]
fn is_high_contrast_edge_block(block_rgba: &[u8]) -> bool {
    let mut min_val = [255u8; 4];
    let mut max_val = [0u8; 4];

    for pixel in block_rgba.chunks_exact(4) {
        for c in 0..4 {
            min_val[c] = min_val[c].min(pixel[c]);
            max_val[c] = max_val[c].max(pixel[c]);
        }
    }

    for c in 0..4 {
        if max_val[c] - min_val[c] > 35 {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy)]
struct BlockSummary {
    mean: [u8; 4],
    primary_metric: u8,
    variance: u16,
    is_protected_edge: bool,
}

impl BlockSummary {
    fn from_block(block: &[u8], is_protected_edge: bool, is_ycocg: bool) -> Self {
        let mut sum = [0u32; 4];
        let mut sum_sq_primary = 0u32;

        for pixel in block.chunks_exact(4) {
            sum[0] += pixel[0] as u32; // Y (or R)
            sum[1] += pixel[1] as u32; // Co (or G)
            sum[2] += pixel[2] as u32; // Cg (or B)
            sum[3] += pixel[3] as u32; // Alpha

            let primary = if is_ycocg {
                pixel[0] as u32
            } else {
                (pixel[0] as u32 + (pixel[1] as u32 * 2) + pixel[2] as u32) >> 2
            };
            sum_sq_primary += primary * primary;
        }

        let mean = [
            (sum[0] / 16) as u8,
            (sum[1] / 16) as u8,
            (sum[2] / 16) as u8,
            (sum[3] / 16) as u8,
        ];

        let mean_primary = if is_ycocg {
            mean[0] as u32
        } else {
            (mean[0] as u32 + (mean[1] as u32 * 2) + mean[2] as u32) >> 2
        };

        let variance = ((sum_sq_primary / 16).saturating_sub(mean_primary * mean_primary)) as u16;

        Self {
            mean,
            primary_metric: mean_primary as u8,
            variance,
            is_protected_edge,
        }
    }

    #[inline(always)]
    fn min_distance_sad(&self, other: &Self) -> f32 {
        let mut dist = 0.0f32;
        for c in 0..4 {
            dist += (self.mean[c] as f32 - other.mean[c] as f32).abs();
        }
        dist * 16.0
    }
}

pub(crate) fn block_level_entropy_reduce(
    encoded_data: &mut [u8],
    element_size: usize,
    dxgi_format: u32,
    target_reduction_pct: f32,
    use_ycocg_perceptual: bool,
) -> GpckResult<usize> {
    let num_blocks = encoded_data.len() / element_size;
    if num_blocks < 2 {
        return Ok(0);
    }

    // Safely normalize percentage ratio (e.g. 5.0 -> 0.05 or 0.05 -> 0.05)
    let reduction_ratio = if target_reduction_pct > 1.0 {
        (target_reduction_pct / 100.0).clamp(0.001, 1.0)
    } else {
        target_reduction_pct.clamp(0.001, 1.0)
    };

    let is_color_format = D3D12FormatTable::is_bc1(dxgi_format)
        || D3D12FormatTable::is_bc2(dxgi_format)
        || D3D12FormatTable::is_bc3(dxgi_format)
        || D3D12FormatTable::is_bc7(dxgi_format);

    let is_normal_or_hdr =
        D3D12FormatTable::is_bc5(dxgi_format) || D3D12FormatTable::is_bc6h(dxgi_format);

    // Smart Guard: Only activate YCoCg for color maps; strictly bypass for normal/height/HDR maps
    let is_ycocg_active = use_ycocg_perceptual && is_color_format && !is_normal_or_hdr;

    let mut decoded_data = vec![0u8; num_blocks * 64];
    let mut edge_flags = vec![false; num_blocks];

    decode_bc_blocks(
        encoded_data,
        element_size,
        dxgi_format,
        &mut decoded_data,
        &mut edge_flags,
        is_ycocg_active,
    );

    let summaries: Vec<BlockSummary> = decoded_data
        .chunks_exact(64)
        .zip(edge_flags)
        .map(|(blk, edge)| BlockSummary::from_block(blk, edge, is_ycocg_active))
        .collect();

    let window_blocks = (32 * 1024 / element_size).clamp(512, 2048).min(num_blocks);

    // Strict ceiling for maximum allowable SAD error across a 4x4 block
    let max_hard_sad_limit: u32 = if is_normal_or_hdr { 96 } else { 240 };

    let target_merges = (num_blocks as f32 * reduction_ratio) as usize;
    let mut paired = vec![false; num_blocks];

    let mut cur_sad_limit = 48u32;
    let base_lambda = (reduction_ratio * 2.0).exp() - 1.0;
    let rate_savings_bits = (element_size * 8) as f32;
    let base_lambda_rate_penalty = base_lambda * rate_savings_bits;

    let mut buckets: Vec<Vec<usize>> = (0..256)
        .map(|_| Vec::with_capacity(window_blocks))
        .collect();
    let mut total_merges = 0;

    while total_merges < target_merges && cur_sad_limit <= max_hard_sad_limit {
        let mut merges_this_pass = 0;
        let mut window_start = 0;

        for bucket in buckets.iter_mut() {
            bucket.clear();
        }

        for i in 0..num_blocks {
            let current_start_idx = i.saturating_sub(window_blocks);
            while window_start < current_start_idx {
                if !paired[window_start] && !summaries[window_start].is_protected_edge {
                    let key = summaries[window_start].primary_metric as usize;
                    if let Some(pos) = buckets[key].iter().position(|&x| x == window_start) {
                        buckets[key].swap_remove(pos);
                    }
                }
                window_start += 1;
            }

            if paired[i] || summaries[i].is_protected_edge {
                continue;
            }

            let summary_i = &summaries[i];
            let pixels_i = &decoded_data[i * 64..(i + 1) * 64];
            let target_key = summary_i.primary_metric as i32;

            let variance_clamped = summary_i.variance.clamp(0, 512) as f32;
            let perceptual_multiplier = (0.5 + (variance_clamped / 512.0) * 1.5).clamp(0.5, 2.0);
            let block_lambda_rate_penalty = base_lambda_rate_penalty * perceptual_multiplier;

            let mut best_cost = 0.0f32;
            let mut best_j = None;

            let search_radius = (cur_sad_limit / 16).min(16) as i32;

            for radius in 0..=search_radius {
                let diff_cost = (radius as f32) * 16.0;
                if (diff_cost - block_lambda_rate_penalty) >= best_cost && best_cost < 0.0 {
                    break;
                }

                let mut check_bucket = |bucket_idx: usize| {
                    for &j in &buckets[bucket_idx] {
                        if paired[j] {
                            continue;
                        }

                        let summary_j = &summaries[j];
                        let sad_bound = summary_i.min_distance_sad(summary_j);
                        if (sad_bound - block_lambda_rate_penalty) >= best_cost && best_cost < 0.0 {
                            continue;
                        }

                        let pixels_j = &decoded_data[j * 64..(j + 1) * 64];
                        let sad_distortion = exact_pixel_distance_sad(pixels_i, pixels_j);

                        if sad_distortion <= cur_sad_limit {
                            let rd_cost = (sad_distortion as f32) - block_lambda_rate_penalty;
                            if rd_cost < best_cost {
                                best_cost = rd_cost;
                                best_j = Some(j);
                            }
                        }
                    }
                };

                let bucket_up = target_key + radius;
                let bucket_down = target_key - radius;

                if radius == 0 {
                    check_bucket(bucket_up as usize);
                } else {
                    if bucket_up <= 255 {
                        check_bucket(bucket_up as usize);
                    }
                    if bucket_down >= 0 {
                        check_bucket(bucket_down as usize);
                    }
                }
            }

            if let Some(j_idx) = best_j {
                let src_offset = j_idx * element_size;
                let dst_offset = i * element_size;
                encoded_data.copy_within(src_offset..src_offset + element_size, dst_offset);

                let src_rgba = j_idx * 64;
                let dst_rgba = i * 64;
                decoded_data.copy_within(src_rgba..src_rgba + 64, dst_rgba);

                paired[i] = true;
                paired[j_idx] = true;

                let key_j = summaries[j_idx].primary_metric as usize;
                if let Some(pos) = buckets[key_j].iter().position(|&x| x == j_idx) {
                    buckets[key_j].swap_remove(pos);
                }

                merges_this_pass += 1;
                total_merges += 1;
                if total_merges >= target_merges {
                    break;
                }
            } else {
                buckets[target_key as usize].push(i);
            }
        }

        if merges_this_pass == 0 {
            cur_sad_limit = cur_sad_limit.saturating_add(32);
        }
    }

    Ok(total_merges)
}

#[inline(always)]
fn convert_rgba_to_ycocg_inplace(slice: &mut [u8]) {
    for pixel in slice.chunks_exact_mut(4) {
        let r = pixel[0] as i32;
        let g = pixel[1] as i32;
        let b = pixel[2] as i32;
        // pixel[3] (Alpha) is preserved untouched

        let y = (r + (g << 1) + b) >> 2;
        let co = (r - b) >> 1;
        let cg = (-r + (g << 1) - b) >> 2;

        pixel[0] = y.clamp(0, 255) as u8;
        pixel[1] = (co + 128).clamp(0, 255) as u8;
        pixel[2] = (cg + 128).clamp(0, 255) as u8;
    }
}

fn decode_bc_blocks(
    src: &[u8],
    element_size: usize,
    dxgi_format: u32,
    dst_data: &mut [u8],
    edge_flags: &mut [bool],
    apply_ycocg: bool,
) {
    let is_bc1 = D3D12FormatTable::is_bc1(dxgi_format);
    let is_bc2 = D3D12FormatTable::is_bc2(dxgi_format);
    let is_bc3 = D3D12FormatTable::is_bc3(dxgi_format);
    let is_bc4 = D3D12FormatTable::is_bc4(dxgi_format);
    let is_bc5 = D3D12FormatTable::is_bc5(dxgi_format);
    let is_bc6h = D3D12FormatTable::is_bc6h(dxgi_format);
    let is_bc7 = D3D12FormatTable::is_bc7(dxgi_format);

    for (i, block) in src.chunks_exact(element_size).enumerate() {
        let slice = &mut dst_data[i * 64..(i + 1) * 64];

        if is_bc1 {
            decode_bc1_block(block, slice);
        } else if is_bc2 {
            decode_bc2_block(block, slice);
        } else if is_bc3 {
            decode_bc3_block(block, slice);
        } else if is_bc4 {
            decode_bc4_block(block, slice);
        } else if is_bc5 {
            decode_bc5_block(block, slice);
        } else if is_bc6h {
            decode_bc6h_block(block, slice);
        } else if is_bc7 {
            decode_bc7_block(block, slice);
        } else {
            for p in 0..16 {
                slice[p * 4] = block[p % element_size];
                slice[p * 4 + 1] = block[(p + 1) % element_size];
                slice[p * 4 + 2] = block[(p + 2) % element_size];
                slice[p * 4 + 3] = 255;
            }
        }

        edge_flags[i] = is_high_contrast_edge_block(slice);

        if apply_ycocg {
            convert_rgba_to_ycocg_inplace(slice);
        }
    }
}
