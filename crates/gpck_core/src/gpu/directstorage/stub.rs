// crates/gpck_core/src/gpu/directstorage/stub.rs
//! # Non-Windows DirectStorage Fallback Stubs
//!
//! Clean, zero-overhead stub implementations of the DirectStorage API for non-Windows platforms
//! (Linux, macOS, Android, WebAssembly).

use super::QueuePriority;
use crate::compression::codecs::CompressionMethod;
use crate::core::error::{GpckError, GpckResult};
use crate::gacl::GaclTransform;
use crate::gpu::traits::GpuStreamingBackend;
use std::path::Path;

/// Non-Windows dummy file handle stub.
pub struct DStorageFile;

impl DStorageFile {
    #[inline(always)]
    pub fn ptr(&self) -> *mut std::ffi::c_void {
        std::ptr::null_mut()
    }
}

/// Non-Windows dummy DirectStorage engine stub.
pub struct GpuDirectStorage;

impl GpuDirectStorage {
    pub fn new() -> GpckResult<Self> {
        Err(GpckError::DirectStorageUnsupported)
    }

    #[inline(always)]
    pub fn is_supported(&self) -> bool {
        false
    }

    pub fn open_file<P: AsRef<Path>>(&self, _path: P) -> GpckResult<DStorageFile> {
        Err(GpckError::DirectStorageUnsupported)
    }

    pub fn enqueue_buffer_request(&self, _priority: QueuePriority, _request: &()) {}

    pub fn enqueue_tile_request(&self, _priority: QueuePriority, _request: &()) {}

    pub fn cancel_requests_with_tag(&self, _priority: QueuePriority, _mask: u64, _value: u64) {}

    pub fn flush_and_signal(&self, _priority: QueuePriority) -> GpckResult<u64> {
        Err(GpckError::DirectStorageUnsupported)
    }

    pub fn wait_for_fence(&self, _priority: QueuePriority, _fence_val: u64) -> GpckResult<()> {
        Err(GpckError::DirectStorageUnsupported)
    }

    pub fn decompress_batch_gpu(
        &self,
        _compressed_data: &[u8],
        _uncompressed_size: usize,
    ) -> GpckResult<()> {
        Err(GpckError::DirectStorageUnsupported)
    }

    pub fn decompress_batch_gpu_zstd(
        &self,
        _compressed_data: &[u8],
        _uncompressed_size: usize,
        _gacl_transform: u8,
    ) -> GpckResult<()> {
        Err(GpckError::DirectStorageUnsupported)
    }

    pub fn decompress_batch_gpu_brotlig(
        &self,
        _compressed_data: &[u8],
        _uncompressed_size: usize,
    ) -> GpckResult<()> {
        Err(GpckError::DirectStorageUnsupported)
    }
}

impl GpuStreamingBackend for GpuDirectStorage {
    fn name(&self) -> &str {
        "DirectStorage Unsupported (Non-Windows)"
    }

    fn is_hardware_accelerated(&self) -> bool {
        false
    }

    fn decompress(
        &self,
        _compressed: &[u8],
        _target_size: usize,
        _method: CompressionMethod,
    ) -> GpckResult<Vec<u8>> {
        Err(GpckError::DirectStorageUnsupported)
    }

    fn decompress_and_unshuffle(
        &self,
        _compressed: &[u8],
        _target_size: usize,
        _method: CompressionMethod,
        _transform: GaclTransform,
        _width_pixels: usize,
    ) -> GpckResult<Vec<u8>> {
        Err(GpckError::DirectStorageUnsupported)
    }
}
