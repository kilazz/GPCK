// src/ffi/mod.rs
//! # Foreign Function Interface (FFI) Subsystem
//!
//! Exposes thread-safe, reference-counted native C-ABI bindings (`c_api`) and
//! Android JNI entry points (`jni`) for integration with native game engine
//! runtimes and mobile platforms.

pub mod c_api;
pub mod jni;

pub use c_api::*;
pub use jni::*;
