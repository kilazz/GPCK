// src/core/asset_id.rs
//! # Asset ID Generator
//!
//! Provides deterministic 128-bit UUID generation from virtual file paths
//! using XxHash3_128 to ensure unique, fast, and repeatable asset identification.

use uuid::Uuid;

/// Helper utility for producing deterministic UUIDs from virtual file paths.
pub struct AssetIdGenerator;

impl AssetIdGenerator {
    /// Generates a deterministic 128-bit Asset UUID from a virtual file path.
    ///
    /// Paths are automatically normalized to lower-case with forward slashes (`/`).
    pub fn generate(path: &str) -> Uuid {
        if path.is_empty() {
            return Uuid::nil();
        }

        // Path normalization: lower-case and standard forward slashes '/'
        let normalized = path.replace('\\', "/").to_lowercase();
        let bytes = normalized.as_bytes();

        // Generate full 128-bit entropy using XxHash3_128
        let hash128 = twox_hash::XxHash3_128::oneshot(bytes);
        let mut guid_bytes = hash128.to_le_bytes();

        // Format to standard UUID v4 byte structure
        guid_bytes[6] = (guid_bytes[6] & 0x0F) | (4 << 4);
        guid_bytes[8] = (guid_bytes[8] & 0x3F) | 0x80;

        Uuid::from_bytes(guid_bytes)
    }
}
