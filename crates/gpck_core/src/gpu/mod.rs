// crates/gpck_core/src/gpu/mod.rs
//! # GPU Acceleration Subsystem
//!
//! Provides hardware-accelerated asset decompression and streaming pipelines:
//! - DirectStorage 1.4 Native NVMe BypassIO direct-to-VRAM streaming (D3D12).
//! - Vulkan Compute Shader decompressors (GDeflate, Zstd ATG, Brotli-G, GACL).
//! - 64KB Sparse Tile Residency Pool Management & Sampler Feedback analysis.
//! - DRED 1.3 crash diagnostic telemetry and D3D12 InfoQueue callbacks.

pub mod debug_layer;
pub mod directstorage;
pub mod directstorage_sys;
pub mod dred;
pub mod sampler_feedback;
pub mod tile_pool;
pub mod traits;

#[cfg(feature = "gpu-vulkan")]
pub mod vulkan;

use crate::gpu::traits::GpuStreamingBackend;
use std::sync::{Arc, OnceLock};

/// Returns a lazily initialized, shared GPU streaming backend singleton.
///
/// Prevents device thrashing, resource contention, and driver TDRs during
/// concurrent multithreaded decompression requests.
pub fn create_default_gpu_backend() -> Option<Arc<dyn GpuStreamingBackend>> {
    static BACKEND: OnceLock<Option<Arc<dyn GpuStreamingBackend>>> = OnceLock::new();
    BACKEND
        .get_or_init(|| {
            #[cfg(all(target_os = "windows", feature = "gpu-directstorage"))]
            {
                if let Ok(ds) = directstorage::GpuDirectStorage::new()
                    && ds.is_supported()
                {
                    return Some(Arc::new(ds));
                }
            }

            #[cfg(feature = "gpu-vulkan")]
            {
                if let Ok(vk) = vulkan::VulkanDecompressor::new() {
                    return Some(Arc::new(vk));
                }
            }

            None
        })
        .clone()
}
