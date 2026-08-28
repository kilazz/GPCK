// crates/gpck_core/src/compression/codecs.rs
//! # Unified Compression Codecs Facade & Trait Architecture
//!
//! Provides an extensible trait-based architecture (`CodecBackend`) and unified dispatcher
//! for Store, LZ4, Zstd (Adaptive ATG/Standard), rANS, DirectStorage GDeflate, and AMD Brotli-G.

use crate::compression::brotlig;
use crate::compression::gdeflate::{self, ERR_INCOMPRESSIBLE};
use crate::compression::rans::RansCodec;
use crate::compression::zstd_atg::ZstdAtgCompressor;
use crate::core::error::{GpckError, GpckResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[repr(u32)]
pub enum CompressionMethod {
    #[default]
    Auto = 0,
    Store = 1,
    GDeflate = 2,
    Zstd = 3,
    Lz4 = 4,
    Rans = 5,
    BrotliG = 6,
}

impl CompressionMethod {
    #[inline(always)]
    pub fn from_flags(flags: u32) -> Self {
        match (flags & 0x38) >> 3 {
            2 => CompressionMethod::GDeflate,
            3 => CompressionMethod::Zstd,
            4 => CompressionMethod::Lz4,
            5 => CompressionMethod::Rans,
            6 => CompressionMethod::BrotliG,
            _ => CompressionMethod::Store,
        }
    }

    #[inline(always)]
    pub fn to_flag_bits(self) -> u32 {
        match self {
            CompressionMethod::GDeflate => 2 << 3,
            CompressionMethod::Zstd => 3 << 3,
            CompressionMethod::Lz4 => 4 << 3,
            CompressionMethod::Rans => 5 << 3,
            CompressionMethod::BrotliG => 6 << 3,
            _ => 1 << 3,
        }
    }
}

/// Unified Trait for all compression and decompression backends.
pub trait CodecBackend: Send + Sync {
    fn method(&self) -> CompressionMethod;
    fn name(&self) -> &'static str;
    fn compress(&self, input: &[u8], level: i32, atg_profile: bool) -> GpckResult<Vec<u8>>;
    fn decompress(&self, input: &[u8], target_size: usize) -> GpckResult<Vec<u8>>;
}

// ============================================================================
// Concrete Backend Implementations
// ============================================================================

pub struct StoreBackend;
impl CodecBackend for StoreBackend {
    #[inline(always)]
    fn method(&self) -> CompressionMethod {
        CompressionMethod::Store
    }
    #[inline(always)]
    fn name(&self) -> &'static str {
        "Store"
    }

    fn compress(&self, input: &[u8], _level: i32, _atg_profile: bool) -> GpckResult<Vec<u8>> {
        Ok(input.to_vec())
    }

    fn decompress(&self, input: &[u8], _target_size: usize) -> GpckResult<Vec<u8>> {
        Ok(input.to_vec())
    }
}

pub struct Lz4Backend;
impl CodecBackend for Lz4Backend {
    #[inline(always)]
    fn method(&self) -> CompressionMethod {
        CompressionMethod::Lz4
    }
    #[inline(always)]
    fn name(&self) -> &'static str {
        "LZ4"
    }

    fn compress(&self, input: &[u8], _level: i32, _atg_profile: bool) -> GpckResult<Vec<u8>> {
        let compressed = lz4_flex::compress_prepend_size(input);
        if compressed.len() >= input.len() {
            Ok(input.to_vec())
        } else {
            Ok(compressed)
        }
    }

    fn decompress(&self, input: &[u8], _target_size: usize) -> GpckResult<Vec<u8>> {
        lz4_flex::decompress_size_prepended(input).map_err(|e| GpckError::DecompressionFailed {
            method: "LZ4",
            message: e.to_string(),
        })
    }
}

pub struct ZstdBackend;
impl CodecBackend for ZstdBackend {
    #[inline(always)]
    fn method(&self) -> CompressionMethod {
        CompressionMethod::Zstd
    }
    #[inline(always)]
    fn name(&self) -> &'static str {
        "Zstd"
    }

    fn compress(&self, input: &[u8], level: i32, atg_profile: bool) -> GpckResult<Vec<u8>> {
        let compress_res = if atg_profile {
            ZstdAtgCompressor::compress_atg(input, level)
        } else {
            ZstdAtgCompressor::compress_standard(input, level)
        };

        let compressed = compress_res.map_err(|e| GpckError::CompressionFailed {
            method: if atg_profile {
                "Zstd_ATG"
            } else {
                "Zstd_Standard"
            },
            message: e.to_string(),
        })?;

        if compressed.len() >= input.len() {
            Ok(input.to_vec())
        } else {
            Ok(compressed)
        }
    }

    fn decompress(&self, input: &[u8], target_size: usize) -> GpckResult<Vec<u8>> {
        ZstdAtgCompressor::decompress(input, target_size).map_err(|e| {
            GpckError::DecompressionFailed {
                method: "Zstd",
                message: e.to_string(),
            }
        })
    }
}

