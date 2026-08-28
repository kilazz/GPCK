// crates/gpck_core/src/core/preset.rs
//! # Packaging Presets & Target Platform Quality Profiles

use crate::compression::codecs::CompressionMethod;
use crate::gacl::GaclTransform;
use crate::packer::{
    DEFAULT_CHUNK_SIZE, DEFAULT_MAX_PARTITION_SIZE, GaclFormatOverrides, PackerOptions,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PackerPreset {
    #[default]
    GpuStreaming,
    MobileAndroid,
    MaxCompression,
    FastDevBuild,
    SecureDelivery,
    Custom,
}

impl PackerPreset {
    pub const ALL_NAMES: &'static [&'static str] = &[
        "GPU Streaming (PC / DirectStorage)",
        "Mobile / Android (ASTC / ETC2 + LZ4)",
        "Maximum Compression (Distribution)",
        "Fast Dev Build (Iteration)",
        "Secure Delivery (Encrypted)",
        "Custom (User Defined)",
    ];

    pub fn from_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        if lower.contains("mobile") || lower.contains("android") {
            Self::MobileAndroid
        } else if lower.contains("gpu") || lower.contains("directstorage") {
            Self::GpuStreaming
        } else if lower.contains("max") || lower.contains("distribution") {
            Self::MaxCompression
        } else if lower.contains("fast") || lower.contains("dev") {
            Self::FastDevBuild
        } else if lower.contains("secure") || lower.contains("encrypted") {
            Self::SecureDelivery
        } else {
            Self::Custom
        }
    }

    pub fn to_packer_options(self, key_bytes: Option<[u8; 32]>) -> PackerOptions {
        match self {
            Self::GpuStreaming => PackerOptions {
                method: CompressionMethod::GDeflate,
                level: 9,
                chunk_size: DEFAULT_CHUNK_SIZE,
                enable_dedup: true,
                key: key_bytes,
                mip_split: true,
                max_tail_dim: 128,
                tags: 1,
                validate_chunks: true,
                max_partition_size: 64 * 1024 * 1024,
                atg_profile: true,
                tiled_streaming: true,
                min_tiled_resolution: 2048, // 2K/4K/8K Sparse Tiling Threshold
                min_tiled_tile_count: 8,
                gacl: GaclFormatOverrides {
                    auto_mode: true,
                    rdo_reduction_pct: 0.0,
                    rdo_use_ycocg: true,
                    rdo_bc1: true,
                    rdo_bc2: true,
                    rdo_bc3: true,
                    rdo_bc4: false,
                    rdo_bc5: false,
                    rdo_bc6h: false,
                    rdo_bc7: true,
                    ..Default::default()
                },
            },
            Self::MobileAndroid => PackerOptions {
                method: CompressionMethod::Lz4,
                level: 3,
                chunk_size: DEFAULT_CHUNK_SIZE,
                enable_dedup: true,
                key: key_bytes,
                mip_split: true,
                max_tail_dim: 64,
                tags: 1,
                validate_chunks: true,
                max_partition_size: 32 * 1024 * 1024,
                atg_profile: false,
                tiled_streaming: false,
                min_tiled_resolution: 0,
                min_tiled_tile_count: 0,
                gacl: GaclFormatOverrides {
                    auto_mode: true,
                    rdo_reduction_pct: 0.0,
                    rdo_use_ycocg: false,
                    ..Default::default()
                },
            },
            Self::MaxCompression => PackerOptions {
                method: CompressionMethod::Zstd,
                level: 19,
                chunk_size: DEFAULT_CHUNK_SIZE,
                enable_dedup: true,
                key: key_bytes,
                mip_split: false,
                max_tail_dim: 128,
                tags: 1,
                validate_chunks: true,
                max_partition_size: 256 * 1024 * 1024,
                atg_profile: false,
                tiled_streaming: false,
                min_tiled_resolution: 0,
                min_tiled_tile_count: 0,
                gacl: GaclFormatOverrides {
                    auto_mode: true,
                    rdo_reduction_pct: 8.0,
                    rdo_use_ycocg: true,
                    rdo_bc1: true,
                    rdo_bc2: true,
                    rdo_bc3: true,
                    rdo_bc4: false,
                    rdo_bc5: false,
                    rdo_bc6h: false,
                    rdo_bc7: true,
                    ..Default::default()
                },
            },
            Self::FastDevBuild => PackerOptions {
                method: CompressionMethod::Lz4,
                level: 3,
                chunk_size: DEFAULT_CHUNK_SIZE,
                enable_dedup: false,
                key: None,
                mip_split: false,
                max_tail_dim: 128,
                tags: 1,
                validate_chunks: false,
                max_partition_size: DEFAULT_MAX_PARTITION_SIZE,
                atg_profile: false,
                tiled_streaming: false,
                min_tiled_resolution: 0,
                min_tiled_tile_count: 0,
                gacl: GaclFormatOverrides {
                    auto_mode: false,
                    bc1_transform: Some(GaclTransform::None.to_u32()),
                    bc3_transform: Some(GaclTransform::None.to_u32()),
                    bc5_transform: Some(GaclTransform::None.to_u32()),
                    bc7_transform: Some(GaclTransform::None.to_u32()),
                    rdo_reduction_pct: 0.0,
                    rdo_use_ycocg: false,
                    ..Default::default()
                },
            },
            Self::SecureDelivery => PackerOptions {
                method: CompressionMethod::Zstd,
                level: 9,
                chunk_size: DEFAULT_CHUNK_SIZE,
                enable_dedup: true,
                key: key_bytes,
                mip_split: true,
                max_tail_dim: 128,
                tags: 1,
                validate_chunks: true,
                max_partition_size: 64 * 1024 * 1024,
                atg_profile: true,
                tiled_streaming: true,
                min_tiled_resolution: 2048,
                min_tiled_tile_count: 8,
                gacl: GaclFormatOverrides::default(),
            },
            Self::Custom => PackerOptions::default(),
        }
    }
}
