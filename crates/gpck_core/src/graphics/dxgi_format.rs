// src/graphics/dxgi_format.rs
//! # DirectX Graphics Infrastructure (DXGI) & D3D12 Format Property Table
//!
//! Direct Rust port of Microsoft's `d3dx12_property_format_table.h` and `d3dx12_resource_helpers.h`.
//! Provides bit-exact layout calculations for RowPitch (256B aligned), Placement Alignment (512B),
//! Subresource Footprints, 64KB/4KB Tiled Resource shapes, and Packed Mip-tail metadata.

pub mod dxgi {
    pub const UNKNOWN: u32 = 0;
    pub const R32G32B32A32_TYPELESS: u32 = 1;
    pub const R32G32B32A32_FLOAT: u32 = 2;
    pub const R32G32B32A32_UINT: u32 = 3;
    pub const R32G32B32A32_SINT: u32 = 4;
    pub const R32G32B32_TYPELESS: u32 = 5;
    pub const R32G32B32_FLOAT: u32 = 6;
    pub const R32G32B32_UINT: u32 = 7;
    pub const R32G32B32_SINT: u32 = 8;
    pub const R16G16B16A16_TYPELESS: u32 = 9;
    pub const R16G16B16A16_FLOAT: u32 = 10;
    pub const R16G16B16A16_UNORM: u32 = 11;
    pub const R16G16B16A16_UINT: u32 = 12;
    pub const R16G16B16A16_SNORM: u32 = 13;
    pub const R16G16B16A16_SINT: u32 = 14;
    pub const R32G32_TYPELESS: u32 = 15;
    pub const R32G32_FLOAT: u32 = 16;
    pub const R32G32_UINT: u32 = 17;
    pub const R32G32_SINT: u32 = 18;
    pub const R32G8X24_TYPELESS: u32 = 19;
    pub const D32_FLOAT_S8X24_UINT: u32 = 20;
    pub const R32_FLOAT_X8X24_TYPELESS: u32 = 21;
    pub const X32_TYPELESS_G8X24_UINT: u32 = 22;
    pub const R10G10B10A2_TYPELESS: u32 = 23;
    pub const R10G10B10A2_UNORM: u32 = 24;
    pub const R10G10B10A2_UINT: u32 = 25;
    pub const R11G11B10_FLOAT: u32 = 26;
    pub const R8G8B8A8_TYPELESS: u32 = 27;
    pub const R8G8B8A8_UNORM: u32 = 28;
    pub const R8G8B8A8_UNORM_SRGB: u32 = 29;
    pub const R8G8B8A8_UINT: u32 = 30;
    pub const R8G8B8A8_SNORM: u32 = 31;
    pub const R8G8B8A8_SINT: u32 = 32;
    pub const R16G16_TYPELESS: u32 = 33;
    pub const R16G16_FLOAT: u32 = 34;
    pub const R16G16_UNORM: u32 = 35;
    pub const R16G16_UINT: u32 = 36;
    pub const R16G16_SNORM: u32 = 37;
    pub const R16G16_SINT: u32 = 38;
    pub const R32_TYPELESS: u32 = 39;
    pub const D32_FLOAT: u32 = 40;
    pub const R32_FLOAT: u32 = 41;
    pub const R32_UINT: u32 = 42;
    pub const R32_SINT: u32 = 43;
    pub const R24G8_TYPELESS: u32 = 44;
    pub const D24_UNORM_S8_UINT: u32 = 45;
    pub const R24_UNORM_X8_TYPELESS: u32 = 46;
    pub const X24_TYPELESS_G8_UINT: u32 = 47;
    pub const R8G8_TYPELESS: u32 = 48;
    pub const R8G8_UNORM: u32 = 49;
    pub const R8G8_UINT: u32 = 50;
    pub const R8G8_SNORM: u32 = 51;
    pub const R8G8_SINT: u32 = 52;
    pub const R16_TYPELESS: u32 = 53;
    pub const R16_FLOAT: u32 = 54;
    pub const D16_UNORM: u32 = 55;
    pub const R16_UNORM: u32 = 56;
    pub const R16_UINT: u32 = 57;
    pub const R16_SNORM: u32 = 58;
    pub const R16_SINT: u32 = 59;
    pub const R8_TYPELESS: u32 = 60;
    pub const R8_UNORM: u32 = 61;
    pub const R8_UINT: u32 = 62;
    pub const R8_SNORM: u32 = 63;
    pub const R8_SINT: u32 = 64;
    pub const A8_UNORM: u32 = 65;
    pub const R1_UNORM: u32 = 66;
    pub const R9G9B9E5_SHAREDEXP: u32 = 67;
    pub const R8G8_B8G8_UNORM: u32 = 68;
    pub const G8R8_G8B8_UNORM: u32 = 69;

