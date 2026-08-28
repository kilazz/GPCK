// crates/gpck_core/src/geometry/dgf.rs
//! # AMD Dense Geometry Format (DGF) Specification & Bitstream Codec
//!
//! Implements the 128-byte hardware-tailored DGF block format developed by AMD for RDNA 2/3/4.
//! Features:
//! - Exact 128-byte (1024-bit) cache-line aligned block packaging.
//! - Two-way bidirectional bitstream layout:
//!   - Front Buffer (Header + Vertices + Palettes) grows forward from bit 160.
//!   - Back Buffer (GTS Control Codes + Is-First flags) grows backward from bit 1023.
//! - 24-bit fixed-point spatial anchors (`S24`) with biased float32 exponents.
//! - Generalized Triangle Strip (GTS) topology compression with backtracking.

use super::meshlet::RawVertex;
use crate::core::error::{GpckError, GpckResult};
use bytemuck::{Pod, Zeroable};

pub const DGF_BLOCK_SIZE: usize = 128; // 1024 bits
pub const DGF_HEADER_SIZE: usize = 20; // 5 DWORDs (160 bits)
pub const DGF_MAX_TRIS: usize = 64;
pub const DGF_MAX_VERTS: usize = 64;
pub const DGF_MAX_INDICES: usize = 3 * DGF_MAX_TRIS;
pub const DGF_VERTEX_BIT_ALIGNMENT: usize = 4;
pub const DGF_EXPONENT_BIAS: i32 = 127;
pub const DGF_S24_MIN: i32 = -8_388_608; // -0x800000
pub const DGF_S24_MAX: i32 = 8_388_607; //  0x7FFFFF

/// 2-bit GTS triangle strip control codes matching AMD DGF specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TriControlValues {
    Restart = 0,   // New isolated triangle (emits 3 new indices)
    Edge1 = 1,     // Reuses edge (1, 2) of predecessor (emits 1 index)
    Edge2 = 2,     // Reuses edge (2, 0) of predecessor (emits 1 index)
    Backtrack = 3, // Dangles from opposite edge of predecessor's predecessor
}

/// 20-byte DGF Block Header (5 DWORDs / 160 bits).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq, Eq, Default)]
pub struct DgfBlockHeader {
    pub dword0: u32,
    pub dword1: u32,
    pub dword2: u32,
    pub dword3: u32,
    pub dword4: u32,
}

const _: () = assert!(std::mem::size_of::<DgfBlockHeader>() == DGF_HEADER_SIZE);

/// Fixed-point S24 vertex offset relative to the block anchor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OffsetVert {
    pub xyz: [u16; 3],
}

/// Bitwise read helper supporting up to 48-bit unaligned extractions.
#[inline(always)]
pub fn read_bits(bytes: &[u8], start: usize, len: usize) -> u64 {
    if len == 0 {
        return 0;
    }
    let first_byte = start / 8;
    let last_byte = (start + len - 1) / 8;
    let num_bytes = 1 + last_byte - first_byte;

    let mut dst = 0u64;
    for i in 0..num_bytes {
        if first_byte + i < bytes.len() {
            dst |= (bytes[first_byte + i] as u64) << (8 * i);
        }
    }

    (dst >> (start % 8)) & ((1u64 << len) - 1)
}

/// Bitwise write helper supporting up to 48-bit unaligned insertions.
#[inline(always)]
pub fn write_bits(bytes: &mut [u8], start: usize, len: usize, mut value: u64) {
    if len == 0 {
        return;
    }
    let mut mask = (1u64 << len) - 1;
    let offset = start % 8;
    value <<= offset;
    mask <<= offset;

    let mut byte_idx = start / 8;
    while mask != 0 && byte_idx < bytes.len() {
        let curr = bytes[byte_idx] as u64;
        bytes[byte_idx] = ((curr & !mask) | (value & mask)) as u8;
        mask >>= 8;
        value >>= 8;
        byte_idx += 1;
    }
}

pub struct DgfEncoder;

impl DgfEncoder {
    /// Adjusts component bit-widths so that their sum is a multiple of 4.
    pub fn adjust_vertex_widths(x_bits: u8, y_bits: u8, z_bits: u8) -> (u8, u8, u8) {
        let mut x = x_bits.max(1) as usize;
        let mut y = y_bits.max(1) as usize;
        let mut z = z_bits.max(1) as usize;

        let total = x + y + z;
        let rounded = (total + DGF_VERTEX_BIT_ALIGNMENT - 1) & !(DGF_VERTEX_BIT_ALIGNMENT - 1);
        let mut extra = rounded - total;

        while extra > 0 {
            if x < 16 {
                x += 1;
                extra -= 1;
                if extra == 0 {
                    break;
                }
            }
            if y < 16 {
                y += 1;
                extra -= 1;
                if extra == 0 {
                    break;
                }
            }
            if z < 16 {
                z += 1;
                extra -= 1;
                if extra == 0 {
                    break;
                }
            }
        }

        (x as u8, y as u8, z as u8)
    }

