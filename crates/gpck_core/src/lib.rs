// crates/gpck_core/src/lib.rs
//! # GPCK Core Game Asset Packaging & VFS Engine
//!
//! Provides core runtime Virtual File System (VFS), DirectStorage 1.4 / Vulkan compute acceleration,
//! Game Asset Conditioning (GACL & RDO), crack-free Meshlet geometry quantization,
//! CHD minimal perfect hashing, and unified compression algorithms (GDeflate, Zstd, LZ4, rANS).

pub mod benchmark;
pub mod compression;
pub mod core;
pub mod ffi;
pub mod format;
pub mod gacl;
pub mod graphics;
pub mod io;
pub mod packer;

#[cfg(feature = "crypto")]
pub mod crypto;

#[cfg(feature = "geometry")]
pub mod geometry;

pub mod gpu;

// Re-exports for convenient access from CLI, GUI, and Godot crates
pub use crate::core::preset::PackerPreset;
pub use crate::core::settings::{self, AppSettings, CustomPresetConfig};
pub use crate::io::extract::{extract_asset_recombined, unshuffle_payload};

#[cfg(feature = "crypto")]
pub use crate::crypto::aes_gcm::derive_key;