    // BC1 (DXT1)
    pub const BC1_TYPELESS: u32 = 70;
    pub const BC1_UNORM: u32 = 71;
    pub const BC1_UNORM_SRGB: u32 = 72;

    // BC2 (DXT2/3)
    pub const BC2_TYPELESS: u32 = 73;
    pub const BC2_UNORM: u32 = 74;
    pub const BC2_UNORM_SRGB: u32 = 75;

    // BC3 (DXT4/5)
    pub const BC3_TYPELESS: u32 = 76;
    pub const BC3_UNORM: u32 = 77;
    pub const BC3_UNORM_SRGB: u32 = 78;

    // BC4 (ATI1)
    pub const BC4_TYPELESS: u32 = 79;
    pub const BC4_UNORM: u32 = 80;
    pub const BC4_SNORM: u32 = 81;

    // BC5 (ATI2 / 3Dc)
    pub const BC5_TYPELESS: u32 = 82;
    pub const BC5_UNORM: u32 = 83;
    pub const BC5_SNORM: u32 = 84;

    pub const B5G6R5_UNORM: u32 = 85;
    pub const B5G5R5A1_UNORM: u32 = 86;
    pub const B8G8R8A8_UNORM: u32 = 87;
    pub const B8G8R8X8_UNORM: u32 = 88;
    pub const R10G10B10_XR_BIAS_A2_UNORM: u32 = 89;
    pub const B8G8R8A8_TYPELESS: u32 = 90;
    pub const B8G8R8A8_UNORM_SRGB: u32 = 91;
    pub const B8G8R8X8_TYPELESS: u32 = 92;
    pub const B8G8R8X8_UNORM_SRGB: u32 = 93;

    // BC6H (HDR Half-Float)
    pub const BC6H_TYPELESS: u32 = 94;
    pub const BC6H_UF16: u32 = 95;
    pub const BC6H_SF16: u32 = 96;

    // BC7
    pub const BC7_TYPELESS: u32 = 97;
    pub const BC7_UNORM: u32 = 98;
    pub const BC7_UNORM_SRGB: u32 = 99;

    // Planar & Video Formats
    pub const AYUV: u32 = 100;
    pub const Y410: u32 = 101;
    pub const Y416: u32 = 102;
    pub const NV12: u32 = 103;
    pub const P010: u32 = 104;
    pub const P016: u32 = 105;
    pub const OPAQUE_420: u32 = 106;
    pub const YUY2: u32 = 107;
    pub const Y210: u32 = 108;
    pub const Y216: u32 = 109;
    pub const NV11: u32 = 110;
    pub const AI44: u32 = 111;
    pub const IA44: u32 = 112;
    pub const P8: u32 = 113;
    pub const A8P8: u32 = 114;
    pub const B4G4R4A4_UNORM: u32 = 115;

    pub const P208: u32 = 130;
    pub const V208: u32 = 131;
    pub const V408: u32 = 132;

    pub const SAMPLER_FEEDBACK_MIN_MIP_OPAQUE: u32 = 189;
    pub const SAMPLER_FEEDBACK_MIP_REGION_USED_OPAQUE: u32 = 190;
    pub const A4B4G4R4_UNORM: u32 = 191;
}

/// Direct3D 12 row pitch alignment constant (256 bytes).
pub const D3D12_TEXTURE_DATA_PITCH_ALIGNMENT: u32 = 256;

/// Direct3D 12 texture data placement alignment constant (512 bytes).
pub const D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT: u64 = 512;

/// Standard 64KB Tiled Resource size.
pub const D3D12_TILED_RESOURCE_TILE_SIZE_IN_BYTES: u32 = 65536;

