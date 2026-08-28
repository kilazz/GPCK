// src/gpu/traits.rs
//! # Unified GPU Decompression & Streaming Backend Trait

use crate::compression::codecs::CompressionMethod;
use crate::core::error::GpckResult;
use crate::gacl::GaclTransform;

pub trait GpuStreamingBackend: Send + Sync {
    fn name(&self) -> &str;
    fn is_hardware_accelerated(&self) -> bool;
    fn decompress(
        &self,
        compressed: &[u8],
        target_size: usize,
        method: CompressionMethod,
    ) -> GpckResult<Vec<u8>>;

    fn decompress_and_unshuffle(
        &self,
        compressed: &[u8],
        target_size: usize,
        method: CompressionMethod,
        transform: GaclTransform,
        width_pixels: usize,
    ) -> GpckResult<Vec<u8>>;
}