    /// Packs raw triangle geometry into a single 128-byte AMD DGF block.
    pub fn encode_block(
        vertices: &[RawVertex],
        indices: &[u8], // Cluster-local triangle indices (triples)
        prim_id_base: u32,
        exponent: i32,
    ) -> GpckResult<[u8; DGF_BLOCK_SIZE]> {
        let num_tris = indices.len() / 3;
        if num_tris == 0
            || num_tris > DGF_MAX_TRIS
            || vertices.is_empty()
            || vertices.len() > DGF_MAX_VERTS
        {
            return Err(GpckError::GeometryError(
                "Invalid cluster dimensions for DGF encoding (max 64 verts, 64 tris)".to_string(),
            ));
        }

        let mut block = [0u8; DGF_BLOCK_SIZE];

        // Compute S24 Anchor and Relative Offsets
        let scale = (2.0f32).powi(exponent - DGF_EXPONENT_BIAS);
        let inv_scale = 1.0f32 / scale;

        let mut min_quant = [i32::MAX; 3];
        let mut max_quant = [i32::MIN; 3];
        let mut quant_verts = Vec::with_capacity(vertices.len());

        for v in vertices {
            let qx = (v.position[0] * inv_scale)
                .round()
                .clamp(DGF_S24_MIN as f32, DGF_S24_MAX as f32) as i32;
            let qy = (v.position[1] * inv_scale)
                .round()
                .clamp(DGF_S24_MIN as f32, DGF_S24_MAX as f32) as i32;
            let qz = (v.position[2] * inv_scale)
                .round()
                .clamp(DGF_S24_MIN as f32, DGF_S24_MAX as f32) as i32;

            for c in 0..3 {
                min_quant[c] = min_quant[c].min([qx, qy, qz][c]);
                max_quant[c] = max_quant[c].max([qx, qy, qz][c]);
            }
            quant_verts.push([qx, qy, qz]);
        }

        let anchor = min_quant;
        let delta_x = (max_quant[0] - min_quant[0]).max(0) as u32;
        let delta_y = (max_quant[1] - min_quant[1]).max(0) as u32;
        let delta_z = (max_quant[2] - min_quant[2]).max(0) as u32;

        let raw_x_bits = (32 - delta_x.leading_zeros()).max(1) as u8;
        let raw_y_bits = (32 - delta_y.leading_zeros()).max(1) as u8;
        let raw_z_bits = (32 - delta_z.leading_zeros()).max(1) as u8;

        let (x_bits, y_bits, z_bits) =
            Self::adjust_vertex_widths(raw_x_bits, raw_y_bits, raw_z_bits);
        let bits_per_vert = (x_bits + y_bits + z_bits) as usize;

        // Pattern Match Strip Topology (GTS)
        let (control_codes, strip_indices) = Self::pattern_match_strip(indices, num_tris);

        // Compute bits per index from repeat counts
        let mut vert_seen = 0u64;
        let mut max_repeat_idx = 3usize;
        for &idx in &strip_indices {
            let bit = 1u64 << idx;
            if (vert_seen & bit) != 0 {
                max_repeat_idx = max_repeat_idx.max(idx as usize);
            } else {
                vert_seen |= bit;
            }
        }
        let bits_per_index = ((32 - (max_repeat_idx as u32).leading_zeros()).clamp(3, 6)) as u8;

        // Encode 20-Byte Block Header
        let dword0 = 0x06
            | (((bits_per_index - 3) as u32) << 8)
            | (((vertices.len() - 1) as u32) << 10)
            | (((num_tris - 1) as u32) << 16);

        let dword1 = ((exponent as u32) & 0xFF) | (((anchor[0] as u32) & 0x00FF_FFFF) << 8);
        let dword2 = ((x_bits - 1) as u32)
            | (((y_bits - 1) as u32) << 4)
            | (((anchor[1] as u32) & 0x00FF_FFFF) << 8);
        let dword3 = ((z_bits - 1) as u32) | (((anchor[2] as u32) & 0x00FF_FFFF) << 8);
        let dword4 = prim_id_base & 0x1FFF_FFFF;

        let header = DgfBlockHeader {
            dword0,
            dword1,
            dword2,
            dword3,
            dword4,
        };
        block[0..20].copy_from_slice(bytemuck::bytes_of(&header));

        // Front Buffer: Encode Vertex Offsets (grows forward from bit 160)
        let mut front_bit_pos = 160usize;
        for qv in &quant_verts {
            let ox = (qv[0] - anchor[0]) as u64;
            let oy = (qv[1] - anchor[1]) as u64;
            let oz = (qv[2] - anchor[2]) as u64;

            let packed_vert = ox | (oy << x_bits) | (oz << (x_bits + y_bits));
            write_bits(&mut block, front_bit_pos, bits_per_vert, packed_vert);
            front_bit_pos += bits_per_vert;
        }

        // Back Buffer: Encode Strip Control Codes (grows backward from bit 1023)
        for (i, &code) in control_codes.iter().enumerate().take(num_tris).skip(1) {
            let ctrl = code as u64;
            let bit_pos = 1024 - 2 * i;
            write_bits(&mut block, bit_pos, 2, ctrl);
        }

        // Middle Buffer: Encode Is-First Flags and Index Reuse Buffer
        let is_first_start_bit = 1024 - 2 * (num_tris - 1) - 1;
        let mut index_bit_pos = (front_bit_pos + 7) & !7;

        vert_seen = 0x07; // First 3 vertices (0, 1, 2) are implicit

        for (i, &idx) in strip_indices.iter().enumerate().skip(3) {
            let is_first = (vert_seen & (1u64 << idx)) == 0;
            let flag_bit_pos = is_first_start_bit - (i - 3);

            if is_first {
                vert_seen |= 1u64 << idx;
                let byte_idx = flag_bit_pos / 8;
                block[byte_idx] |= 1 << (flag_bit_pos % 8);
            } else {
                write_bits(
                    &mut block,
                    index_bit_pos,
                    bits_per_index as usize,
                    idx as u64,
                );
                index_bit_pos += bits_per_index as usize;
            }
        }

        Ok(block)
    }

