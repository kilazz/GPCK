// crates/gpck_core/src/graphics/bcn_decoder.rs
//! # Block Decoders for BC1–BC7 Texture Formats

use super::bc7_tables::*;

pub struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    #[inline(always)]
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    #[inline(always)]
    pub fn read_bits(&mut self, count: usize) -> u32 {
        if count == 0 {
            return 0;
        }
        let start = self.bit_pos;
        self.bit_pos += count;
        let byte_idx = start / 8;
        let bit_off = start % 8;
        if byte_idx + 8 <= self.data.len() {
            let bytes = u64::from_le_bytes(self.data[byte_idx..byte_idx + 8].try_into().unwrap());
            ((bytes >> bit_off) & ((1u64 << count) - 1)) as u32
        } else {
            let mut res = 0u32;
            for i in 0..count {
                let cur = start + i;
                if cur / 8 < self.data.len() {
                    res |= (((self.data[cur / 8] >> (cur % 8)) & 1) as u32) << i;
                }
            }
            res
        }
    }
}

#[inline(always)]
pub fn expand_565(color: u16) -> (u8, u8, u8) {
    let r = (((color >> 11) & 0x1F) as u32 * 255 / 31) as u8;
    let g = (((color >> 5) & 0x3F) as u32 * 255 / 63) as u8;
    let b = ((color & 0x1F) as u32 * 255 / 31) as u8;
    (r, g, b)
}

#[inline(always)]
pub fn expand_quantized_component(val: u32, bits: u32) -> u8 {
    if bits == 0 {
        return 255;
    }
    if bits >= 8 {
        return (val & 0xFF) as u8;
    }
    let shift = 8 - bits;
    let rep_shift = (bits * 2).saturating_sub(8);
    let unquant = (val << shift) | (val >> rep_shift);
    unquant.min(255) as u8
}

pub fn decode_bc1_block(block: &[u8], dst: &mut [u8]) {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);

    let (r0, g0, b0) = expand_565(c0);
    let (r1, g1, b1) = expand_565(c1);

    let colors = if c0 > c1 {
        [
            (r0, g0, b0, 255),
            (r1, g1, b1, 255),
            (
                ((2 * r0 as u32 + r1 as u32) / 3) as u8,
                ((2 * g0 as u32 + g1 as u32) / 3) as u8,
                ((2 * b0 as u32 + b1 as u32) / 3) as u8,
                255,
            ),
            (
                ((r0 as u32 + 2 * r1 as u32) / 3) as u8,
                ((g0 as u32 + 2 * g1 as u32) / 3) as u8,
                ((b0 as u32 + 2 * b1 as u32) / 3) as u8,
                255,
            ),
        ]
    } else {
        [
            (r0, g0, b0, 255),
            (r1, g1, b1, 255),
            (
                ((r0 as u32 + r1 as u32) / 2) as u8,
                ((g0 as u32 + g1 as u32) / 2) as u8,
                ((b0 as u32 + b1 as u32) / 2) as u8,
                255,
            ),
            (0, 0, 0, 0),
        ]
    };

    for k in 0..16 {
        let idx = ((indices >> (k * 2)) & 0x03) as usize;
        let (r, g, b, a) = colors[idx];
        dst[k * 4] = r;
        dst[k * 4 + 1] = g;
        dst[k * 4 + 2] = b;
        dst[k * 4 + 3] = a;
    }
}

pub fn decode_bc2_block(block: &[u8], dst: &mut [u8]) {
    decode_bc1_block(&block[8..16], dst);
    for k in 0..16 {
        let byte_idx = k / 2;
        let alpha_nibble = if k % 2 == 0 {
            block[byte_idx] & 0x0F
        } else {
            (block[byte_idx] >> 4) & 0x0F
        };
        dst[k * 4 + 3] = alpha_nibble * 17;
    }
}

pub fn decode_bc3_block(block: &[u8], dst: &mut [u8]) {
    let a0 = block[0] as u32;
    let a1 = block[1] as u32;
    let mut a_indices = 0u64;
    for i in 0..6 {
        a_indices |= (block[2 + i] as u64) << (i * 8);
    }

    let alphas = if a0 > a1 {
        [
            a0 as u8,
            a1 as u8,
            ((6 * a0 + a1) / 7) as u8,
            ((5 * a0 + 2 * a1) / 7) as u8,
            ((4 * a0 + 3 * a1) / 7) as u8,
            ((3 * a0 + 4 * a1) / 7) as u8,
            ((2 * a0 + 5 * a1) / 7) as u8,
            ((a0 + 6 * a1) / 7) as u8,
        ]
    } else {
        [
            a0 as u8,
            a1 as u8,
            ((4 * a0 + a1) / 5) as u8,
            ((3 * a0 + 2 * a1) / 5) as u8,
            ((2 * a0 + 3 * a1) / 5) as u8,
            ((a0 + 4 * a1) / 5) as u8,
            0,
            255,
        ]
    };

    decode_bc1_block(&block[8..16], dst);
    for k in 0..16 {
        let idx = ((a_indices >> (k * 3)) & 0x07) as usize;
        dst[k * 4 + 3] = alphas[idx];
    }
}

