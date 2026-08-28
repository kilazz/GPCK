// crates/gpck_core/src/gpu/directstorage/mod.rs
//! # Microsoft DirectStorage 1.4 GPU Subsystem Facade
//!
//! Provides hardware-accelerated NVMe BypassIO streaming directly to VRAM (GPU buffers
//! and textures) without CPU staging overhead, using DirectStorage 1.4, D3D12 Agility SDK,
//! Enhanced Barriers, DRED 1.3 crash diagnostics, and native Microsoft ZstdGPU execution.

pub const GACL_TRANSFORM_NONE: u8 = 0;

/// DirectStorage queue priority levels matching engine streaming tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QueuePriority {
    Low = 0,
    Normal = 1,
    High = 2,
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
mod stub;
#[cfg(not(windows))]
pub use stub::*;
