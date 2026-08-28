// crates/gpck_core/src/format/dds.rs
//! # DirectDraw Surface (DDS) Texture Utilities
//!
//! Provides naturally-aligned header parsing, DXGI format detection (both block-compressed
//! and uncompressed RGB/RGBA/BGRA/Grayscale masks), MipMap calculations, and streaming splits.

use crate::graphics::dxgi_format::dxgi;
use bytemuck::{Pod, Zeroable};
use std::cmp::max;

pub const DDS_MAGIC: u32 = 0x20534444; // "DDS "

// DirectDraw Surface Flags
pub const DDSD_CAPS: u32 = 0x1;
pub const DDSD_HEIGHT: u32 = 0x2;
pub const DDSD_WIDTH: u32 = 0x4;
pub const DDSD_PITCH: u32 = 0x8;
pub const DDSD_PIXELFORMAT: u32 = 0x1000;
pub const DDSD_MIPMAPCOUNT: u32 = 0x20000;
pub const DDSD_LINEARSIZE: u32 = 0x80000;

pub const DDSCAPS_COMPLEX: u32 = 0x8;
pub const DDSCAPS_TEXTURE: u32 = 0x1000;
pub const DDSCAPS_MIPMAP: u32 = 0x400000;

pub const DDSCAPS2_MIPMAPSUBTREE: u32 = 0x00400000;

