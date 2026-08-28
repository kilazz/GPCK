// crates/gpck_core/src/compression/zstd_atg.rs
//! # ZSTD ATG-Optimized, Standard & Dictionary-Trained Compression
//!
//! Provides three compression pipelines:
//! 1. **ATG Profile (`compress_atg`):** Emulates the DirectStorage 1.4 `clevels_p.h` specification
//!    by enforcing `WindowLog = 18` (256 KB). This guarantees that decompression fits entirely
//!    within GPU L2/L3 cache and LDS/Shared Memory structures.
//! 2. **Standard Profile (`compress_standard`):** Uses Zstd's native sliding dictionary window
//!    (up to 128 MB at level 19+) for maximum compression ratio on non-streaming and monolithic assets.
//! 3. **Dictionary-Trained Profile (`compress_with_dict`):** Trains custom dictionaries on small file
//!    samples (< 4 KB, JSON, scripts, metadata) via `zstd::dict::from_samples` to boost compression ratios.

use crate::core::error::{GpckError, GpckResult};
use zstd::bulk::{Compressor, decompress as bulk_decompress};
use zstd::zstd_safe::CParameter;

/// Maximum Window Log mapped to 18 (2^18 bytes = 256 KB).
/// Matches the `clevels_p.h` specification for DirectStorage 1.4 GPU hardware limits.
pub const ZSTD_ATG_WINDOW_LOG_MAX: u32 = 18;

pub struct ZstdAtgCompressor;

