// crates/gpck_core/src/compression/gdeflate.rs
//! # GDeflate C++ Native Wrapper
//!
//! Direct Rust interface for the statically-linked Microsoft DirectStorage GDeflate C++ library.
//! 100% DirectStorage GPU Metacommand format compliant.

use crate::core::error::{GpckError, GpckResult};

#[cfg(gdeflate_native)]
unsafe extern "C" {
    fn GDeflateCompressBound(in_size: usize) -> usize;
    fn GDeflateCompress(
        out: *mut u8,
        out_size: *mut usize,
        in_data: *const u8,
        in_size: usize,
        level: u32,
        flags: u32,
    ) -> bool;
    fn GDeflateDecompress(
        out: *mut u8,
        out_size: usize,
        in_data: *const u8,
        in_size: usize,
        num_workers: u32,
    ) -> bool;
}

/// Returns true if the native GDeflate C++ static library was compiled and linked.
pub fn is_gdeflate_available() -> bool {
    cfg!(gdeflate_native)
}

pub const ERR_INCOMPRESSIBLE: &str = "INCOMPRESSIBLE_DATA";

/// Compresses an input buffer using the native Microsoft GDeflate compressor.
pub fn compress(input: &[u8], level: i32) -> GpckResult<Vec<u8>> {
    #[cfg(gdeflate_native)]
    {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        unsafe {
            let bound = GDeflateCompressBound(input.len());
            let alloc_size = (bound + 65536 + 4096).max(65536 * 2);
            let mut output_buffer = vec![0u8; alloc_size];
            let mut output_size = output_buffer.len();

            let success = GDeflateCompress(
                output_buffer.as_mut_ptr(),
                &mut output_size,
                input.as_ptr(),
                input.len(),
                level.clamp(1, 9) as u32,
                0,
            );

            if !success || output_size >= input.len() {
                return Ok(input.to_vec());
            }

            output_buffer.truncate(output_size);
            Ok(output_buffer)
        }
    }
    #[cfg(not(gdeflate_native))]
    {
        let _ = (input, level);
        Err(GpckError::CompressionFailed {
            method: "GDeflate",
            message: "Native GDeflate C++ library is not compiled or unavailable".to_string(),
        })
    }
}

/// Decompresses a GDeflate-compressed payload into the expected uncompressed buffer size.
pub fn decompress(input: &[u8], target_size: usize) -> GpckResult<Vec<u8>> {
    #[cfg(gdeflate_native)]
    {
        if target_size == 0 || input.is_empty() {
            return Ok(Vec::new());
        }

        unsafe {
            // Allocate extra 64KB page padding to prevent tile-boundary heap overruns on small payloads (< 64KB)
            let alloc_size = (target_size + 65536 + 64).max(65536 + 64);
            let mut output_buffer = vec![0u8; alloc_size];

            // For single 64KB tile payloads or when already running within parallel Rayon workers,
            // enforce 1 worker to prevent thread pool exhaustion and race conditions in GDeflate C++ runtime.
            let num_workers = if target_size <= 65536 {
                1u32
            } else {
                rayon::current_num_threads().clamp(1, 4) as u32
            };

            let success = GDeflateDecompress(
                output_buffer.as_mut_ptr(),
                target_size,
                input.as_ptr(),
                input.len(),
                num_workers,
            );

            if !success {
                return Err(GpckError::DecompressionFailed {
                    method: "GDeflate",
                    message: "GDeflate native decompression failed".to_string(),
                });
            }

            output_buffer.truncate(target_size);
            Ok(output_buffer)
        }
    }
    #[cfg(not(gdeflate_native))]
    {
        let _ = (input, target_size);
        Err(GpckError::DecompressionFailed {
            method: "GDeflate",
            message: "Native GDeflate C++ library is not compiled or unavailable".to_string(),
        })
    }
}
