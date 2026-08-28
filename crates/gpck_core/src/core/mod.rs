// crates/gpck_core/src/core/mod.rs
//! # Core Utility Module
//!
//! Contains foundational primitives for deterministic asset ID generation,
//! thread-safe logging, cross-platform panic/exception handling, and unified path management.

pub mod asset_id;
pub mod crash_handler;
pub mod error;
pub mod logger;
pub mod paths;
pub mod preset;
pub mod settings;

pub use paths::GpckPaths;