impl ZstdAtgCompressor {
    /// Compresses data using standard Zstd bulk compression (unrestricted sliding window up to 128 MB).
    pub fn compress_standard(data: &[u8], compression_level: i32) -> GpckResult<Vec<u8>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        zstd::bulk::compress(data, compression_level).map_err(|e| GpckError::CompressionFailed {
            method: "Zstd_Standard",
            message: format!("ZSTD bulk compression failed: {}", e),
        })
    }

    /// Compresses data in-memory using the ATG-optimized 256 KB Window limit.
    pub fn compress_atg(data: &[u8], compression_level: i32) -> GpckResult<Vec<u8>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }

        let mut compressor =
            Compressor::new(compression_level).map_err(|e| GpckError::CompressionFailed {
                method: "Zstd_ATG",
                message: format!("Failed to create ZSTD bulk compressor: {}", e),
            })?;

        compressor
            .set_parameter(CParameter::WindowLog(ZSTD_ATG_WINDOW_LOG_MAX))
            .map_err(|e| GpckError::CompressionFailed {
                method: "Zstd_ATG",
                message: format!("Failed to set WindowLog(18) parameter: {}", e),
            })?;

        compressor
            .compress(data)
            .map_err(|e| GpckError::CompressionFailed {
                method: "Zstd_ATG",
                message: format!("ZSTD bulk compression failed: {}", e),
            })
    }

    /// Default compression helper (routes to `compress_atg` for streaming safety).
    pub fn compress(data: &[u8], compression_level: i32) -> GpckResult<Vec<u8>> {
        Self::compress_atg(data, compression_level)
    }

    /// Decompresses an ATG-optimized or standard ZSTD payload directly in memory.
    pub fn decompress(compressed_data: &[u8], expected_size: usize) -> GpckResult<Vec<u8>> {
        if compressed_data.is_empty() || expected_size == 0 {
            return Ok(Vec::new());
        }

        let output = bulk_decompress(compressed_data, expected_size).map_err(|e| {
            GpckError::DecompressionFailed {
                method: "Zstd",
                message: format!("ZSTD bulk decompression failed: {}", e),
            }
        })?;

        if output.len() != expected_size {
            return Err(GpckError::DecompressionFailed {
                method: "Zstd",
                message: format!(
                    "Decompression size mismatch. Expected {} bytes, got {} bytes",
                    expected_size,
                    output.len()
                ),
            });
        }

        Ok(output)
    }

    /// Trains a custom Zstandard dictionary from small file samples (< 4 KB).
    pub fn train_dictionary(samples: &[&[u8]], dict_capacity_bytes: usize) -> GpckResult<Vec<u8>> {
        if samples.is_empty() {
            return Ok(Vec::new());
        }

        zstd::dict::from_samples(samples, dict_capacity_bytes).map_err(|e| {
            GpckError::CompressionFailed {
                method: "Zstd_Dict_Training",
                message: format!("ZSTD dictionary training failed: {}", e),
            }
        })
    }

    /// Compresses a small payload using a pre-trained Zstandard dictionary.
    pub fn compress_with_dict(data: &[u8], dict: &[u8], level: i32) -> GpckResult<Vec<u8>> {
        if data.is_empty() {
            return Ok(Vec::new());
        }
        if dict.is_empty() {
            return Self::compress_atg(data, level);
        }

        let mut compressor = zstd::bulk::Compressor::with_dictionary(level, dict).map_err(|e| {
            GpckError::CompressionFailed {
                method: "Zstd_Dict",
                message: format!("Failed to create ZSTD dictionary compressor: {}", e),
            }
        })?;

        compressor
            .compress(data)
            .map_err(|e| GpckError::CompressionFailed {
                method: "Zstd_Dict",
                message: format!("ZSTD dictionary compression failed: {}", e),
            })
    }

    /// Decompresses a dictionary-compressed payload into the expected original size.
    pub fn decompress_with_dict(
        compressed: &[u8],
        dict: &[u8],
        expected_size: usize,
    ) -> GpckResult<Vec<u8>> {
        if compressed.is_empty() || expected_size == 0 {
            return Ok(Vec::new());
        }
        if dict.is_empty() {
            return Self::decompress(compressed, expected_size);
        }

        let mut decompressor = zstd::bulk::Decompressor::with_dictionary(dict).map_err(|e| {
            GpckError::DecompressionFailed {
                method: "Zstd_Dict",
                message: format!("Failed to create ZSTD dictionary decompressor: {}", e),
            }
        })?;

        decompressor
            .decompress(compressed, expected_size)
            .map_err(|e| GpckError::DecompressionFailed {
                method: "Zstd_Dict",
                message: format!("ZSTD dictionary decompression failed: {}", e),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atg_compression_roundtrip() {
        let raw_data = vec![0x42u8; 1024 * 1024];

        let compressed_atg = ZstdAtgCompressor::compress_atg(&raw_data, 15).unwrap();
        assert!(!compressed_atg.is_empty());

        let decompressed_atg =
            ZstdAtgCompressor::decompress(&compressed_atg, raw_data.len()).unwrap();
        assert_eq!(raw_data, decompressed_atg);

        let compressed_std = ZstdAtgCompressor::compress_standard(&raw_data, 19).unwrap();
        assert!(!compressed_std.is_empty());

        let decompressed_std =
            ZstdAtgCompressor::decompress(&compressed_std, raw_data.len()).unwrap();
        assert_eq!(raw_data, decompressed_std);
    }

    #[test]
    fn test_dictionary_training_and_compression() {
        // Zstandard dictionary trainer requires multiple representative samples (>= 8-16)
        let mut sample_buffers = Vec::new();
        for i in 0..24 {
            sample_buffers.push(format!(
                "{{\"entity_id\": {}, \"position\": [{:.1}, {:.1}, {:.1}], \"type\": \"Type_{}\", \"health\": {}, \"mana\": {}}}",
                100 + i,
                i as f32 * 1.5,
                i as f32 * 2.5,
                i as f32 * 3.5,
                i % 4,
                100 + i * 10,
                50 + i * 5
            ));
        }

        let samples: Vec<&[u8]> = sample_buffers.iter().map(|s| s.as_bytes()).collect();
        let dict = ZstdAtgCompressor::train_dictionary(&samples, 1024).unwrap();
        assert!(!dict.is_empty());

        let target = b"{\"entity_id\": 200, \"position\": [10.0, 11.0, 12.0], \"type\": \"Type_1\", \"health\": 500, \"mana\": 200}";
        let compressed = ZstdAtgCompressor::compress_with_dict(target, &dict, 9).unwrap();
        assert!(!compressed.is_empty());

        let decompressed =
            ZstdAtgCompressor::decompress_with_dict(&compressed, &dict, target.len()).unwrap();
        assert_eq!(decompressed, target);
    }
}
