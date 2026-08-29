// crates/gpck_core/src/format/chd.rs
//! # CHD (Compress, Hash, and Displace) Minimal Perfect Hashing
//!
//! Provides deterministic O(1) hash evaluation and collision-free slot resolution
//! for Table of Contents (TOC) lookups with a 1.0 load factor and multi-trial master seed support.

use crate::format::archive::{FLAG_DELETED, FileEntry};
use uuid::Uuid;

/// Hashes a 128-bit Asset ID with a 32-bit displacement seed.
#[inline(always)]
pub fn hash_asset_id_with_seed(seed: u32, id_bytes: &[u8; 16]) -> u64 {
    let mut payload = [0u8; 20];
    payload[..4].copy_from_slice(&seed.to_le_bytes());
    payload[4..].copy_from_slice(id_bytes);
    twox_hash::XxHash64::oneshot(0, &payload)
}

/// Computes the primary 64-bit bucket distribution hash incorporating the bundle's master seed.
#[inline(always)]
pub fn calculate_primary_hash_with_seed(id: &Uuid, master_seed: u32) -> u64 {
    if master_seed == 0 {
        u64::from_le_bytes(id.as_bytes()[0..8].try_into().unwrap())
    } else {
        let mut payload = [0u8; 20];
        payload[..4].copy_from_slice(&master_seed.to_le_bytes());
        payload[4..].copy_from_slice(id.as_bytes());
        twox_hash::XxHash64::oneshot(0, &payload)
    }
}

/// Computes the default primary hash (master seed = 0).
#[inline(always)]
pub fn calculate_primary_hash(id: &Uuid) -> u64 {
    calculate_primary_hash_with_seed(id, 0)
}

/// Resolves the destination slot in the hash table given displacement seed and capacity.
#[inline(always)]
pub fn resolve_chd_slot(seed: i32, id_bytes: &[u8; 16], capacity: usize) -> usize {
    if seed >= 0 {
        (hash_asset_id_with_seed(seed as u32, id_bytes) % capacity as u64) as usize
    } else {
        (-seed - 1) as usize
    }
}

pub struct ChdLookup;

impl ChdLookup {
    /// Queries an entry from a memory-mapped byte buffer using CHD displacement tables.
    pub fn query_entry_from_mmap(
        mmap: &[u8],
        id: Uuid,
        toc_offset: usize,
        capacity: usize,
        seed_table_offset: usize,
        seed_count: usize,
        master_seed: u32,
    ) -> Option<FileEntry> {
        if capacity == 0 || seed_count == 0 {
            return None;
        }

        let hash = calculate_primary_hash_with_seed(&id, master_seed);
        let id_bytes = id.as_bytes();
        let bucket_idx = (hash % seed_count as u64) as usize;
        let disp_offset = seed_table_offset + bucket_idx * 4;

        let disp_slice = mmap.get(disp_offset..disp_offset + 4)?;
        let disp = i32::from_le_bytes(disp_slice.try_into().ok()?);

        let slot = resolve_chd_slot(disp, id_bytes, capacity);
        let entry_size = std::mem::size_of::<FileEntry>();
        let entry_offset = toc_offset + slot * entry_size;

        let entry_slice = mmap.get(entry_offset..entry_offset + entry_size)?;
        let entry: FileEntry = bytemuck::pod_read_unaligned(entry_slice);

        if entry.asset_id == *id_bytes && (entry.flags & FLAG_DELETED) == 0 {
            Some(entry)
        } else {
            None
        }
    }
}
