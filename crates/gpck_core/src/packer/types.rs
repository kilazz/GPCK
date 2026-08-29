// crates/gpck_core/src/packer/types.rs
//! # Core Data Types & Builders for Asset Packaging Pipeline
//!
//! Defines configuration structures, chunk definitions, 64KB hardware tile models,
//! texture metadata descriptors, and the fluent `ProcessedFileBuilder`.

use crate::compression::codecs::CompressionMethod;
use crate::core::asset_id::AssetIdGenerator;
use crate::format::archive::{
    FLAG_ENCRYPTED_META, FLAG_IS_COMPRESSED, FLAG_STREAMING, SHIFT_ALIGNMENT, SHIFT_GACL_TRANSFORM,
    TAG_BASE_GAME,
};
use crate::gacl::GaclTransform;
use crate::graphics::dxgi_format::D3D12FormatTable;
use uuid::Uuid;

/// Native 64 KB D3D12 / Vulkan Sparse Hardware Tile Chunk Size (65,536 bytes).
pub const DEFAULT_CHUNK_SIZE: usize = 64 * 1024;

/// Default NVMe Partition Boundary (64 MB).
pub const DEFAULT_MAX_PARTITION_SIZE: usize = 64 * 1024 * 1024;

// ============================================================================
// Texture Metadata & Conditioning Descriptors
// ============================================================================

/// Strongly-typed descriptor encapsulating container and subresource dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextureMetadata {
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
    pub dxgi_format: u32,
    pub header_length: usize,
}

impl TextureMetadata {
    #[inline(always)]
    pub fn new(
        width: u32,
        height: u32,
        mip_count: u32,
        dxgi_format: u32,
        header_length: usize,
    ) -> Self {
        Self {
            width,
            height,
            mip_count: mip_count.max(1),
            dxgi_format,
            header_length,
        }
    }

    /// Packs primary width and height into the 32-bit archive metadata field 1.
    #[inline(always)]
    pub fn meta1(&self) -> u32 {
        (self.width << 16) | (self.height & 0xFFFF)
    }

    /// Packs mip count, DXGI format tag, and tail/tile parameters into archive metadata field 2.
    #[inline(always)]
    pub fn meta2(&self, extra_param: u32) -> u32 {
        ((self.mip_count & 0xFF) << 24) | ((self.dxgi_format & 0xFF) << 16) | (extra_param & 0xFFFF)
    }

    #[inline(always)]
    pub fn is_block_compressed(&self) -> bool {
        D3D12FormatTable::is_block_compressed(self.dxgi_format)
    }

    #[inline(always)]
    pub fn element_size(&self) -> usize {
        D3D12FormatTable::get_element_size(self.dxgi_format).unwrap_or(16)
    }

    #[inline(always)]
    pub fn max_dimension(&self) -> u32 {
        self.width.max(self.height)
    }
}

/// Result of GACL texture conditioning and Rate-Distortion Optimization (RDO).
#[derive(Debug, Clone, Default)]
pub struct TextureConditioningResult {
    pub payload: Vec<u8>,
    pub transform: GaclTransform,
    pub space_curve_applied: bool,
}

// ============================================================================
// Processed File Model & Fluent Builder
// ============================================================================

#[derive(Clone, Debug)]
pub struct ProcessedChunk {
    pub data: Vec<u8>,
    pub compressed_size: u32,
    pub original_size: u32,
    pub hash: u64,
    pub offset: i64,
}

#[derive(Clone, Debug)]
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

/// Fluent builder for constructing `ProcessedFile` instances with automated flag derivation.
pub struct ProcessedFileBuilder {
    rel_path: String,
    original_size: u32,
    chunks: Vec<ProcessedChunk>,
    flags: u32,
    tags: u32,
    method: CompressionMethod,
    alignment: i64,
    meta1: u32,
    meta2: u32,
    partition_id: u32,
    sub_chunk_offset: u32,
    sub_chunk_size: u32,
    has_encryption_key: bool,
}

