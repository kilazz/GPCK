// crates/gpck_core/src/packer/mod.rs
//! # Build-Time Asset Packaging Engine
//!
//! Modular asset packaging pipeline: asset discovery, texture conditioning (GACL & RDO),
//! geometry meshlet clustering, 64KB hardware sparse tile packaging, deduplicated chunking,
//! streaming layout sorting, GDAT emission, CHD Minimal Perfect Hashing, and neural texture bundling.

pub mod chunker;
pub mod discovery;
pub mod emitter;
pub mod geometry;
pub mod pipeline;
pub mod sorter;
pub mod texture;
pub mod tiler;
pub mod types;

#[cfg(feature = "neural-textures")]
pub mod ntc_packer;

// Public re-exports for the crate API
pub use discovery::AssetDiscovery;
pub use geometry::process_geometry_file;
#[cfg(feature = "neural-textures")]
pub use ntc_packer::NtcBundlePacker;
pub use pipeline::PackingPipeline;
pub use texture::{ProcessedFileParams, build_processed_file};
pub use tiler::{D3D12_TILE_SIZE, TileSliceResult, TiledTexturePacker};
pub use types::{
    DEFAULT_CHUNK_SIZE, DEFAULT_MAX_PARTITION_SIZE, GaclFormatOverrides, NtcPackerOptions,
    PackerOptions, PbrSuffixConfig, PipGap, PipTocEntry, ProcessedChunk, ProcessedFile,
};

use crate::core::error::GpckResult;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Unified Facade for the Asset Packaging Engine
pub struct AssetPacker;

impl AssetPacker {
    /// Recursively scans files from a directory or single file path, producing a normalized virtual path map.
    #[inline(always)]
    pub fn build_file_map<P: AsRef<Path>>(input_path: P) -> GpckResult<HashMap<PathBuf, String>> {
        discovery::AssetDiscovery::build_file_map(input_path)
    }

    /// Compresses and packages indexed files into a GPCK dual-file archive (.gtoc + .gdat).
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

    /// Builds an incremental delta patch against a reference base archive using Best-Fit Decreasing (BFD) layout.
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