    /// Pattern matches an indexed triangle list into a Generalized Triangle Strip (GTS).
    fn pattern_match_strip(tri_list: &[u8], num_tris: usize) -> (Vec<TriControlValues>, Vec<u8>) {
        let mut ctrl = vec![TriControlValues::Restart; num_tris];
        let mut indices = Vec::with_capacity(3 * num_tris);

        indices.push(tri_list[0]);
        indices.push(tri_list[1]);
        indices.push(tri_list[2]);

        for i in 1..num_tris {
            let v0 = tri_list[3 * i];
            let v1 = tri_list[3 * i + 1];
            let v2 = tri_list[3 * i + 2];

            let p0 = tri_list[3 * (i - 1)];
            let p1 = tri_list[3 * (i - 1) + 1];
            let p2 = tri_list[3 * (i - 1) + 2];

            if p1 == v1 && p2 == v0 {
                ctrl[i] = TriControlValues::Edge1;
                indices.push(v2);
            } else if p2 == v1 && p0 == v0 {
                ctrl[i] = TriControlValues::Edge2;
                indices.push(v2);
            } else if i >= 2 {
                let pp0 = tri_list[3 * (i - 2)];
                let pp1 = tri_list[3 * (i - 2) + 1];
                let pp2 = tri_list[3 * (i - 2) + 2];

                let backtrack1 = ctrl[i - 1] == TriControlValues::Edge1 && pp2 == v1 && pp0 == v0;
                let backtrack2 = ctrl[i - 1] == TriControlValues::Edge2 && pp1 == v1 && pp2 == v0;

                if backtrack1 || backtrack2 {
                    ctrl[i] = TriControlValues::Backtrack;
                    indices.push(v2);
                } else {
                    ctrl[i] = TriControlValues::Restart;
                    indices.push(v0);
                    indices.push(v1);
                    indices.push(v2);
                }
            } else {
                ctrl[i] = TriControlValues::Restart;
                indices.push(v0);
                indices.push(v1);
                indices.push(v2);
            }
        }

        (ctrl, indices)
    }
}

pub struct DgfDecoder;

