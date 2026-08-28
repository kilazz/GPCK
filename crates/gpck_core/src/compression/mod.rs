// crates/gpck_core/src/compression/mod.rs
//! # Compression Subsystem
//!
//! Provides unified compression and decompression interfaces for Zstd (ATG profile),
//! LZ4, native DirectStorage GDeflate, Interleaved 4-Way rANS, and AMD GPUOpen Brotli-G algorithms.

pub mod brotlig;
pub mod codecs;
pub mod gdeflate;
pub mod rans;
pub mod zstd_atg;