impl ProcessedFileBuilder {
    pub fn new(rel_path: impl Into<String>, original_size: u32, method: CompressionMethod) -> Self {
        Self {
            rel_path: rel_path.into(),
            original_size,
            chunks: Vec::new(),
            flags: FLAG_STREAMING,
            tags: TAG_BASE_GAME,
            method,
            alignment: 4096,
            meta1: 0,
            meta2: 0,
            partition_id: 0,
            sub_chunk_offset: 0,
            sub_chunk_size: 0,
            has_encryption_key: false,
        }
    }

    #[inline(always)]
    pub fn chunks(mut self, chunks: Vec<ProcessedChunk>) -> Self {
        self.chunks = chunks;
        self
    }

    #[inline(always)]
    pub fn flags(mut self, flags: u32) -> Self {
        self.flags |= flags;
        self
    }

    #[inline(always)]
    pub fn gacl_transform(mut self, transform: GaclTransform) -> Self {
        self.flags |= (transform.to_u32() & 0x3F) << SHIFT_GACL_TRANSFORM;
        self
    }

    #[inline(always)]
    pub fn metadata(mut self, meta1: u32, meta2: u32) -> Self {
        self.meta1 = meta1;
        self.meta2 = meta2;
        self
    }

    #[inline(always)]
    pub fn tags(mut self, tags: u32) -> Self {
        self.tags = tags;
        self
    }

    #[inline(always)]
    pub fn alignment(mut self, alignment: i64) -> Self {
        self.alignment = alignment;
        self
    }

    #[inline(always)]
    pub fn partition_id(mut self, partition_id: u32) -> Self {
        self.partition_id = partition_id;
        self
    }

    #[inline(always)]
    pub fn sub_chunk(mut self, offset: u32, size: u32) -> Self {
        self.sub_chunk_offset = offset;
        self.sub_chunk_size = size;
        self
    }

    #[inline(always)]
    pub fn encryption_key(mut self, key: Option<&[u8; 32]>) -> Self {
        self.has_encryption_key = key.is_some();
        self
    }

    /// Builds the final `ProcessedFile`, computing dynamic compression flags and alignment bits.
    pub fn build(self) -> ProcessedFile {
        let compressed_size: u32 = self.chunks.iter().map(|c| c.compressed_size).sum();
        let mut final_flags = self.flags;

        if self.method != CompressionMethod::Store && compressed_size < self.original_size {
            final_flags |= FLAG_IS_COMPRESSED;
        }
        final_flags |= self.method.to_flag_bits();

        if self.has_encryption_key {
            final_flags |= FLAG_ENCRYPTED_META;
        }

        let align_power = (self.alignment as f64).log2() as u32;
        final_flags |= align_power << SHIFT_ALIGNMENT;

        let asset_id = AssetIdGenerator::generate(&self.rel_path);

        ProcessedFile {
            asset_id,
            original_path: self.rel_path,
            original_size: self.original_size,
            compressed_size,
            flags: final_flags,
            tags: self.tags,
            partition_id: self.partition_id,
            alignment: self.alignment,
            meta1: self.meta1,
            meta2: self.meta2,
            chunks: self.chunks,
            sub_chunk_offset: self.sub_chunk_offset,
            sub_chunk_size: self.sub_chunk_size,
        }
    }
}

// ============================================================================
// Packaging Option Structures
// ============================================================================

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
                "_albedo".into(),
                "_basecolor".into(),
                "_diff".into(),
                "_color".into(),
                "_col".into(),
                "_d".into(),
                "_alb".into(),
            ],
            normal: vec![
                "_normal".into(),
                "_norm".into(),
                "_nrm".into(),
                "_n".into(),
                "_nor".into(),
            ],
            metallic: vec![
                "_metal".into(),
                "_metallic".into(),
                "_metalness".into(),
                "_m".into(),
                "_met".into(),
                "_specular".into(),
                "_spec".into(),
            ],
            roughness: vec![
                "_rough".into(),
                "_roughness".into(),
                "_rgh".into(),
                "_r".into(),
                "_gloss".into(),
            ],
            ao: vec![
                "_ao".into(),
                "_ambient".into(),
                "_occlusion".into(),
                "_ambientocclusion".into(),
            ],
            displacement: vec![
                "_disp".into(),
                "_displ".into(),
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
    pub grid_res_index: i32,
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
            target_bpp: 6.0,
            grid_res_index: 0,
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
