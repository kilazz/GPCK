// crates/gpck_core/src/format/toc_view.rs
//! # Zero-Copy Master Table of Contents View

use crate::core::error::{GpckError, GpckResult};
use crate::format::archive::{ArchiveHeader, BundleEntry, FLAG_DELETED, FileEntry};
use crate::format::chd::{ChdLookup, calculate_primary_hash_with_seed};
use uuid::Uuid;

pub struct MasterTocView<'a> {
    pub header: &'a ArchiveHeader,
    pub bundles: &'a [BundleEntry],
    raw_mmap: &'a [u8],
}

impl<'a> MasterTocView<'a> {
    pub fn parse(raw_mmap: &'a [u8]) -> GpckResult<Self> {
        let header_size = std::mem::size_of::<ArchiveHeader>();
        if raw_mmap.len() < header_size {
            return Err(GpckError::InvalidFormat(
                "TOC buffer is smaller than ArchiveHeader size".to_string(),
            ));
        }

        let header: &'a ArchiveHeader = bytemuck::from_bytes(&raw_mmap[0..header_size]);
        let bundle_size = std::mem::size_of::<BundleEntry>();
        let bundles_start = header.master_toc_offset as usize;
        let bundles_end = bundles_start + (header.bundle_count as usize * bundle_size);

        if bundles_end > raw_mmap.len() {
            return Err(GpckError::InvalidFormat(
                "Bundle table offset exceeds TOC buffer bounds".to_string(),
            ));
        }

        let bundles: &'a [BundleEntry] =
            bytemuck::cast_slice(&raw_mmap[bundles_start..bundles_end]);

        Ok(Self {
            header,
            bundles,
            raw_mmap,
        })
    }

    pub fn find_entry(&self, id: Uuid) -> Option<FileEntry> {
        let entry_size = std::mem::size_of::<FileEntry>();

        for bundle in self.bundles.iter().rev() {
            let capacity = bundle.hash_table_capacity as usize;
            let seed_count = bundle.seed_count as usize;

            if capacity == 0 {
                continue;
            }

            // Unified O(1) Minimal Perfect Hashing with Master Seed
            if seed_count > 0
                && bundle.seed_table_offset > 0
                && let Some(entry) = ChdLookup::query_entry_from_mmap(
                    self.raw_mmap,
                    id,
                    bundle.toc_offset as usize,
                    capacity,
                    bundle.seed_table_offset as usize,
                    seed_count,
                    bundle.master_seed,
                )
            {
                return Some(entry);
            }

            // Open-Addressing fallback
            let hash = calculate_primary_hash_with_seed(&id, bundle.master_seed);
            let index = (hash % capacity as u64) as usize;
            for i in 0..capacity {
                let probe = (index + i) % capacity;
                let offset = bundle.toc_offset as usize + probe * entry_size;

                if offset + entry_size > self.raw_mmap.len() {
                    break;
                }

                let entry: FileEntry =
                    bytemuck::pod_read_unaligned(&self.raw_mmap[offset..offset + entry_size]);

                if entry.asset_id == [0u8; 16] {
                    break;
                }

                if entry.asset_id == *id.as_bytes() {
                    if (entry.flags & FLAG_DELETED) != 0 {
                        return None;
                    }
                    return Some(entry);
                }
            }
        }
        None
    }
}