pub fn decode_bc4_block(block: &[u8], dst: &mut [u8]) {
    let r0 = block[0] as u32;
    let r1 = block[1] as u32;
    let mut indices = 0u64;
    for i in 0..6 {
        indices |= (block[2 + i] as u64) << (i * 8);
    }

    let reds = if r0 > r1 {
        [
            r0 as u8,
            r1 as u8,
            ((6 * r0 + r1) / 7) as u8,
            ((5 * r0 + 2 * r1) / 7) as u8,
            ((4 * r0 + 3 * r1) / 7) as u8,
            ((3 * r0 + 4 * r1) / 7) as u8,
            ((2 * r0 + 5 * r1) / 7) as u8,
            ((r0 + 6 * r1) / 7) as u8,
        ]
    } else {
        [
            r0 as u8,
            r1 as u8,
            ((4 * r0 + r1) / 5) as u8,
            ((3 * r0 + 2 * r1) / 5) as u8,
            ((2 * r0 + 3 * r1) / 5) as u8,
            ((r0 + 4 * r1) / 5) as u8,
            0,
            255,
        ]
    };

    for k in 0..16 {
        let idx = ((indices >> (k * 3)) & 0x07) as usize;
        let r = reds[idx];
        dst[k * 4] = r;
        dst[k * 4 + 1] = r;
        dst[k * 4 + 2] = r;
        dst[k * 4 + 3] = 255;
    }
}

pub fn decode_bc5_block(block: &[u8], dst: &mut [u8]) {
    decode_bc4_block(&block[0..8], dst);
    let g0 = block[8] as u32;
    let g1 = block[9] as u32;
    let mut g_indices = 0u64;
    for i in 0..6 {
        g_indices |= (block[10 + i] as u64) << (i * 8);
    }

    let greens = if g0 > g1 {
        [
            g0 as u8,
            g1 as u8,
            ((6 * g0 + g1) / 7) as u8,
            ((5 * g0 + 2 * g1) / 7) as u8,
            ((4 * g0 + 3 * g1) / 7) as u8,
            ((3 * g0 + 4 * g1) / 7) as u8,
            ((2 * g0 + 5 * g1) / 7) as u8,
            ((g0 + 6 * g1) / 7) as u8,
        ]
    } else {
        [
            g0 as u8,
            g1 as u8,
            ((4 * g0 + g1) / 5) as u8,
            ((3 * g0 + 2 * g1) / 5) as u8,
            ((2 * g0 + 3 * g1) / 5) as u8,
            ((g0 + 4 * g1) / 5) as u8,
            0,
            255,
        ]
    };

    for k in 0..16 {
        let idx = ((g_indices >> (k * 3)) & 0x07) as usize;
        let g = greens[idx];
        let r = dst[k * 4];

        // Accurate Tangent-Space Normal Vector Z-Reconstruction:
        // Normal X = (R / 255.0) * 2.0 - 1.0
        // Normal Y = (G / 255.0) * 2.0 - 1.0
        // Normal Z = sqrt(max(0, 1.0 - X^2 - Y^2))
        let nx = (r as f32 / 255.0) * 2.0 - 1.0;
        let ny = (g as f32 / 255.0) * 2.0 - 1.0;
        let nz = (1.0f32 - nx * nx - ny * ny).max(0.0).sqrt();
        let b = (nz * 255.0).clamp(0.0, 255.0) as u8;

        dst[k * 4 + 1] = g;
        dst[k * 4 + 2] = b; // Reconstructed tangent-space Z (flat neutral normal = 255 / White)
        dst[k * 4 + 3] = 255;
    }
}