// Pixel Format Flags
pub const DDPF_ALPHAPIXELS: u32 = 0x1;
pub const DDPF_ALPHA: u32 = 0x2;
pub const DDPF_FOURCC: u32 = 0x4;
pub const DDPF_RGB: u32 = 0x40;
pub const DDPF_LUMINANCE: u32 = 0x20000;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct DdsPixelFormat {
    pub dw_size: u32,
    pub dw_flags: u32,
    pub dw_four_cc: u32,
    pub dw_rgb_bit_count: u32,
    pub dw_r_bit_mask: u32,
    pub dw_g_bit_mask: u32,
    pub dw_b_bit_mask: u32,
    pub dw_a_bit_mask: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct DdsHeader {
    pub dw_size: u32,
    pub dw_flags: u32,
    pub dw_height: u32,
    pub dw_width: u32,
    pub dw_pitch_or_linear_size: u32,
    pub dw_depth: u32,
    pub dw_mip_map_count: u32,
    pub dw_reserved1: [u32; 11],
    pub ddspf: DdsPixelFormat,
    pub dw_caps: u32,
    pub dw_caps2: u32,
    pub dw_caps3: u32,
    pub dw_caps4: u32,
    pub dw_reserved2: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdsBasicInfo {
    pub width: usize,
    pub height: usize,
    pub mip_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdsSplitInfo {
    pub header_size: usize,
    pub split_offset: usize,
    pub low_res_width: usize,
    pub low_res_height: usize,
    pub low_res_mip_count: usize,
    pub cut_mip_count: usize,
}

pub struct DdsUtils;

impl DdsUtils {
    /// Detects the DXGI format and header length from raw DDS byte data
    /// supporting both block-compressed (BC1..BC7) and uncompressed (RGB/RGBA/BGRA/Luminance).
    pub fn detect_dxgi_format(data: &[u8]) -> (u32, usize) {
        if data.len() < 128 {
            return (0, 0);
        }

        let magic = u32::from_le_bytes(data[0..4].try_into().unwrap_or_default());
        if magic != DDS_MAGIC {
            return (0, 0);
        }

        let header: DdsHeader = bytemuck::pod_read_unaligned(&data[4..128]);
        let four_cc = header.ddspf.dw_four_cc;
        let pf_flags = header.ddspf.dw_flags;

        // 1. DX10 Extended Header ("DX10" = 0x30315844)
        if four_cc == 0x30315844 && data.len() >= 148 {
            let dxgi_format = u32::from_le_bytes(data[128..132].try_into().unwrap_or_default());
            if dxgi_format > 0 && dxgi_format < 120 {
                return (dxgi_format, 148);
            }
        }

        // 2. Block-Compressed FourCC matching
        let compressed_dxgi = match four_cc {
            0x31545844 | 0x55314342 | 0x53314342 => dxgi::BC1_UNORM, // DXT1, BC1U, BC1S
            0x32545844 | 0x33545844 => dxgi::BC2_UNORM,              // DXT2, DXT3
            0x34545844 | 0x35545844 | 0x55334342 | 0x53334342 => dxgi::BC3_UNORM, // DXT4, DXT5, BC3U, BC3S
            0x31434241 | 0x31495441 | 0x55344342 | 0x53344342 | 0x41544931 => dxgi::BC4_UNORM, // BC4A, ATI1, BC4U, BC4S
            0x32434241 | 0x32495441 | 0x55354342 | 0x53354342 | 0x20434433 | 0x20634433
            | 0x41544932 | 0x204E5844 => dxgi::BC5_UNORM, // BC5A, ATI2, BC5U, BC5S, 3DC, 3dc, DXN
            0x48364342 => dxgi::BC6H_UF16,              // BC6H
            0x20374342 | 0x00374342 => dxgi::BC7_UNORM, // BC7
            _ => 0,
        };

        if compressed_dxgi != 0 {
            return (compressed_dxgi, 128);
        }

        // 3. Uncompressed RGB / RGBA / BGRA / Grayscale Channel Mask Matching
        let bit_count = header.ddspf.dw_rgb_bit_count;
        let r_mask = header.ddspf.dw_r_bit_mask;
        let g_mask = header.ddspf.dw_g_bit_mask;
        let b_mask = header.ddspf.dw_b_bit_mask;
        let a_mask = header.ddspf.dw_a_bit_mask;

        let uncompressed_dxgi = match bit_count {
            32 => {
                if r_mask == 0x00FF0000 && g_mask == 0x0000FF00 && b_mask == 0x000000FF {
                    dxgi::B8G8R8A8_UNORM // 32-bit BGRA (Standard for CryEngine / Windows)
                } else if r_mask == 0x000000FF && g_mask == 0x0000FF00 && b_mask == 0x00FF0000 {
                    dxgi::R8G8B8A8_UNORM // 32-bit RGBA
                } else if r_mask == 0x000003FF && g_mask == 0x000FFC00 && b_mask == 0x3FF00000 {
                    dxgi::R10G10B10A2_UNORM // 10:10:10:2 HDR
                } else if r_mask == 0x0000FFFF && g_mask == 0xFFFF0000 {
                    dxgi::R16G16_UNORM // 16-bit 2-channel
                } else if r_mask == 0xFFFFFFFF && g_mask == 0 && b_mask == 0 {
                    dxgi::R32_FLOAT // 32-bit single channel float
                } else {
                    dxgi::B8G8R8A8_UNORM
                }
            }
            24 => dxgi::B8G8R8X8_UNORM, // 24-bit BGR
            16 => {
                if r_mask == 0xF800 && g_mask == 0x07E0 && b_mask == 0x001F {
                    dxgi::B5G6R5_UNORM // 16-bit RGB 5:6:5
                } else if r_mask == 0x7C00 && g_mask == 0x03E0 && b_mask == 0x001F {
                    dxgi::B5G5R5A1_UNORM // 16-bit RGBA 5:5:5:1
                } else if r_mask == 0x0F00 && g_mask == 0x00F0 && b_mask == 0x000F {
                    dxgi::B4G4R4A4_UNORM // 16-bit RGBA 4:4:4:4
                } else if (pf_flags & DDPF_LUMINANCE) != 0 || r_mask == 0xFFFF {
                    dxgi::R16_UNORM // 16-bit Grayscale
                } else if r_mask == 0x00FF && g_mask == 0xFF00 {
                    dxgi::R8G8_UNORM // 16-bit RG
                } else {
                    dxgi::B5G6R5_UNORM
                }
            }
            8 => {
                if (pf_flags & DDPF_ALPHA) != 0 || a_mask == 0xFF {
                    dxgi::A8_UNORM
                } else {
                    dxgi::R8_UNORM // 8-bit Grayscale
                }
            }
            _ => 0,
        };

        (uncompressed_dxgi, 128)
    }

    /// Reads basic resolution and mipmap count from the DDS header.
    pub fn get_header_info(data: &[u8]) -> Option<DdsBasicInfo> {
        if data.len() < 128 {
            return None;
        }

        let magic = u32::from_le_bytes(data[0..4].try_into().ok()?);
        if magic != DDS_MAGIC {
            return None;
        }

        let header: DdsHeader = bytemuck::pod_read_unaligned(&data[4..128]);
        Some(DdsBasicInfo {
            width: header.dw_width.max(1) as usize,
            height: header.dw_height.max(1) as usize,
            mip_count: max(1, header.dw_mip_map_count as usize),
        })
    }

    /// Calculates the split offset separating high-res Mip payloads from low-res Tail Mips.
    pub fn calculate_split(data: &[u8], max_tail_dim: usize) -> Option<DdsSplitInfo> {
        if data.len() < 128 {
            return None;
        }

        let magic = u32::from_le_bytes(data[0..4].try_into().ok()?);
        if magic != DDS_MAGIC {
            return None;
        }

        let header: DdsHeader = bytemuck::pod_read_unaligned(&data[4..128]);
        if header.dw_size != 124 {
            return None;
        }

        let width = header.dw_width.max(1) as usize;
        let height = header.dw_height.max(1) as usize;
        let mips = if header.dw_mip_map_count == 0 {
            1
        } else {
            header.dw_mip_map_count as usize
        };

        if width <= max_tail_dim && height <= max_tail_dim {
            return None;
        }

        let (dxgi_fmt, header_size) = Self::detect_dxgi_format(data);
        if dxgi_fmt == 0 {
            return None;
        }

        let is_block =
            crate::graphics::dxgi_format::D3D12FormatTable::is_block_compressed(dxgi_fmt);
        let element_size =
            crate::graphics::dxgi_format::D3D12FormatTable::get_element_size(dxgi_fmt)
                .unwrap_or(16);

        let mut current_offset = header_size;
        let mut w = width;
        let mut h = height;
        let mut split_offset = None;
        let mut cut_mips = 0;
        let mut low_res_w = w;
        let mut low_res_h = h;

        for i in 0..mips {
            if split_offset.is_none() && w <= max_tail_dim && h <= max_tail_dim {
                split_offset = Some(current_offset);
                cut_mips = i;
                low_res_w = w;
                low_res_h = h;
                break;
            }

            let mip_bytes = if is_block {
                let blocks_w = w.div_ceil(4);
                let blocks_h = h.div_ceil(4);
                blocks_w * blocks_h * element_size
            } else {
                w * h * element_size
            };

            current_offset += mip_bytes;
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }

        let split_off = split_offset?;
        if split_off >= data.len() {
            return None;
        }

        Some(DdsSplitInfo {
            header_size,
            split_offset: split_off,
            cut_mip_count: cut_mips,
            low_res_width: low_res_w,
            low_res_height: low_res_h,
            low_res_mip_count: mips - cut_mips,
        })
    }

    /// Rearranges the texture data layout for GPU streaming.
    pub fn process_texture_for_streaming(source: &[u8], max_tail_dim: usize) -> (Vec<u8>, usize) {
        let info = match Self::calculate_split(source, max_tail_dim) {
            Some(i) => i,
            None => return (source.to_vec(), source.len()),
        };

        let payload_size = info.split_offset - info.header_size;
        let tail_mips_size = source.len() - info.split_offset;
        let tail_size = info.header_size + tail_mips_size;
        let total_size = tail_size + payload_size;

        let mut result = vec![0u8; total_size];
        result[0..info.header_size].copy_from_slice(&source[0..info.header_size]);

        let mut header: DdsHeader = bytemuck::pod_read_unaligned(&result[4..128]);
        header.dw_width = info.low_res_width as u32;
        header.dw_height = info.low_res_height as u32;
        header.dw_mip_map_count = info.low_res_mip_count as u32;
        header.dw_pitch_or_linear_size = 0;
        header.dw_caps |= DDSCAPS_COMPLEX | DDSCAPS_MIPMAP;
        header.dw_caps2 |= DDSCAPS2_MIPMAPSUBTREE;

        let header_bytes = bytemuck::bytes_of(&header);
        result[4..128].copy_from_slice(header_bytes);

        result[info.header_size..info.header_size + tail_mips_size]
            .copy_from_slice(&source[info.split_offset..source.len()]);

        result[tail_size..total_size].copy_from_slice(&source[info.header_size..info.split_offset]);

        (result, tail_size)
    }
}
