// crates/gpck_core/src/compression/brotlig.rs
//! # AMD GPUOpen Brotli-G Native & GPU Codec Interface

use crate::core::error::{GpckError, GpckResult};

#[cfg(brotlig_native)]
unsafe extern "C" {
    fn BrotliGCompressBound(in_size: usize) -> usize;
    fn BrotliGGetDecompressedSize(in_data: *const u8) -> u32;
    fn BrotliGCompress(
        out: *mut u8,
        out_size: *mut usize,
        in_data: *const u8,
        in_size: usize,
        page_size: u32,
        level: u32,
    ) -> bool;
    fn BrotliGDecompress(out: *mut u8, out_size: usize, in_data: *const u8, in_size: usize)
    -> bool;
}

pub fn is_brotlig_available() -> bool {
    cfg!(brotlig_native)
}

pub const BROTLIG_DEFAULT_PAGE_SIZE: u32 = 65536;

pub fn get_decompressed_size(input: &[u8]) -> Option<usize> {
    #[cfg(brotlig_native)]
    {
        if input.len() < 8 {
            return None;
        }
        let size = unsafe { BrotliGGetDecompressedSize(input.as_ptr()) };
        if size > 0 { Some(size as usize) } else { None }
    }
    #[cfg(not(brotlig_native))]
    {
        let _ = input;
        None
    }
}

pub fn compress(input: &[u8], level: i32) -> GpckResult<Vec<u8>> {
    #[cfg(brotlig_native)]
    {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        unsafe {
            let bound = BrotliGCompressBound(input.len());
            // Allocate with safe headroom to prevent buffer overflow
            let alloc_size = (bound + 131072).max(131072);
            let mut output_buffer = vec![0u8; alloc_size];
            let mut output_size = output_buffer.len();

            let success = BrotliGCompress(
                output_buffer.as_mut_ptr(),
                &mut output_size,
                input.as_ptr(),
                input.len(),
                BROTLIG_DEFAULT_PAGE_SIZE,
                level.clamp(1, 11) as u32,
            );

            if !success || output_size == 0 {
                return Err(GpckError::CompressionFailed {
                    method: "BrotliG",
                    message: "Brotli-G native compression failed".to_string(),
                });
            }

            output_buffer.truncate(output_size);
            Ok(output_buffer)
        }
    }
    #[cfg(not(brotlig_native))]
    {
        let _ = (input, level);
        Err(GpckError::CompressionFailed {
            method: "BrotliG",
            message: "Native AMD Brotli-G SDK is not compiled or unavailable".to_string(),
        })
    }
}

pub fn decompress(input: &[u8], target_size: usize) -> GpckResult<Vec<u8>> {
    #[cfg(brotlig_native)]
    {
        if target_size == 0 || input.is_empty() {
            return Ok(Vec::new());
        }

        unsafe {
            let alloc_size = (target_size + 65536 + 64).max(65536 + 64);
            let mut output_buffer = vec![0u8; alloc_size];
            let success = BrotliGDecompress(
                output_buffer.as_mut_ptr(),
                target_size,
                input.as_ptr(),
                input.len(),
            );

            if !success {
                return Err(GpckError::DecompressionFailed {
                    method: "BrotliG",
                    message: "Brotli-G native decompression failed".to_string(),
                });
            }

            output_buffer.truncate(target_size);
            Ok(output_buffer)
        }
    }
    #[cfg(not(brotlig_native))]
    {
        let _ = (input, target_size);
        Err(GpckError::DecompressionFailed {
            method: "BrotliG",
            message: "Native AMD Brotli-G SDK is not compiled or unavailable".to_string(),
        })
    }
}
