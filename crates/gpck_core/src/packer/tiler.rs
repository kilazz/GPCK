// crates/gpck_core/src/packer/tiler.rs
//! # 64KB Sparse Tile Slicer & Hardware Tile Packaging Engine

use crate::compression::codecs::{Codec, CompressionMethod};
use crate::core::error::{GpckError, GpckResult};
use crate::gacl::{Gacl, GaclTransform};
use crate::graphics::dxgi_format::D3D12FormatTable;
use crate::packer::texture::select_format_override;
use crate::packer::{PackerOptions, ProcessedChunk};

pub const D3D12_TILE_SIZE: usize = 65536; // 64 KB

#[derive(Debug, Clone)]
pub struct TileSliceResult {
    pub standard_chunks: Vec<ProcessedChunk>,
    pub tail_chunks: Vec<ProcessedChunk>,
    pub total_tiles: u32,
    pub num_standard_mips: u32,
    pub num_packed_mips: u32,
    pub gacl_transform: GaclTransform,
}

pub struct TiledTexturePacker;

impl TiledTexturePacker {
    /// Slices, conditions, and compresses texture subresources into standalone 64 KB hardware tiles.
    ///
    /// Standard mips and packed tail tiles are partitioned separately to allow immediate
    /// Partition 0 (Boot) placement of tail mips for sub-100ms startup rendering.
    pub fn slice_and_compress_texture_tiles(
        raw_dds_or_ktx: &[u8],
        header_len: usize,
        dxgi_fmt: u32,
        width: u32,
        height: u32,
        mip_count: u32,
        options: &PackerOptions,
    ) -> GpckResult<TileSliceResult> {
        let is_block_compressed = D3D12FormatTable::is_block_compressed(dxgi_fmt);
        let element_size = D3D12FormatTable::get_element_size(dxgi_fmt).unwrap_or(16);

        // Ensure options method is resolved from Auto to a concrete algorithm
        let mut resolved_options = options.clone();
        if resolved_options.method == CompressionMethod::Auto {
            resolved_options.method = if crate::compression::gdeflate::is_gdeflate_available() {
                CompressionMethod::GDeflate
            } else {
                CompressionMethod::Zstd
            };
        }

        // 1. Calculate Standard vs Packed Mip Tilings (D3D12 Spec)
        let (tilings, packed_info, total_tiles) = D3D12FormatTable::calculate_subresource_tilings(
            dxgi_fmt, width, height, 1, mip_count, 1,
        );

        let num_standard_mips = packed_info.num_standard_mips as u32;
        let num_packed_mips = packed_info.num_packed_mips as u32;

        let tile_shape = D3D12FormatTable::get_tile_shape_64k(dxgi_fmt, false);
        let tile_w = tile_shape.width_in_texels;
        let tile_h = tile_shape.height_in_texels;

        // 2. Extract Byte Slices for each Mip Level from DDS/KTX2 Payload
        let mut mip_payloads = Vec::with_capacity(mip_count as usize);
        let mut curr_offset = header_len;

        for m in 0..mip_count {
            let (mw, mh, _) = D3D12FormatTable::get_mip_dimensions(m, width, height, 1);
            let mip_size = if is_block_compressed {
                mw.div_ceil(4) as usize * mh.div_ceil(4) as usize * element_size
            } else {
                let bpu = D3D12FormatTable::get_bits_per_unit(dxgi_fmt);
                (mw as usize * mh as usize * bpu as usize) / 8
            };

            if curr_offset + mip_size <= raw_dds_or_ktx.len() {
                mip_payloads.push(&raw_dds_or_ktx[curr_offset..curr_offset + mip_size]);
            } else {
                break;
            }
            curr_offset += mip_size;
        }

        // 3. Slice all subresources into Raw 64KB Tile Buffers (In-Memory)
        let mut raw_standard_tiles = Vec::new();

        for m in 0..num_standard_mips {
            let tiling = &tilings[m as usize];
            let (mw, mh, _) = D3D12FormatTable::get_mip_dimensions(m, width, height, 1);
            let mip_data = mip_payloads.get(m as usize).copied().unwrap_or(&[]);

            if is_block_compressed {
                let tile_wb = (tile_w / 4) as usize;
                let tile_hb = (tile_h / 4) as usize;
                let mip_wb = mw.div_ceil(4) as usize;
                let mip_hb = mh.div_ceil(4) as usize;

                for ty in 0..tiling.height_in_tiles as usize {
                    for tx in 0..tiling.width_in_tiles as usize {
                        let mut tile_bytes = vec![0u8; D3D12_TILE_SIZE];

                        let y_start_b = ty * tile_hb;
                        let y_end_b = (y_start_b + tile_hb).min(mip_hb);
                        let x_start_b = tx * tile_wb;
                        let x_end_b = (x_start_b + tile_wb).min(mip_wb);

                        for yb in y_start_b..y_end_b {
                            let row_in_tile = yb - y_start_b;
                            let src_off = (yb * mip_wb + x_start_b) * element_size;
                            let dst_off = (row_in_tile * tile_wb) * element_size;
                            let copy_len = (x_end_b - x_start_b) * element_size;

                            if src_off + copy_len <= mip_data.len()
                                && dst_off + copy_len <= tile_bytes.len()
                            {
                                tile_bytes[dst_off..dst_off + copy_len]
                                    .copy_from_slice(&mip_data[src_off..src_off + copy_len]);
                            }
                        }

                        raw_standard_tiles.push(tile_bytes);
                    }
                }
            } else {
                let bpu = D3D12FormatTable::get_bits_per_unit(dxgi_fmt);
                let pixel_size = (bpu / 8).max(1) as usize;
                let mip_pitch = mw as usize * pixel_size;
                let tile_pitch = tile_w as usize * pixel_size;

                for ty in 0..tiling.height_in_tiles as usize {
                    for tx in 0..tiling.width_in_tiles as usize {
                        let mut tile_bytes = vec![0u8; D3D12_TILE_SIZE];

                        let y_start = ty * tile_h as usize;
                        let y_end = (y_start + tile_h as usize).min(mh as usize);
                        let x_start = tx * tile_w as usize;
                        let x_end = (x_start + tile_w as usize).min(mw as usize);

                        for y in y_start..y_end {
                            let row_in_tile = y - y_start;
                            let src_off = y * mip_pitch + x_start * pixel_size;
                            let dst_off = row_in_tile * tile_pitch;
                            let copy_len = (x_end - x_start) * pixel_size;

                            if src_off + copy_len <= mip_data.len()
                                && dst_off + copy_len <= tile_bytes.len()
                            {
                                tile_bytes[dst_off..dst_off + copy_len]
                                    .copy_from_slice(&mip_data[src_off..src_off + copy_len]);
                            }
                        }

                        raw_standard_tiles.push(tile_bytes);
                    }
                }
            }
        }

        // 4. Assemble and Pad Tail Mip Tiles (Separated for Partition 0 Boot Placement)
        let mut raw_tail_tiles = Vec::new();
        if num_packed_mips > 0 {
            let mut packed_tail_bytes = Vec::with_capacity(D3D12_TILE_SIZE);

            for m in num_standard_mips..mip_count {
                if let Some(&slice) = mip_payloads.get(m as usize) {
                    packed_tail_bytes.extend_from_slice(slice);
                }
            }

            let remainder = packed_tail_bytes.len() % D3D12_TILE_SIZE;
            if remainder != 0 {
                let pad_needed = D3D12_TILE_SIZE - remainder;
                packed_tail_bytes.extend((0..pad_needed).map(|i| (i & 0xFF) as u8));
            }

            for tile_slice in packed_tail_bytes.chunks_exact(D3D12_TILE_SIZE) {
                raw_tail_tiles.push(tile_slice.to_vec());
            }
        }

        // 5. Whole-Texture In-Memory Probing
        let (baseline_size, baseline_std_chunks, baseline_tail_chunks) =
            Self::evaluate_transform_across_all_tiles(
                &raw_standard_tiles,
                &raw_tail_tiles,
                dxgi_fmt,
                tile_w as usize,
                element_size,
                GaclTransform::None,
                &resolved_options,
            )?;

        let mut best_transform = GaclTransform::None;
        let mut best_std_chunks = baseline_std_chunks;
        let mut best_tail_chunks = baseline_tail_chunks;
        let mut best_total_size = baseline_size;

        if resolved_options.gacl.enabled {
            let candidate_list =
                if let Some(forced) = select_format_override(dxgi_fmt, &resolved_options.gacl) {
                    if !resolved_options.gacl.auto_mode {
                        vec![forced]
                    } else {
                        Self::get_candidate_transforms(dxgi_fmt, element_size).to_vec()
                    }
                } else if resolved_options.gacl.auto_mode {
                    Self::get_candidate_transforms(dxgi_fmt, element_size).to_vec()
                } else {
                    Vec::new()
                };

            for candidate in candidate_list {
                if candidate == GaclTransform::None {
                    continue;
                }

                if let Ok((cand_size, cand_std, cand_tail)) =
                    Self::evaluate_transform_across_all_tiles(
                        &raw_standard_tiles,
                        &raw_tail_tiles,
                        dxgi_fmt,
                        tile_w as usize,
                        element_size,
                        candidate,
                        &resolved_options,
                    )
                    && cand_size < best_total_size
                {
                    best_total_size = cand_size;
                    best_std_chunks = cand_std;
                    best_tail_chunks = cand_tail;
                    best_transform = candidate;
                }
            }
        }

        Ok(TileSliceResult {
            standard_chunks: best_std_chunks,
            tail_chunks: best_tail_chunks,
            total_tiles,
            num_standard_mips,
            num_packed_mips,
            gacl_transform: best_transform,
        })
    }