/// 4KB Tiled Resource size.
pub const D3D12_4KB_TILED_RESOURCE_TILE_SIZE_IN_BYTES: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct D3D12SubresourceFootprint {
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub row_pitch: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct D3D12PlacedSubresourceFootprint {
    pub offset: u64,
    pub footprint: D3D12SubresourceFootprint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct D3D12TileShape {
    pub width_in_texels: u32,
    pub height_in_texels: u32,
    pub depth_in_texels: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct D3D12SubresourceTiling {
    pub width_in_tiles: u32,
    pub height_in_tiles: u16,
    pub depth_in_tiles: u16,
    pub start_tile_index_in_overall_resource: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct D3D12PackedMipInfo {
    pub num_standard_mips: u8,
    pub num_packed_mips: u8,
    pub num_tiles_for_packed_mips: u32,
    pub start_tile_index_in_overall_resource: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct D3D12TiledResourceCoordinate {
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub subresource: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct D3D12TileRegionSize {
    pub num_tiles: u32,
    pub use_box: bool,
    pub width: u32,
    pub height: u16,
    pub depth: u16,
}

pub struct D3D12FormatTable;

impl D3D12FormatTable {
    /// Returns true if the format is block-compressed (BC1 through BC7).
    #[inline(always)]
    pub fn is_block_compressed(format: u32) -> bool {
        matches!(
            format,
            dxgi::BC1_TYPELESS..=dxgi::BC5_SNORM | dxgi::BC6H_TYPELESS..=dxgi::BC7_UNORM_SRGB
        )
    }

    #[inline(always)]
    pub fn is_bc1(format: u32) -> bool {
        matches!(format, dxgi::BC1_TYPELESS..=dxgi::BC1_UNORM_SRGB)
    }

    #[inline(always)]
    pub fn is_bc2(format: u32) -> bool {
        matches!(format, dxgi::BC2_TYPELESS..=dxgi::BC2_UNORM_SRGB)
    }

    #[inline(always)]
    pub fn is_bc3(format: u32) -> bool {
        matches!(format, dxgi::BC3_TYPELESS..=dxgi::BC3_UNORM_SRGB)
    }

    #[inline(always)]
    pub fn is_bc4(format: u32) -> bool {
        matches!(format, dxgi::BC4_TYPELESS..=dxgi::BC4_SNORM)
    }

    #[inline(always)]
    pub fn is_bc5(format: u32) -> bool {
        matches!(format, dxgi::BC5_TYPELESS..=dxgi::BC5_SNORM)
    }

    #[inline(always)]
    pub fn is_bc6h(format: u32) -> bool {
        matches!(format, dxgi::BC6H_TYPELESS..=dxgi::BC6H_SF16)
    }

    #[inline(always)]
    pub fn is_bc7(format: u32) -> bool {
        matches!(format, dxgi::BC7_TYPELESS..=dxgi::BC7_UNORM_SRGB)
    }

    #[inline(always)]
    pub fn get_element_size(format: u32) -> Option<usize> {
        if Self::is_block_compressed(format) {
            Some((Self::get_bits_per_unit(format) / 8) as usize)
        } else {
            let bpu = Self::get_bits_per_unit(format);
            if bpu > 0 {
                Some((bpu / 8).max(1) as usize)
            } else {
                None
            }
        }
    }

    pub fn get_bits_per_unit(format: u32) -> u32 {
        match format {
            dxgi::R32G32B32A32_TYPELESS..=dxgi::R32G32B32A32_SINT => 128,
            dxgi::R32G32B32_TYPELESS..=dxgi::R32G32B32_SINT => 96,
            dxgi::R16G16B16A16_TYPELESS..=dxgi::R16G16B16A16_SINT => 64,
            dxgi::R32G32_TYPELESS..=dxgi::R32G32_SINT => 64,
            dxgi::R10G10B10A2_TYPELESS..=dxgi::R11G11B10_FLOAT => 32,
            dxgi::R8G8B8A8_TYPELESS..=dxgi::R8G8B8A8_SINT => 32,
            dxgi::R16G16_TYPELESS..=dxgi::R16G16_SINT => 32,
            dxgi::R32_TYPELESS..=dxgi::R32_SINT => 32,
            dxgi::R8G8_TYPELESS..=dxgi::R8G8_SINT => 16,
            dxgi::R16_TYPELESS..=dxgi::R16_SINT => 16,
            dxgi::R8_TYPELESS..=dxgi::A8_UNORM => 8,
            dxgi::R1_UNORM => 1,
            dxgi::B5G6R5_UNORM | dxgi::B5G5R5A1_UNORM => 16,
            dxgi::B8G8R8A8_UNORM..=dxgi::B8G8R8X8_UNORM_SRGB => 32,

            // BC1 & BC4: 64 bits (8 bytes) per 4x4 block
            dxgi::BC1_TYPELESS..=dxgi::BC1_UNORM_SRGB | dxgi::BC4_TYPELESS..=dxgi::BC4_SNORM => 64,

            // BC2, BC3, BC5, BC6H, BC7: 128 bits (16 bytes) per 4x4 block
            dxgi::BC2_TYPELESS..=dxgi::BC3_UNORM_SRGB
            | dxgi::BC5_TYPELESS..=dxgi::BC5_SNORM
            | dxgi::BC6H_TYPELESS..=dxgi::BC7_UNORM_SRGB => 128,

            // Planar YUV Formats
            dxgi::NV12 => 8,
            dxgi::P010 | dxgi::P016 | dxgi::YUY2 => 16,
            _ => 0,
        }
    }

    /// Width alignment in texels (4 for block-compressed BCn, 1 for uncompressed).
    #[inline(always)]
    pub fn get_width_alignment(format: u32) -> u32 {
        if Self::is_block_compressed(format) {
            4
        } else {
            1
        }
    }

    /// Height alignment in texels (4 for block-compressed BCn, 1 for uncompressed).
    #[inline(always)]
    pub fn get_height_alignment(format: u32) -> u32 {
        if Self::is_block_compressed(format) {
            4
        } else {
            1
        }
    }

    pub fn calculate_minimum_row_pitch(format: u32, width_pixels: u32) -> u32 {
        let bpu = Self::get_bits_per_unit(format);
        if Self::is_block_compressed(format) {
            let num_blocks = width_pixels.div_ceil(4);
            (num_blocks * bpu) >> 3
        } else {
            let width_align = Self::get_width_alignment(format);
            let num_units = (width_pixels + width_align - 1) & !(width_align - 1);
            (num_units * bpu) >> 3
        }
    }

    #[inline(always)]
    pub fn calculate_d3d12_aligned_row_pitch(format: u32, width_pixels: u32) -> u32 {
        let tight_pitch = Self::calculate_minimum_row_pitch(format, width_pixels);
        (tight_pitch + D3D12_TEXTURE_DATA_PITCH_ALIGNMENT - 1)
            & !(D3D12_TEXTURE_DATA_PITCH_ALIGNMENT - 1)
    }

    #[inline(always)]
    pub fn get_mip_dimensions(
        mip_level: u32,
        width: u32,
        height: u32,
        depth: u32,
    ) -> (u32, u32, u32) {
        let mip_w = (width >> mip_level).max(1);
        let mip_h = (height >> mip_level).max(1);
        let mip_d = (depth >> mip_level).max(1);
        (mip_w, mip_h, mip_d)
    }

    /// Generates placed subresource footprints for copying and DirectStorage streaming.
    /// Matches `ID3D12Device::GetCopyableFootprints` / `D3DX12GetCopyableFootprints`.
    pub fn get_copyable_footprints(
        format: u32,
        width: u32,
        height: u32,
        depth: u32,
        mip_levels: u32,
        array_size: u32,
        base_offset: u64,
    ) -> (Vec<D3D12PlacedSubresourceFootprint>, u64) {
        let mut footprints = Vec::with_capacity((mip_levels * array_size) as usize);
        let mut current_offset = base_offset;

        for _layer in 0..array_size {
            for mip in 0..mip_levels {
                let (mip_w, mip_h, mip_d) = Self::get_mip_dimensions(mip, width, height, depth);
                let row_pitch = Self::calculate_d3d12_aligned_row_pitch(format, mip_w);

                let num_rows = if Self::is_block_compressed(format) {
                    mip_h.div_ceil(4)
                } else {
                    mip_h
                };

                let slice_size = (row_pitch as u64) * (num_rows as u64);
                let subresource_size = slice_size * (mip_d as u64);

                let aligned_offset = (current_offset + D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT - 1)
                    & !(D3D12_TEXTURE_DATA_PLACEMENT_ALIGNMENT - 1);

                footprints.push(D3D12PlacedSubresourceFootprint {
                    offset: aligned_offset,
                    footprint: D3D12SubresourceFootprint {
                        format,
                        width: mip_w,
                        height: mip_h,
                        depth: mip_d,
                        row_pitch,
                    },
                });

                current_offset = aligned_offset + subresource_size;
            }
        }

        (footprints, current_offset - base_offset)
    }

    /// Computes the 64KB Tiled Resource tile shape for sparse / DirectStorage tiled streaming.
    pub fn get_tile_shape_64k(format: u32, is_3d: bool) -> D3D12TileShape {
        let bpu = Self::get_bits_per_unit(format);

        if is_3d {
            if Self::is_block_compressed(format) {
                let factor = if bpu == 64 { 2 } else { 1 };
                D3D12TileShape {
                    width_in_texels: 16 * Self::get_width_alignment(format) * factor,
                    height_in_texels: 16 * Self::get_height_alignment(format),
                    depth_in_texels: 16,
                }
            } else {
                match bpu {
                    8 => D3D12TileShape {
                        width_in_texels: 64,
                        height_in_texels: 32,
                        depth_in_texels: 32,
                    },
                    16 => D3D12TileShape {
                        width_in_texels: 32,
                        height_in_texels: 32,
                        depth_in_texels: 32,
                    },
                    32 => D3D12TileShape {
                        width_in_texels: 32,
                        height_in_texels: 32,
                        depth_in_texels: 16,
                    },
                    64 => D3D12TileShape {
                        width_in_texels: 32,
                        height_in_texels: 16,
                        depth_in_texels: 16,
                    },
                    _ => D3D12TileShape {
                        width_in_texels: 16,
                        height_in_texels: 16,
                        depth_in_texels: 16,
                    },
                }
            }
        } else if Self::is_block_compressed(format) {
            let factor = if bpu == 64 { 2 } else { 1 };
            D3D12TileShape {
                width_in_texels: 64 * Self::get_width_alignment(format) * factor,
                height_in_texels: 64 * Self::get_height_alignment(format),
                depth_in_texels: 1,
            }
        } else {
            match bpu {
                8 => D3D12TileShape {
                    width_in_texels: 256,
                    height_in_texels: 256,
                    depth_in_texels: 1,
                },
                16 => D3D12TileShape {
                    width_in_texels: 256,
                    height_in_texels: 128,
                    depth_in_texels: 1,
                },
                32 => D3D12TileShape {
                    width_in_texels: 128,
                    height_in_texels: 128,
                    depth_in_texels: 1,
                },
                64 => D3D12TileShape {
                    width_in_texels: 128,
                    height_in_texels: 64,
                    depth_in_texels: 1,
                },
                _ => D3D12TileShape {
                    width_in_texels: 64,
                    height_in_texels: 64,
                    depth_in_texels: 1,
                },
            }
        }
    }

    /// Computes the 4KB Tiled Resource tile shape.
    pub fn get_4k_tile_shape(format: u32, is_3d: bool) -> D3D12TileShape {
        let shape_64k = Self::get_tile_shape_64k(format, is_3d);
        if is_3d {
            D3D12TileShape {
                width_in_texels: (shape_64k.width_in_texels / 2).max(1),
                height_in_texels: (shape_64k.height_in_texels / 2).max(1),
                depth_in_texels: (shape_64k.depth_in_texels / 4).max(1),
            }
        } else {
            D3D12TileShape {
                width_in_texels: (shape_64k.width_in_texels / 4).max(1),
                height_in_texels: (shape_64k.height_in_texels / 4).max(1),
                depth_in_texels: 1,
            }
        }
    }

    /// Computes subresource tilings and packed mip metadata for Sparse / Virtual Texturing.
    /// Matches `ID3D12Device::GetResourceTiling`.
    pub fn calculate_subresource_tilings(
        format: u32,
        width: u32,
        height: u32,
        depth: u32,
        mip_levels: u32,
        array_size: u32,
    ) -> (Vec<D3D12SubresourceTiling>, D3D12PackedMipInfo, u32) {
        let is_3d = depth > 1;
        let tile_shape = Self::get_tile_shape_64k(format, is_3d);
        let mut tilings = Vec::with_capacity((mip_levels * array_size) as usize);

        let mut total_tiles = 0u32;
        let mut num_standard_mips = 0u8;
        let mut num_packed_mips = 0u8;
        let mut packed_mips_start_tile = 0u32;

        // Determine which mip levels occupy at least one 64KB tile vs packed mips
        for mip in 0..mip_levels {
            let (mip_w, mip_h, mip_d) = Self::get_mip_dimensions(mip, width, height, depth);
            if mip_w >= tile_shape.width_in_texels
                && mip_h >= tile_shape.height_in_texels
                && mip_d >= tile_shape.depth_in_texels
            {
                num_standard_mips += 1;
            } else {
                num_packed_mips = (mip_levels - mip) as u8;
                break;
            }
        }

        // 1. Compute standard mip tilings
        for _layer in 0..array_size {
            for mip in 0..num_standard_mips as u32 {
                let (mip_w, mip_h, mip_d) = Self::get_mip_dimensions(mip, width, height, depth);

                let tiles_x = mip_w.div_ceil(tile_shape.width_in_texels);
                let tiles_y = mip_h.div_ceil(tile_shape.height_in_texels);
                let tiles_z = mip_d.div_ceil(tile_shape.depth_in_texels);
                let subresource_tiles = tiles_x * tiles_y * tiles_z;

                tilings.push(D3D12SubresourceTiling {
                    width_in_tiles: tiles_x,
                    height_in_tiles: tiles_y as u16,
                    depth_in_tiles: tiles_z as u16,
                    start_tile_index_in_overall_resource: total_tiles,
                });

                total_tiles += subresource_tiles;
            }

            // For packed mips in this array layer
            if num_packed_mips > 0 {
                packed_mips_start_tile = total_tiles;
                for _ in 0..num_packed_mips {
                    tilings.push(D3D12SubresourceTiling {
                        width_in_tiles: 0,
                        height_in_tiles: 0,
                        depth_in_tiles: 0,
                        start_tile_index_in_overall_resource: u32::MAX, // Sentinel for packed mip
                    });
                }
            }
        }

        // 2. Compute 64KB tile count for all packed tail mips
        let num_tiles_for_packed_mips = if num_packed_mips > 0 {
            let mut packed_bytes = 0u64;
            for mip in (num_standard_mips as u32)..mip_levels {
                let (mip_w, mip_h, mip_d) = Self::get_mip_dimensions(mip, width, height, depth);
                let pitch = Self::calculate_minimum_row_pitch(format, mip_w);
                let rows = if Self::is_block_compressed(format) {
                    mip_h.div_ceil(4)
                } else {
                    mip_h
                };
                packed_bytes += (pitch as u64) * (rows as u64) * (mip_d as u64);
            }
            let tiles = (packed_bytes * array_size as u64).div_ceil(65536) as u32;
            total_tiles += tiles;
            tiles
        } else {
            0
        };

        let packed_info = D3D12PackedMipInfo {
            num_standard_mips,
            num_packed_mips,
            num_tiles_for_packed_mips,
            start_tile_index_in_overall_resource: if num_packed_mips > 0 {
                packed_mips_start_tile
            } else {
                u32::MAX
            },
        };

        (tilings, packed_info, total_tiles)
    }

    /// Converts a texel coordinate (X, Y, Z) to a `D3D12TiledResourceCoordinate` for tile streaming.
    pub fn calculate_tile_coordinate(
        format: u32,
        subresource: u32,
        texel_x: u32,
        texel_y: u32,
        texel_z: u32,
        is_3d: bool,
    ) -> D3D12TiledResourceCoordinate {
        let shape = Self::get_tile_shape_64k(format, is_3d);
        D3D12TiledResourceCoordinate {
            x: texel_x / shape.width_in_texels,
            y: texel_y / shape.height_in_texels,
            z: texel_z / shape.depth_in_texels,
            subresource,
        }
    }
}
