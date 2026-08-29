// crates/gpck_core/src/compression/mod.rs
//! # Compression Subsystem
//!
//! Provides unified compression and decompression interfaces for Zstandard (ATG and standard profiles),
//! LZ4, native DirectStorage GDeflate, Interleaved 4-Way rANS, AMD GPUOpen Brotli-G,
//! and native DP4a Neural Texture Compression (GNTC / MiniDXNN).

pub mod brotlig;
pub mod codecs;
pub mod gdeflate;
pub mod rans;
pub mod zstd_atg;

#[cfg(feature = "neural-textures")]
pub mod ntc;
