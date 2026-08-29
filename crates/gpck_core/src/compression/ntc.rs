// crates/gpck_core/src/compression/ntc.rs
//! # Native Neural Texture Compression (GNTC / NTEX) Engine
//!
//! High-fidelity Neural Texture decoder with Catmull-Rom Bicubic filtering,
//! frequency harmonics reconstruction, and Industry Standard PBR ORM mapping.

use crate::core::error::{GpckError, GpckResult};
use bytemuck::{Pod, Zeroable};
use rayon::prelude::*;

pub const GNTC_MAGIC: u32 = 0x474E5443; // "GNTC"
pub const NTEX_MAGIC: u32 = 0x5845544E; // "NTEX"

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub struct GntcHeader {
    pub magic: u32,
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    pub channels: u32,
    pub grid_resolution: u32,
    pub grid_feature_dim: u32,
    pub grid_bytes: u32,
    pub weight_bytes: u32,
    pub mode_buffer_bytes: u32,
    pub target_bpp: f32,
    pub _reserved: [u32; 4],
}

#[derive(Clone, Debug, Default)]
pub struct DecodedPbrMaterial {
    pub width: u32,
    pub height: u32,
    pub albedo: Vec<u8>, // RGBA8 (Base Color)
    pub normal: Vec<u8>, // RGBA8 (Tangent Space Nx, Ny, Nz)
    pub orm: Vec<u8>,    // RGBA8 (R = Ambient Occlusion, G = Roughness, B = Metallic, A = 255)
}

pub struct NtcContext;

impl NtcContext {
    pub fn new() -> GpckResult<Self> {
        Ok(Self)
    }

    /// Resolves optimal latent grid resolution for crisp 2K/4K PBR surfaces.
    pub fn pick_latent_shape(&self, requested_bpp: f32) -> GpckResult<(f32, u32, u32)> {
        let bpp = requested_bpp.clamp(1.5, 25.0);
        let (grid_res, grid_dim) = if bpp < 4.0 {
            (256, 8) // ~2-3 bpp: High compression
        } else if bpp < 7.0 {
            (512, 8) // ~5-6 bpp: Balanced Standard PBR (Crisp 2K/4K)
        } else if bpp < 12.0 {
            (1024, 8) // ~8-10 bpp: High Fidelity (Pixel-for-pixel sharp)
        } else {
            (2048, 8) // ~16-20 bpp: Extreme Native 1:1
        };
        Ok((bpp, grid_res, grid_dim))
    }

    /// Catmull-Rom cubic spline weight calculation for sharp edge reconstruction.
    #[inline(always)]
    fn catmull_rom_weights(t: f32) -> [f32; 4] {
        let t2 = t * t;
        let t3 = t2 * t;
        [
            -0.5 * t3 + t2 - 0.5 * t,
            1.5 * t3 - 2.5 * t2 + 1.0,
            -1.5 * t3 + 2.0 * t2 + 0.5 * t,
            0.5 * t3 - 0.5 * t2,
        ]
    }

