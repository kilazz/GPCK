// src/format/ktx2.rs
//! # Khronos Texture 2.0 (KTX2) Container Parser & VkFormat Mapper
//!
//! Provides zero-allocation header parsing, level index table resolution,
//! Zstandard supercompression decoding, and 1:1 mapping from Vulkan `VkFormat`
//! to standard block-compressed engine formats for GACL texture conditioning.

use crate::graphics::dxgi_format::dxgi;
use bytemuck::{Pod, Zeroable};

/// 12-byte KTX2 File Identifier magic header bytes.
pub const KTX2_MAGIC: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];

pub const KTX2_SUPERCOMPRESSION_NONE: u32 = 0;
pub const KTX2_SUPERCOMPRESSION_BASIS_LZ: u32 = 1;
pub const KTX2_SUPERCOMPRESSION_ZSTD: u32 = 2;
pub const KTX2_SUPERCOMPRESSION_ZLIB: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct Ktx2Header {
    pub magic: [u8; 12],
    pub vk_format: u32,
    pub type_size: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub pixel_depth: u32,
    pub layer_count: u32,
    pub face_count: u32,
    pub level_count: u32,
    pub supercompression_scheme: u32,
    pub dfd_byte_offset: u32,
    pub dfd_byte_length: u32,
    pub kvd_byte_offset: u32,
    pub kvd_byte_length: u32,
    pub sgd_byte_offset: u64,
    pub sgd_byte_length: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct Ktx2LevelIndex {
    pub byte_offset: u64,
    pub byte_length: u64,
    pub uncompressed_byte_length: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ktx2BasicInfo {
    pub width: usize,
    pub height: usize,
    pub mip_count: usize,
    pub vk_format: u32,
    pub dxgi_format: u32,
    pub supercompression_scheme: u32,
}

pub struct Ktx2Utils;

impl Ktx2Utils {
    /// Returns true if the buffer starts with the KTX2 magic identifier.
    #[inline(always)]
    pub fn is_ktx2(data: &[u8]) -> bool {
        if data.len() < 80 {
            return false;
        }
        data[0..12] == KTX2_MAGIC
    }

    /// Reads basic resolution, format, and mipmap metadata from the KTX2 header.
    pub fn get_header_info(data: &[u8]) -> Option<Ktx2BasicInfo> {
        if !Self::is_ktx2(data) {
            return None;
        }

        let header: Ktx2Header = bytemuck::pod_read_unaligned(&data[0..80]);
        let width = header.pixel_width as usize;
        let height = header.pixel_height.max(1) as usize;
        let mip_count = header.level_count.max(1) as usize;
        let dxgi_format = Self::vk_format_to_dxgi_format(header.vk_format);

        Some(Ktx2BasicInfo {
            width,
            height,
            mip_count,
            vk_format: header.vk_format,
            dxgi_format,
            supercompression_scheme: header.supercompression_scheme,
        })
    }

    /// Maps standard Vulkan `VkFormat` enum integers to DirectX `DXGI_FORMAT` constants.
    pub fn vk_format_to_dxgi_format(vk_format: u32) -> u32 {
        match vk_format {
            // BC1 (DXT1)
            131 | 133 => dxgi::BC1_UNORM,
            132 | 134 => dxgi::BC1_UNORM_SRGB,

            // BC2 (DXT3)
            135 => dxgi::BC2_UNORM,
            136 => dxgi::BC2_UNORM_SRGB,

            // BC3 (DXT5)
            137 => dxgi::BC3_UNORM,
            138 => dxgi::BC3_UNORM_SRGB,

            // BC4 (ATI1)
            139 => dxgi::BC4_UNORM,
            140 => dxgi::BC4_SNORM,

            // BC5 (ATI2 / 3Dc)
            141 => dxgi::BC5_UNORM,
            142 => dxgi::BC5_SNORM,

            // BC6H (HDR Half-Float)
            143 => dxgi::BC6H_UF16,
            144 => dxgi::BC6H_SF16,

            // BC7
            145 => dxgi::BC7_UNORM,
            146 => dxgi::BC7_UNORM_SRGB,

            // Uncompressed RGBA formats
            37 => dxgi::R8G8B8A8_UNORM,
            43 => dxgi::R8G8B8A8_UNORM_SRGB,
            44 => dxgi::B8G8R8A8_UNORM,
            50 => dxgi::B8G8R8A8_UNORM_SRGB,
            97 => dxgi::R16G16B16A16_FLOAT,
            109 => dxgi::R32G32B32A32_FLOAT,

            _ => dxgi::UNKNOWN,
        }
    }

    /// Extracts and decompresses Level 0 block payload from a KTX2 byte stream.
    /// Returns `(decompressed_payload, dxgi_format, width, height, mip_count)`.
    pub fn extract_texture_payload(data: &[u8]) -> Option<(Vec<u8>, u32, usize, usize, usize)> {
        if !Self::is_ktx2(data) {
            return None;
        }

        let header: Ktx2Header = bytemuck::pod_read_unaligned(&data[0..80]);
        let width = header.pixel_width as usize;
        let height = header.pixel_height.max(1) as usize;
        let mip_count = header.level_count.max(1) as usize;
        let dxgi_format = Self::vk_format_to_dxgi_format(header.vk_format);

        if width == 0 || height == 0 || dxgi_format == dxgi::UNKNOWN {
            return None;
        }

        let level_index_offset = 80usize;
        let level_index_size = std::mem::size_of::<Ktx2LevelIndex>();
        let level0_entry_offset = level_index_offset;

        if level0_entry_offset + level_index_size > data.len() {
            return None;
        }

        let level0_index: Ktx2LevelIndex = bytemuck::pod_read_unaligned(
            &data[level0_entry_offset..level0_entry_offset + level_index_size],
        );

        let payload_start = level0_index.byte_offset as usize;
        let payload_len = level0_index.byte_length as usize;

        if payload_start + payload_len > data.len() {
            return None;
        }

        let raw_slice = &data[payload_start..payload_start + payload_len];

        // Handle Supercompression Schemes
        let uncompressed_bytes = match header.supercompression_scheme {
            KTX2_SUPERCOMPRESSION_NONE => raw_slice.to_vec(),
            KTX2_SUPERCOMPRESSION_ZSTD => {
                let target_len = level0_index.uncompressed_byte_length as usize;
                zstd::bulk::decompress(raw_slice, target_len).ok()?
            }
            _ => return None, // BasisLZ requires transcoding
        };

        Some((uncompressed_bytes, dxgi_format, width, height, mip_count))
    }
}