    fn evaluate_transform_across_all_tiles(
        standard_tiles: &[Vec<u8>],
        tail_tiles: &[Vec<u8>],
        dxgi_fmt: u32,
        tile_w_pixels: usize,
        element_size: usize,
        transform: GaclTransform,
        options: &PackerOptions,
    ) -> GpckResult<(usize, Vec<ProcessedChunk>, Vec<ProcessedChunk>)> {
        let mut std_chunks = Vec::with_capacity(standard_tiles.len());
        let mut tail_chunks = Vec::with_capacity(tail_tiles.len());
        let mut total_size = 0usize;

        // A. Standard Mip Tiles
        for tile in standard_tiles {
            let conditioned_tile = if transform != GaclTransform::None {
                Self::apply_tile_conditioning(
                    tile,
                    dxgi_fmt,
                    tile_w_pixels,
                    element_size,
                    transform,
                    options,
                )?
            } else {
                tile.clone()
            };

            let chunk = Self::compress_tile_chunk(
                &conditioned_tile,
                options.method,
                options.level,
                options.validate_chunks,
                options.atg_profile,
            )?;
            total_size += chunk.compressed_size as usize;
            std_chunks.push(chunk);
        }

        // B. Packed Tail Tiles (Always preserved with linear stream split or raw fallback)
        let tail_transform = get_linear_transform(transform);
        for tail_tile in tail_tiles {
            let conditioned_tail = if tail_transform != GaclTransform::None {
                Self::apply_tile_conditioning(
                    tail_tile,
                    dxgi_fmt,
                    tile_w_pixels,
                    element_size,
                    tail_transform,
                    options,
                )?
            } else {
                tail_tile.clone()
            };

            let chunk = Self::compress_tile_chunk(
                &conditioned_tail,
                options.method,
                options.level,
                options.validate_chunks,
                options.atg_profile,
            )?;
            total_size += chunk.compressed_size as usize;
            tail_chunks.push(chunk);
        }

        Ok((total_size, std_chunks, tail_chunks))
    }