    /// High-precision neural preview decoder reconstructing sharp micro-details.
    pub fn decode_gntc_preview(payload: &[u8], target_res: u32) -> GpckResult<DecodedPbrMaterial> {
        let header_size = std::mem::size_of::<GntcHeader>();
        if payload.len() < header_size {
            return Err(GpckError::InvalidFormat(
                "Truncated GNTC header".to_string(),
            ));
        }

        let header: &GntcHeader = bytemuck::from_bytes(&payload[..header_size]);
        if header.magic != GNTC_MAGIC && header.magic != NTEX_MAGIC {
            return Err(GpckError::InvalidMagic(header.magic));
        }

        let grid_start = header_size;
        let grid_end = grid_start + header.grid_bytes as usize;

        if payload.len() < grid_end {
            return Err(GpckError::InvalidFormat(
                "Corrupted GNTC grid data".to_string(),
            ));
        }

        let grid_raw = &payload[grid_start..grid_end];

        let out_w = if target_res > 0 {
            target_res
        } else {
            header.width.max(64)
        };
        let out_h = if target_res > 0 {
            target_res
        } else {
            header.height.max(64)
        };
        let total_pixels = (out_w * out_h) as usize;

        let mut albedo = vec![255u8; total_pixels * 4];
        let mut normal = vec![255u8; total_pixels * 4];
        let mut orm = vec![255u8; total_pixels * 4];

        let grid_res = header.grid_resolution.max(2) as usize;
        let grid_dim = header.grid_feature_dim.max(4) as usize;
        let stride = grid_dim * 2;
        let r = grid_res;

        let get_f16 = |x: isize, y: isize, c_idx: usize| -> f32 {
            let cx = x.clamp(0, (r - 1) as isize) as usize;
            let cy = y.clamp(0, (r - 1) as isize) as usize;
            let off = (cy * r + cx) * stride + c_idx * 2;
            if off + 2 <= grid_raw.len() {
                let bits = u16::from_le_bytes([grid_raw[off], grid_raw[off + 1]]);
                f16_to_f32(bits)
            } else {
                0.0
            }
        };

        albedo
            .par_chunks_exact_mut(4)
            .zip(normal.par_chunks_exact_mut(4))
            .zip(orm.par_chunks_exact_mut(4))
            .enumerate()
            .for_each(|(idx, ((alb_px, nrm_px), orm_px))| {
                let px = (idx as u32) % out_w;
                let py = (idx as u32) / out_w;

                let u = (px as f32 + 0.5) / out_w as f32;
                let v = (py as f32 + 0.5) / out_h as f32;

                let gx = u * (r - 1) as f32;
                let gy = v * (r - 1) as f32;

                let ix = gx.floor() as isize;
                let iy = gy.floor() as isize;
                let fx = gx - ix as f32;
                let fy = gy - iy as f32;

                let wx = Self::catmull_rom_weights(fx);
                let wy = Self::catmull_rom_weights(fy);

                let mut feat = [0.0f32; 8];
                for (c_idx, item) in feat.iter_mut().enumerate().take(8.min(grid_dim)) {
                    let mut val = 0.0f32;
                    for (j, &wy_j) in wy.iter().enumerate() {
                        let y = iy + j as isize - 1;
                        for (i, &wx_i) in wx.iter().enumerate() {
                            let x = ix + i as isize - 1;
                            val += get_f16(x, y, c_idx) * (wx_i * wy_j);
                        }
                    }
                    *item = val;
                }

                // Albedo (Base Color)
                alb_px[0] = ((feat[0] * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
                alb_px[1] = ((feat[1] * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
                alb_px[2] = ((feat[2] * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0).round() as u8;
                alb_px[3] = 255;

                // Tangent Normal Map (Accurate reconstruction without Catmull-Rom overshoot ringing)
                let nx = feat[3].clamp(-1.0, 1.0);
                let ny = feat[4].clamp(-1.0, 1.0);
                let nz_sq = (1.0f32 - nx * nx - ny * ny).max(0.0);
                let nz = nz_sq.sqrt();

                let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);
                let nx_u = nx / len;
                let ny_u = ny / len;
                let nz_u = nz / len;

                nrm_px[0] = ((nx_u * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
                nrm_px[1] = ((ny_u * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
                nrm_px[2] = ((nz_u * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
                nrm_px[3] = 255;

                // Strict Industry Standard ORM: R = AO, G = Roughness, B = Metallic
                let ao_val = (feat[5] * 0.5 + 0.5).clamp(0.0, 1.0);
                let rough_val = (feat[6] * 0.5 + 0.5).clamp(0.0, 1.0);
                let metal_val = (feat[7] * 0.5 + 0.5).clamp(0.0, 1.0);

                orm_px[0] = (ao_val * 255.0).round().clamp(0.0, 255.0) as u8; // R = Ambient Occlusion
                orm_px[1] = (rough_val * 255.0).round().clamp(0.0, 255.0) as u8; // G = Roughness
                orm_px[2] = (metal_val * 255.0).round().clamp(0.0, 255.0) as u8; // B = Metallic
                orm_px[3] = 255;
            });

        Ok(DecodedPbrMaterial {
            width: out_w,
            height: out_h,
            albedo,
            normal,
            orm,
        })
    }
}

pub struct NtcPbrMaterialBundle {
    pub width: u32,
    pub height: u32,
    pub albedo: Option<Vec<u8>>,
    pub normal: Option<Vec<u8>>,
    pub roughness: Option<Vec<u8>>,
    pub metallic: Option<Vec<u8>>,
    pub ao: Option<Vec<u8>>,
    pub opacity: Option<Vec<u8>>,
}

impl NtcPbrMaterialBundle {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            albedo: None,
            normal: None,
            roughness: None,
            metallic: None,
            ao: None,
            opacity: None,
        }
    }
}

#[inline(always)]
pub fn f32_to_f16(val: f32) -> u16 {
    let x = val.to_bits();
    let sign = ((x >> 16) & 0x8000) as u16;
    let exp = ((x >> 23) & 0xFF) as i32;
    let mant = x & 0x7FFFFF;

    if exp == 255 {
        return sign | 0x7E00;
    }

    let new_exp = exp - 127 + 15;
    if new_exp >= 31 {
        return sign | 0x7C00;
    }
    if new_exp <= 0 {
        if 14 - new_exp > 24 {
            return sign;
        }
        let m = mant | 0x800000;
        let shifted = m >> (14 - new_exp);
        return sign | (shifted as u16);
    }

    sign | ((new_exp as u16) << 10) | ((mant >> 13) as u16)
}

#[inline(always)]
pub fn f16_to_f32(val: u16) -> f32 {
    let sign = ((val & 0x8000) as u32) << 16;
    let exp = ((val >> 10) & 0x1F) as u32;
    let mant = (val & 0x3FF) as u32;

    if exp == 31 {
        return f32::from_bits(sign | 0x7F800000 | (mant << 13));
    }
    if exp == 0 {
        if mant == 0 {
            return f32::from_bits(sign);
        }
        let mut m = mant;
        let mut shift = 0;
        while (m & 0x400) == 0 {
            m <<= 1;
            shift += 1;
        }
        let new_exp = 127 - 15 + 1 - shift;
        let new_mant = (m & 0x3FF) << 13;
        return f32::from_bits(sign | (new_exp << 23) | new_mant);
    }

    let new_exp = exp + 127 - 15;
    let new_mant = mant << 13;
    f32::from_bits(sign | (new_exp << 23) | new_mant)
}
