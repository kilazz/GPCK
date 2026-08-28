// crates/gpck_core/src/core/error.rs
//! # GPCK Typed Error System
//!
//! Provides strongly-typed errors for engine integration and pattern-matching.

use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum GpckError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Archive TOC/Header format error: {0}")]
    InvalidFormat(String),

    #[error("Invalid magic signature: expected 0x4B435047 (GPCK), found 0x{0:08X}")]
    InvalidMagic(u32),

    #[error("Asset not found at virtual path: '{0}'")]
    AssetNotFound(String),

    #[error("Asset ID not found in VFS: {0}")]
    AssetIdNotFound(Uuid),

    #[error("Encrypted payload error: {0}")]
    Crypto(String),

    #[error("Decryption failed: invalid passphrase or tampered payload tag")]
    DecryptionFailed,

    #[error("Compression failed using '{method}': {message}")]
    CompressionFailed {
        method: &'static str,
        message: String,
    },

    #[error("Decompression failed using '{method}': {message}")]
    DecompressionFailed {
        method: &'static str,
        message: String,
    },

    #[error("Chunk validation failed for hash {hash:016X}: {message}")]
    ChunkValidationFailed { hash: u64, message: String },

    #[error("Geometry cluster processing error: {0}")]
    GeometryError(String),

    #[error("Shader file '{0}' not found in registry or disk")]
    ShaderNotFound(String),

    #[error("DirectX Bytecode (DXBC) parse error: {0}")]
    DxbcParseError(String),

    #[error("SPIR-V shader reflection error: {0}")]
    SpirvError(String),

    #[error("ASTC texture conditioning error: {0}")]
    AstcError(String),

    #[error("Direct I/O error: {0}")]
    DirectIoError(String),

    #[error("Direct I/O page-aligned buffer allocation failed for size: {0} bytes")]
    BufferAllocationFailed(usize),

    #[error("CDN network error for URL '{url}': {message}")]
    CdnNetworkError { url: String, message: String },

    #[error("DirectStorage GPU operation failed (HRESULT: 0x{hresult:08X}): {message}")]
    DirectStorageError { hresult: u32, message: &'static str },

    #[error("DirectStorage is unsupported on this OS/Hardware")]
    DirectStorageUnsupported,

    #[error("Vulkan compute error: {0}")]
    VulkanError(String),

    #[error("GACL transform error: {0}")]
    GaclError(String),

    #[error("Rate-Distortion Optimization (RDO) error: {0}")]
    RdoError(String),

    #[error("DDS parsing error: {0}")]
    DdsError(String),

    #[error("UTF-8 string decoding error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
}

pub type GpckResult<T> = Result<T, GpckError>;
