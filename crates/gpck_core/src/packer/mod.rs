// crates/gpck_core/src/packer/mod.rs
//! # Build-Time Asset Packaging Engine
//!
//! Modular asset packaging pipeline: discovery, texture conditioning,
//! geometry meshlet clustering, 64KB sparse tile packaging, deduplicated chunking,
//! streaming layout sorting, GDAT emission, and CHD Minimal Perfect Hashing.

pub mod chunker;
pub mod discovery;
pub mod emitter;
pub mod geometry;
pub mod pipeline;
pub mod sorter;
pub mod texture;
pub mod tiler;
pub mod types;

// Public re-exports for the crate API
pub use discovery::AssetDiscovery;
pub use geometry::process_geometry_file;
pub use pipeline::PackingPipeline;
pub use texture::{ProcessedFileParams, build_processed_file};
pub use tiler::{D3D12_TILE_SIZE, TileSliceResult, TiledTexturePacker};
pub use types::{
    DEFAULT_CHUNK_SIZE, DEFAULT_MAX_PARTITION_SIZE, GaclFormatOverrides, PackerOptions, PipGap,
    PipTocEntry, ProcessedChunk, ProcessedFile,
};

use crate::core::error::GpckResult;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Unified Facade for the Asset Packaging Engine
pub struct AssetPacker;

impl AssetPacker {
    #[inline(always)]
    pub fn build_file_map<P: AsRef<Path>>(input_path: P) -> GpckResult<HashMap<PathBuf, String>> {
        discovery::AssetDiscovery::build_file_map(input_path)
    }

    #[inline(always)]
    pub fn compress_files_to_archive<P: AsRef<Path>, F>(
        file_map: &HashMap<PathBuf, String>,
        output_path: P,
        options: &PackerOptions,
        log_fn: F,
    ) -> GpckResult<()>
    where
        F: Fn(&str) + Sync + Send + 'static,
    {
        pipeline::PackingPipeline::execute(file_map, output_path, options, log_fn)
    }

    #[inline(always)]
    pub fn build_delta_patch<P: AsRef<Path>>(
        base_archive_path: P,
        file_map: &HashMap<PathBuf, String>,
        output_path: P,
        level: i32,
        key: Option<[u8; 32]>,
        force_method: crate::compression::codecs::CompressionMethod,
    ) -> GpckResult<()> {
        pipeline::PackingPipeline::execute_delta_patch(
            base_archive_path,
            file_map,
            output_path,
            level,
            key,
            force_method,
        )
    }
}