pub struct RansBackend;
impl CodecBackend for RansBackend {
    #[inline(always)]
    fn method(&self) -> CompressionMethod {
        CompressionMethod::Rans
    }
    #[inline(always)]
    fn name(&self) -> &'static str {
        "Interleaved_rANS"
    }

    fn compress(&self, input: &[u8], _level: i32, _atg_profile: bool) -> GpckResult<Vec<u8>> {
        let compressed = RansCodec::compress(input).map_err(|e| GpckError::CompressionFailed {
            method: "Interleaved_rANS",
            message: e.to_string(),
        })?;

        if compressed.len() >= input.len() {
            Ok(input.to_vec())
        } else {
            Ok(compressed)
        }
    }

    fn decompress(&self, input: &[u8], target_size: usize) -> GpckResult<Vec<u8>> {
        RansCodec::decompress(input, target_size).map_err(|e| GpckError::DecompressionFailed {
            method: "Interleaved_rANS",
            message: e.to_string(),
        })
    }
}

pub struct GDeflateBackend;
impl CodecBackend for GDeflateBackend {
    #[inline(always)]
    fn method(&self) -> CompressionMethod {
        CompressionMethod::GDeflate
    }
    #[inline(always)]
    fn name(&self) -> &'static str {
        "GDeflate"
    }

    fn compress(&self, input: &[u8], level: i32, _atg_profile: bool) -> GpckResult<Vec<u8>> {
        if gdeflate::is_gdeflate_available() {
            match gdeflate::compress(input, level) {
                Ok(compressed) => Ok(compressed),
                Err(e) if e.to_string().contains(ERR_INCOMPRESSIBLE) => Ok(input.to_vec()),
                Err(e) => Err(GpckError::CompressionFailed {
                    method: "GDeflate",
                    message: e.to_string(),
                }),
            }
        } else {
            Err(GpckError::CompressionFailed {
                method: "GDeflate",
                message: "Native GDeflate library is missing".to_string(),
            })
        }
    }

    fn decompress(&self, input: &[u8], target_size: usize) -> GpckResult<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        // If native CPU GDeflate library is compiled, use it directly
        if gdeflate::is_gdeflate_available() {
            return gdeflate::decompress(input, target_size);
        }

        // Otherwise fallback to GPU compute decompressor
        if let Some(gpu) = crate::gpu::create_default_gpu_backend() {
            return gpu.decompress(input, target_size, CompressionMethod::GDeflate);
        }

        Err(GpckError::DecompressionFailed {
            method: "GDeflate",
            message: "Cannot decompress GDeflate payload: native GDeflate or Vulkan GPU required"
                .to_string(),
        })
    }
}

pub struct BrotliGBackend;
impl CodecBackend for BrotliGBackend {
    #[inline(always)]
    fn method(&self) -> CompressionMethod {
        CompressionMethod::BrotliG
    }
    #[inline(always)]
    fn name(&self) -> &'static str {
        "BrotliG"
    }

    fn compress(&self, input: &[u8], level: i32, _atg_profile: bool) -> GpckResult<Vec<u8>> {
        if brotlig::is_brotlig_available() {
            brotlig::compress(input, level)
        } else {
            Err(GpckError::CompressionFailed {
                method: "BrotliG",
                message: "Native AMD Brotli-G SDK is not compiled or unavailable".to_string(),
            })
        }
    }

    fn decompress(&self, input: &[u8], target_size: usize) -> GpckResult<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        // If native CPU Brotli-G SDK is available, use it directly
        if brotlig::is_brotlig_available() {
            return brotlig::decompress(input, target_size);
        }

        // Otherwise fallback to GPU compute decompressor
        if let Some(gpu) = crate::gpu::create_default_gpu_backend() {
            return gpu.decompress(input, target_size, CompressionMethod::BrotliG);
        }

        Err(GpckError::DecompressionFailed {
            method: "BrotliG",
            message:
                "Cannot decompress Brotli-G payload: native Brotli-G SDK or Vulkan GPU required"
                    .to_string(),
        })
    }
}

// Static Singletons
static STORE_BACKEND: StoreBackend = StoreBackend;
static LZ4_BACKEND: Lz4Backend = Lz4Backend;
static ZSTD_BACKEND: ZstdBackend = ZstdBackend;
static RANS_BACKEND: RansBackend = RansBackend;
static GDEFLATE_BACKEND: GDeflateBackend = GDeflateBackend;
static BROTLIG_BACKEND: BrotliGBackend = BrotliGBackend;

pub struct Codec;

impl Codec {
    /// Resolves the corresponding backend implementation for the specified method.
    #[inline(always)]
    pub fn get_backend(method: CompressionMethod) -> &'static dyn CodecBackend {
        match method {
            CompressionMethod::Store => &STORE_BACKEND,
            CompressionMethod::Lz4 => &LZ4_BACKEND,
            CompressionMethod::Zstd | CompressionMethod::Auto => &ZSTD_BACKEND,
            CompressionMethod::Rans => &RANS_BACKEND,
            CompressionMethod::GDeflate => &GDEFLATE_BACKEND,
            CompressionMethod::BrotliG => &BROTLIG_BACKEND,
        }
    }

    /// Compresses an input buffer using the specified method and profile.
    #[inline(always)]
    pub fn compress(
        input: &[u8],
        method: CompressionMethod,
        level: i32,
        atg_profile: bool,
    ) -> GpckResult<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }
        Self::get_backend(method).compress(input, level, atg_profile)
    }

    /// Decompresses an input buffer into its original uncompressed size.
    #[inline(always)]
    pub fn decompress(
        input: &[u8],
        target_size: usize,
        method: CompressionMethod,
    ) -> GpckResult<Vec<u8>> {
        if target_size == 0 || input.is_empty() {
            return Ok(Vec::new());
        }
        if input.len() == target_size || method == CompressionMethod::Store {
            return Ok(input.to_vec());
        }
        Self::get_backend(method).decompress(input, target_size)
    }
}