    fn get_candidate_transforms(dxgi_fmt: u32, element_size: usize) -> &'static [GaclTransform] {
        if D3D12FormatTable::is_bc1(dxgi_fmt) {
            &[
                GaclTransform::None,
                GaclTransform::Bc1Linear,
                GaclTransform::Bc1LinearSpaceCurve,
                GaclTransform::Bc1V2BitInterleaved,
                GaclTransform::Bc1V2SpaceCurve,
            ]
        } else if D3D12FormatTable::is_bc2(dxgi_fmt) {
            &[GaclTransform::None, GaclTransform::Bc2AlphaNibble]
        } else if D3D12FormatTable::is_bc3(dxgi_fmt) {
            &[
                GaclTransform::None,
                GaclTransform::Bc3Linear,
                GaclTransform::Bc3LinearSpaceCurve,
                GaclTransform::Bc3V2BitInterleaved,
                GaclTransform::Bc3V2SpaceCurve,
            ]
        } else if D3D12FormatTable::is_bc4(dxgi_fmt) {
            &[
                GaclTransform::None,
                GaclTransform::Bc4Linear,
                GaclTransform::Bc4LinearSpaceCurve,
            ]
        } else if D3D12FormatTable::is_bc5(dxgi_fmt) {
            &[
                GaclTransform::None,
                GaclTransform::Bc5DualChannel,
                GaclTransform::Bc5SpaceCurve,
            ]
        } else if D3D12FormatTable::is_bc6h(dxgi_fmt) {
            &[GaclTransform::None, GaclTransform::Bc6hHeaderJoin]
        } else if D3D12FormatTable::is_bc7(dxgi_fmt) {
            &[
                GaclTransform::None,
                GaclTransform::Bc7ModeSplit,
                GaclTransform::Bc7ModeJoin,
            ]
        } else if element_size == 16 {
            &[GaclTransform::None, GaclTransform::CurveOnly16B]
        } else {
            &[GaclTransform::None]
        }
    }

    fn apply_tile_conditioning(
        tile_bytes: &[u8],
        dxgi_fmt: u32,
        tile_width_pixels: usize,
        element_size: usize,
        transform: GaclTransform,
        options: &PackerOptions,
    ) -> GpckResult<Vec<u8>> {
        if transform == GaclTransform::None {
            return Ok(tile_bytes.to_vec());
        }

        let mut working = tile_bytes.to_vec();

        if options.gacl.rdo_reduction_pct > 0.0 {
            let _ = Gacl::apply_bler(
                &mut working,
                dxgi_fmt,
                options.gacl.rdo_reduction_pct / 100.0,
                options.gacl.rdo_use_ycocg,
            );
        }

        Gacl::apply_exact_transform(
            &working,
            transform.to_u32(),
            element_size,
            tile_width_pixels,
        )
    }

    fn compress_tile_chunk(
        tile_data: &[u8],
        method: CompressionMethod,
        level: i32,
        validate: bool,
        atg_profile: bool,
    ) -> GpckResult<ProcessedChunk> {
        let actual_method = match method {
            CompressionMethod::Auto => {
                if crate::compression::gdeflate::is_gdeflate_available() {
                    CompressionMethod::GDeflate
                } else {
                    CompressionMethod::Zstd
                }
            }
            m => m,
        };

        let hash = twox_hash::XxHash64::oneshot(0, tile_data);

        let compressed = match Codec::compress(tile_data, actual_method, level, atg_profile) {
            Ok(c) => c,
            Err(_) => tile_data.to_vec(),
        };

        let is_uncompressed_fallback =
            compressed.len() >= tile_data.len() || compressed == tile_data;

        if validate && actual_method != CompressionMethod::Store && !is_uncompressed_fallback {
            match Codec::decompress(&compressed, tile_data.len(), actual_method) {
                Ok(decompressed) => {
                    let decomp_hash = twox_hash::XxHash64::oneshot(0, &decompressed);
                    if decomp_hash != hash {
                        // Detailed byte-level difference analysis
                        let mut first_diff_idx = None;
                        let mut diff_count = 0usize;

                        for (i, (&orig, &dec)) in
                            tile_data.iter().zip(decompressed.iter()).enumerate()
                        {
                            if orig != dec {
                                if first_diff_idx.is_none() {
                                    first_diff_idx = Some(i);
                                }
                                diff_count += 1;
                            }
                        }

                        let diff_idx = first_diff_idx.unwrap_or(0);
                        let orig_slice = &tile_data[diff_idx..(diff_idx + 16).min(tile_data.len())];
                        let dec_slice =
                            &decompressed[diff_idx..(diff_idx + 16).min(decompressed.len())];

                        let diag_msg = format!(
                            "\n==================== [BROTLI-G CHUNK MISMATCH DIAGNOSTIC] ====================\n\
                             Chunk Hash           : {:016X}\n\
                             Original Size        : {} bytes\n\
                             Compressed Size      : {} bytes (Ratio: {:.1}%)\n\
                             Decompressed Size    : {} bytes\n\
                             Total Mismatched     : {} / {} bytes ({:.2}% corrupted)\n\
                             First Mismatch Offset: Byte {} (0x{:04X})\n\
                             Original Bytes       : {:02X?}\n\
                             Decompressed Bytes   : {:02X?}\n\
                             ================================================================================",
                            hash,
                            tile_data.len(),
                            compressed.len(),
                            (compressed.len() as f64 / tile_data.len() as f64) * 100.0,
                            decompressed.len(),
                            diff_count,
                            tile_data.len(),
                            (diff_count as f64 / tile_data.len() as f64) * 100.0,
                            diff_idx,
                            diff_idx,
                            orig_slice,
                            dec_slice
                        );

                        crate::core::logger::log_error(&diag_msg);
                        eprintln!("{}", diag_msg);

                        // Save failure dumps into the centralized log directory for offline analysis
                        let log_dir = crate::core::paths::GpckPaths::get_logs_dir();
                        let _ = std::fs::write(log_dir.join("failed_tile_orig.bin"), tile_data);
                        let _ =
                            std::fs::write(log_dir.join("failed_tile_compressed.bin"), &compressed);
                        let _ =
                            std::fs::write(log_dir.join("failed_tile_decomp.bin"), &decompressed);

                        return Err(GpckError::ChunkValidationFailed {
                            hash,
                            message: format!(
                                "Tile chunk validation failed: hash mismatch after decompression (diff at 0x{:04X}, {} bytes diff)",
                                diff_idx, diff_count
                            ),
                        });
                    }
                }
                Err(e) => {
                    return Err(GpckError::ChunkValidationFailed {
                        hash,
                        message: format!(
                            "Tile chunk validation failed: unable to decompress generated tile: {}",
                            e
                        ),
                    });
                }
            }
        }

        Ok(ProcessedChunk {
            data: compressed.clone(),
            compressed_size: compressed.len() as u32,
            original_size: tile_data.len() as u32,
            hash,
            offset: 0,
        })
    }
}

#[inline(always)]
pub fn get_linear_transform(transform: GaclTransform) -> GaclTransform {
    match transform {
        GaclTransform::Bc1LinearSpaceCurve => GaclTransform::Bc1Linear,
        GaclTransform::Bc1V2SpaceCurve => GaclTransform::Bc1V2BitInterleaved,
        GaclTransform::Bc3LinearSpaceCurve => GaclTransform::Bc3Linear,
        GaclTransform::Bc3V2SpaceCurve => GaclTransform::Bc3V2BitInterleaved,
        GaclTransform::Bc4LinearSpaceCurve => GaclTransform::Bc4Linear,
        GaclTransform::Bc5SpaceCurve => GaclTransform::Bc5DualChannel,
        GaclTransform::CurveOnly16B => GaclTransform::None,
        t => t,
    }
}