impl DgfDecoder {
    /// Decodes an exact 128-byte AMD DGF block on CPU for validation and inspection.
    pub fn decode_block(block: &[u8; DGF_BLOCK_SIZE]) -> GpckResult<(Vec<[f32; 3]>, Vec<u32>)> {
        let header: &DgfBlockHeader = bytemuck::from_bytes(&block[0..20]);

        let num_tris = (((header.dword0 >> 16) & 0x3F) + 1) as usize;
        let num_verts = (((header.dword0 >> 10) & 0x3F) + 1) as usize;
        let bits_per_index = (((header.dword0 >> 8) & 0x3) + 3) as usize;

        let exponent = (header.dword1 & 0xFF) as i32;
        let scale = (2.0f32).powi(exponent - DGF_EXPONENT_BIAS);

        // Decode 24-bit signed anchors
        let ax = ((header.dword1 >> 8) as i32) << 8 >> 8;
        let ay = ((header.dword2 >> 8) as i32) << 8 >> 8;
        let az = ((header.dword3 >> 8) as i32) << 8 >> 8;

        let x_bits = ((header.dword2 & 0x0F) + 1) as usize;
        let y_bits = (((header.dword2 >> 4) & 0x0F) + 1) as usize;
        let z_bits = ((header.dword3 & 0x0F) + 1) as usize;
        let bpv = x_bits + y_bits + z_bits;

        // Decode Vertices from Front Buffer
        let mut vertices = Vec::with_capacity(num_verts);
        for v in 0..num_verts {
            let bit_pos = 160 + v * bpv;
            let raw_vert = read_bits(block, bit_pos, bpv);

            let ox = (raw_vert & ((1u64 << x_bits) - 1)) as i32;
            let oy = ((raw_vert >> x_bits) & ((1u64 << y_bits) - 1)) as i32;
            let oz = ((raw_vert >> (x_bits + y_bits)) & ((1u64 << z_bits) - 1)) as i32;

            let px = (ax + ox) as f32 * scale;
            let py = (ay + oy) as f32 * scale;
            let pz = (az + oz) as f32 * scale;
            vertices.push([px, py, pz]);
        }

        // Decode GTS Topology and Indices
        let mut control = vec![TriControlValues::Restart; num_tris];
        let mut num_indices = 3usize;

        for (i, item) in control.iter_mut().enumerate().take(num_tris).skip(1) {
            let ctrl = read_bits(block, 1024 - 2 * i, 2) as u8;
            let code = match ctrl {
                1 => TriControlValues::Edge1,
                2 => TriControlValues::Edge2,
                3 => TriControlValues::Backtrack,
                _ => TriControlValues::Restart,
            };
            num_indices += if code == TriControlValues::Restart {
                3
            } else {
                1
            };
            *item = code;
        }

        let is_first_start_bit = 1024 - 2 * (num_tris - 1) - 1;
        let mut index_bit_pos = (160 + num_verts * bpv + 7) & !7;

        let mut strip_indices = Vec::with_capacity(num_indices);
        strip_indices.push(0u8);
        strip_indices.push(1u8);
        strip_indices.push(2u8);

        let mut vertex_counter = 3u8;
        for i in 3..num_indices {
            let is_first = read_bits(block, is_first_start_bit - (i - 3), 1) != 0;
            if is_first {
                strip_indices.push(vertex_counter);
                vertex_counter += 1;
            } else {
                let idx = read_bits(block, index_bit_pos, bits_per_index) as u8;
                index_bit_pos += bits_per_index;
                strip_indices.push(idx);
            }
        }

        // Convert GTS Strip to Triangle List
        let mut tri_list = Vec::with_capacity(3 * num_tris);
        let mut prev = [0u32; 3];
        let mut prev_prev = [0u32; 3];
        let mut idx_pos = 0usize;

        for i in 0..num_tris {
            let v = match control[i] {
                TriControlValues::Restart => {
                    let a = strip_indices[idx_pos] as u32;
                    let b = strip_indices[idx_pos + 1] as u32;
                    let c = strip_indices[idx_pos + 2] as u32;
                    idx_pos += 3;
                    [a, b, c]
                }
                TriControlValues::Edge1 => {
                    let c = strip_indices[idx_pos] as u32;
                    idx_pos += 1;
                    [prev[2], prev[1], c]
                }
                TriControlValues::Edge2 => {
                    let c = strip_indices[idx_pos] as u32;
                    idx_pos += 1;
                    [prev[0], prev[2], c]
                }
                TriControlValues::Backtrack => {
                    let c = strip_indices[idx_pos] as u32;
                    idx_pos += 1;
                    if control[i - 1] == TriControlValues::Edge1 {
                        [prev_prev[0], prev_prev[2], c]
                    } else {
                        [prev_prev[2], prev_prev[1], c]
                    }
                }
            };

            tri_list.extend_from_slice(&v);
            prev_prev = prev;
            prev = v;
        }

        Ok((vertices, tri_list))
    }
}
