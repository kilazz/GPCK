// crates/gpck_core/src/packer/types.rs
//! # Core Data Types for Asset Packaging Pipeline
//!
//! Defines configuration structures, chunk definitions, 64KB hardware tile models,
//! PBR auto-clustering suffix rules, and neural texture compression (NTC) settings.

use crate::compression::codecs::CompressionMethod;
use crate::format::archive::TAG_BASE_GAME;
use uuid::Uuid;

/// Native 64 KB D3D12 / Vulkan Sparse Hardware Tile Chunk Size (65,536 bytes).
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;

/// Default NVMe Partition Boundary (64 MB).
pub const DEFAULT_MAX_PARTITION_SIZE: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct PbrSuffixConfig {
    pub albedo: Vec<String>,
    pub normal: Vec<String>,
    pub metallic: Vec<String>,
    pub roughness: Vec<String>,
    pub ao: Vec<String>,
    pub displacement: Vec<String>,
}

impl Default for PbrSuffixConfig {
    fn default() -> Self {
        Self {
            albedo: vec![
                "_diff".into(),
                "_albedo".into(),
                "_basecolor".into(),
                "_color".into(),
                "_col".into(),
                "_d".into(),
                "_alb".into(),
            ],
            normal: vec![
                "_ddn".into(),
                "_ddna".into(),
                "_normal".into(),
                "_norm".into(),
                "_nrm".into(),
                "_n".into(),
                "_nor".into(),
            ],
            metallic: vec![
                "_spec".into(),
                "_specular".into(),
                "_metal".into(),
                "_metallic".into(),
                "_metalness".into(),
                "_m".into(),
                "_met".into(),
            ],
            roughness: vec![
                "_gloss".into(),
                "_rough".into(),
                "_roughness".into(),
                "_r".into(),
                "_rgh".into(),
            ],
            ao: vec![
                "_ao".into(),
                "_ambient".into(),
                "_occlusion".into(),
                "_ambientocclusion".into(),
            ],
            displacement: vec![
                "_displ".into(),
                "_disp".into(),
                "_height".into(),
                "_h".into(),
                "_bump".into(),
            ],
        }
    }
}

#[derive(Clone, Debug)]
pub struct NtcPackerOptions {
    pub enabled: bool,
    pub target_bpp: f32,
    pub training_steps: i32,
    pub auto_bundle_pbr: bool,
    pub precompute_bc7_modes: bool,
    pub stable_training: bool,
    pub pbr_suffixes: PbrSuffixConfig,
}

impl Default for NtcPackerOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            target_bpp: 5.0,
            training_steps: 10000,
            auto_bundle_pbr: true,
            precompute_bc7_modes: true,
            stable_training: true,
            pbr_suffixes: PbrSuffixConfig::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GaclFormatOverrides {
    pub enabled: bool,
    pub auto_mode: bool,
    pub bc1_transform: Option<u32>,
    pub bc2_transform: Option<u32>,
    pub bc3_transform: Option<u32>,
    pub bc4_transform: Option<u32>,
    pub bc5_transform: Option<u32>,
    pub bc6h_transform: Option<u32>,
    pub bc7_transform: Option<u32>,
    pub rdo_reduction_pct: f32,
    pub rdo_use_ycocg: bool,
    pub rdo_bc1: bool,
    pub rdo_bc2: bool,
    pub rdo_bc3: bool,
    pub rdo_bc4: bool,
    pub rdo_bc5: bool,
    pub rdo_bc6h: bool,
    pub rdo_bc7: bool,
}

impl Default for GaclFormatOverrides {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_mode: true,
            bc1_transform: None,
            bc2_transform: None,
            bc3_transform: None,
            bc4_transform: None,
            bc5_transform: None,
            bc6h_transform: None,
            bc7_transform: None,
            rdo_reduction_pct: 0.0,
            rdo_use_ycocg: true,
            rdo_bc1: true,
            rdo_bc2: true,
            rdo_bc3: true,
            rdo_bc7: true,
            rdo_bc4: false,
            rdo_bc5: false,
            rdo_bc6h: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PackerOptions {
    pub method: CompressionMethod,
    pub level: i32,
    pub chunk_size: usize,
    pub enable_dedup: bool,
    pub key: Option<[u8; 32]>,
    pub mip_split: bool,
    pub max_tail_dim: usize,
    pub tags: u32,
    pub validate_chunks: bool,
    pub max_partition_size: usize,
    pub gacl: GaclFormatOverrides,
    pub ntc: NtcPackerOptions,
    pub atg_profile: bool,
    pub tiled_streaming: bool,
    pub min_tiled_resolution: usize,
    pub min_tiled_tile_count: u32,
}

impl Default for PackerOptions {
    fn default() -> Self {
        Self {
            method: CompressionMethod::GDeflate,
            level: 9,
            chunk_size: DEFAULT_CHUNK_SIZE,
            enable_dedup: true,
            key: None,
            mip_split: true,
            max_tail_dim: 128,
            tags: TAG_BASE_GAME,
            validate_chunks: true,
            max_partition_size: DEFAULT_MAX_PARTITION_SIZE,
            gacl: GaclFormatOverrides::default(),
            ntc: NtcPackerOptions::default(),
            atg_profile: true,
            tiled_streaming: true,
            min_tiled_resolution: 2048,
            min_tiled_tile_count: 8,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProcessedChunk {
    pub data: Vec<u8>,
    pub compressed_size: u32,
    pub original_size: u32,
    pub hash: u64,
    pub offset: i64,
}

pub struct ProcessedFile {
    pub asset_id: Uuid,
    pub original_path: String,
    pub original_size: u32,
    pub compressed_size: u32,
    pub flags: u32,
    pub tags: u32,
    pub partition_id: u32,
    pub alignment: i64,
    pub meta1: u32,
    pub meta2: u32,
    pub chunks: Vec<ProcessedChunk>,
    pub sub_chunk_offset: u32,
    pub sub_chunk_size: u32,
}

#[derive(Clone, Debug)]
pub struct PipTocEntry {
    pub id: Uuid,
    pub hash: u64,
    pub offset: i64,
    pub original_offset: i64,
    pub size: usize,
    pub is_pinned: bool,
}

#[derive(Clone, Debug)]
pub struct PipGap {
    pub begin: usize,
    pub end: usize,
}

impl PipGap {
    pub fn size(&self) -> usize {
        self.end.saturating_sub(self.begin)
    }
}