/// Decodes an authentic 16-byte BC6H HDR compressed block into 16 RGBA8 pixels.
pub fn decode_bc6h_block(block: &[u8], dst: &mut [u8]) {
    #[cfg(feature = "texture-decoders")]
    {
        let surface = image_dds::Surface {
            width: 4,
            height: 4,
            depth: 1,
            layers: 1,
            mipmaps: 1,
            image_format: image_dds::ImageFormat::BC6hRgbUfloat,
            data: block,
        };
        if let Ok(decoded) = surface.decode_rgba8()
            && decoded.data.len() >= 64
            && dst.len() >= 64
        {
            dst[..64].copy_from_slice(&decoded.data[..64]);
            return;
        }
    }
    let fill_len = 64.min(dst.len());
    dst[..fill_len].fill(0);
}

pub fn decode_bc7_block(block: &[u8], dst: &mut [u8]) {
    let mut reader = BitReader::new(block);
    let mut mode = 0;
    while mode < 8 {
        if reader.read_bits(1) == 1 {
            break;
        }
        mode += 1;
    }
    if mode >= 8 {
        dst.fill(0);
        return;
    }

    let partition = match mode {
        0 => reader.read_bits(4) as usize,
        1 | 2 | 3 | 7 => reader.read_bits(6) as usize,
        _ => 0,
    };
    let rotation = match mode {
        4 | 5 => reader.read_bits(2) as usize,
        _ => 0,
    };
    let index_selection = if mode == 4 {
        reader.read_bits(1) as usize
    } else {
        0
    };
    let (num_subsets, num_endpoints) = match mode {
        0 | 2 => (3, 6),
        1 | 3 | 7 => (2, 4),
        _ => (1, 2),
    };

    let (color_bits, alpha_bits) = match mode {
        0 => (4, 0),
        1 => (6, 0),
        2 => (5, 0),
        3 => (7, 0),
        4 => (5, 6),
        5 => (7, 8),
        6 => (7, 7),
        7 => (5, 5),
        _ => (0, 0),
    };

    let mut r = [0u32; 6];
    let mut g = [0u32; 6];
    let mut b = [0u32; 6];
    let mut a = [255u32; 6];
    for v in r.iter_mut().take(num_endpoints) {
        *v = reader.read_bits(color_bits);
    }
    for v in g.iter_mut().take(num_endpoints) {
        *v = reader.read_bits(color_bits);
    }
    for v in b.iter_mut().take(num_endpoints) {
        *v = reader.read_bits(color_bits);
    }
    if alpha_bits > 0 {
        for v in a.iter_mut().take(num_endpoints) {
            *v = reader.read_bits(alpha_bits);
        }
    }

    let mut p_bits = [0u32; 6];
    let num_pbits = match mode {
        0 | 3 | 6 | 7 => num_endpoints,
        1 => num_subsets,
        _ => 0,
    };
    for p in p_bits.iter_mut().take(num_pbits) {
        *p = reader.read_bits(1);
    }

    let mut ep_r = [0u8; 6];
    let mut ep_g = [0u8; 6];
    let mut ep_b = [0u8; 6];
    let mut ep_a = [255u8; 6];
    for i in 0..num_endpoints {
        let (p_r, p_g, p_b, p_a) = match mode {
            0 | 3 | 6 | 7 => (p_bits[i], p_bits[i], p_bits[i], p_bits[i]),
            1 => {
                let p = p_bits[i / 2];
                (p, p, p, p)
            }
            _ => (0, 0, 0, 0),
        };
        let effective_color_bits = color_bits + if num_pbits > 0 { 1 } else { 0 };
        let effective_alpha_bits = alpha_bits
            + if num_pbits > 0 && alpha_bits > 0 {
                1
            } else {
                0
            };

        let final_r = if num_pbits > 0 {
            (r[i] << 1) | p_r
        } else {
            r[i]
        };
        let final_g = if num_pbits > 0 {
            (g[i] << 1) | p_g
        } else {
            g[i]
        };
        let final_b = if num_pbits > 0 {
            (b[i] << 1) | p_b
        } else {
            b[i]
        };
        let final_a = if num_pbits > 0 && alpha_bits > 0 {
            (a[i] << 1) | p_a
        } else {
            a[i]
        };

        ep_r[i] = expand_quantized_component(final_r, effective_color_bits as u32);
        ep_g[i] = expand_quantized_component(final_g, effective_color_bits as u32);
        ep_b[i] = expand_quantized_component(final_b, effective_color_bits as u32);
        if alpha_bits > 0 {
            ep_a[i] = expand_quantized_component(final_a, effective_alpha_bits as u32);
        }
    }

    let mut color_indices = [0u32; 16];
    let mut alpha_indices = [0u32; 16];
    let fixup_subset0 = 0;
    let mut fixup_subset1 = 16;
    let mut fixup_subset2 = 16;

    let part_table = match num_subsets {
        2 => &BC7_PARTITION_2[partition],
        3 => &BC7_PARTITION_3[partition],
        _ => &[0u8; 16],
    };
    if num_subsets >= 2 {
        for (k, &part) in part_table.iter().enumerate().take(16) {
            if part == 1 && fixup_subset1 == 16 {
                fixup_subset1 = k;
            } else if part == 2 && fixup_subset2 == 16 {
                fixup_subset2 = k;
            }
        }
    }

    if mode == 4 {
        let mut idx_2bit = [0u32; 16];
        let mut idx_3bit = [0u32; 16];
        for (k, val) in idx_2bit.iter_mut().enumerate() {
            *val = reader.read_bits(if k == 0 { 1 } else { 2 });
        }
        for (k, val) in idx_3bit.iter_mut().enumerate() {
            *val = reader.read_bits(if k == 0 { 2 } else { 3 });
        }
        if index_selection == 0 {
            color_indices = idx_2bit;
            alpha_indices = idx_3bit;
        } else {
            color_indices = idx_3bit;
            alpha_indices = idx_2bit;
        }
    } else if mode == 5 {
        for (k, val) in color_indices.iter_mut().enumerate() {
            *val = reader.read_bits(if k == 0 { 1 } else { 2 });
        }
        for (k, val) in alpha_indices.iter_mut().enumerate() {
            *val = reader.read_bits(if k == 0 { 1 } else { 2 });
        }
    } else {
        let idx_bits = match mode {
            0 | 1 => 3,
            6 => 4,
            _ => 2,
        };
        for (k, (c_idx, a_idx)) in color_indices
            .iter_mut()
            .zip(alpha_indices.iter_mut())
            .enumerate()
        {
            let is_fixup = k == fixup_subset0
                || (num_subsets >= 2 && k == fixup_subset1)
                || (num_subsets == 3 && k == fixup_subset2);
            *c_idx = reader.read_bits(if is_fixup { idx_bits - 1 } else { idx_bits });
            *a_idx = *c_idx;
        }
    }

    let (color_idx_bits, alpha_idx_bits) = match mode {
        0 | 1 => (3, 3),
        4 => {
            if index_selection == 0 {
                (2, 3)
            } else {
                (3, 2)
            }
        }
        6 => (4, 4),
        _ => (2, 2),
    };

    for k in 0..16 {
        let subset_idx = match num_subsets {
            2 => BC7_PARTITION_2[partition][k] as usize,
            3 => BC7_PARTITION_3[partition][k] as usize,
            _ => 0,
        };
        let e0 = subset_idx * 2;
        let e1 = e0 + 1;
        let w_c = match color_idx_bits {
            2 => BC7_WEIGHT_2BIT[color_indices[k] as usize],
            3 => BC7_WEIGHT_3BIT[color_indices[k] as usize],
            4 => BC7_WEIGHT_4BIT[color_indices[k] as usize],
            _ => 0,
        };
        let w_a = match alpha_idx_bits {
            2 => BC7_WEIGHT_2BIT[alpha_indices[k] as usize],
            3 => BC7_WEIGHT_3BIT[alpha_indices[k] as usize],
            4 => BC7_WEIGHT_4BIT[alpha_indices[k] as usize],
            _ => 0,
        };

        let mut pix_r = ((64 - w_c) * ep_r[e0] as u32 + w_c * ep_r[e1] as u32 + 32) >> 6;
        let mut pix_g = ((64 - w_c) * ep_g[e0] as u32 + w_c * ep_g[e1] as u32 + 32) >> 6;
        let mut pix_b = ((64 - w_c) * ep_b[e0] as u32 + w_c * ep_b[e1] as u32 + 32) >> 6;
        let mut pix_a = ((64 - w_a) * ep_a[e0] as u32 + w_a * ep_a[e1] as u32 + 32) >> 6;

        if rotation == 1 {
            std::mem::swap(&mut pix_r, &mut pix_a);
        } else if rotation == 2 {
            std::mem::swap(&mut pix_g, &mut pix_a);
        } else if rotation == 3 {
            std::mem::swap(&mut pix_b, &mut pix_a);
        }

        dst[k * 4] = pix_r as u8;
        dst[k * 4 + 1] = pix_g as u8;
        dst[k * 4 + 2] = pix_b as u8;
        dst[k * 4 + 3] = pix_a as u8;
    }
}
