// crates/gpck_core/src/packer/emitter.rs
//! # Pipeline Stage 5: GDAT Sequential Emission & CHD Minimal Perfect Hashing

use crate::core::error::{GpckError, GpckResult};
use crate::format::archive::{
    ArchiveHeader, BundleEntry, ChunkInfo, FileEntry, MAGIC_INT, hash_asset_id_with_seed,
};
use crate::packer::ProcessedFile;
use bytemuck::Zeroable;
use crossbeam_channel::Receiver;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use uuid::Uuid;

const MAX_DISPLACEMENT_ATTEMPTS: u32 = 2_000_000;
const SECONDARY_SEARCH_ATTEMPTS: u32 = 10_000_000;

pub fn spawn_gdat_writer(
    gdat_path: PathBuf,
    rx: Receiver<ProcessedFile>,
    enable_dedup: bool,
    max_partition_size: usize,
) -> JoinHandle<GpckResult<Vec<ProcessedFile>>> {
    std::thread::spawn(move || -> GpckResult<Vec<ProcessedFile>> {
        let mut gdat_file = File::create(&gdat_path).map_err(GpckError::Io)?;
        let mut current_data_ptr = 0i64;
        let mut current_partition_bytes = 0usize;
        let mut current_partition_id = 0u32;
        let mut chunk_offset_map: HashMap<u64, i64> = HashMap::new();
        let mut written_files = Vec::new();

        while let Ok(mut file) = rx.recv() {
            file.partition_id = current_partition_id;

            for chunk in &mut file.chunks {
                if enable_dedup && let Some(&existing_offset) = chunk_offset_map.get(&chunk.hash) {
                    chunk.offset = existing_offset;
                    chunk.data.clear();
                    continue;
                }

                if current_partition_bytes > 0
                    && current_partition_bytes + chunk.data.len() > max_partition_size
                {
                    current_partition_id += 1;
                    current_partition_bytes = 0;
                    file.partition_id = current_partition_id;
                }

                let aligned = (current_data_ptr + file.alignment - 1) & !(file.alignment - 1);
                let padding = (aligned - current_data_ptr) as usize;
                if padding > 0 {
                    gdat_file
                        .write_all(&vec![0u8; padding])
                        .map_err(GpckError::Io)?;
                }

                gdat_file.write_all(&chunk.data).map_err(GpckError::Io)?;
                chunk.offset = aligned;
                current_data_ptr = aligned + chunk.data.len() as i64;
                current_partition_bytes += padding + chunk.data.len();

                if enable_dedup {
                    chunk_offset_map.insert(chunk.hash, aligned);
                }

                chunk.data.clear();
            }
            written_files.push(file);
        }
        Ok(written_files)
    })
}

/// Industrial CHD (Compress, Hash and Displace) Minimal Perfect Hashing Algorithm.
/// Guarantees strict O(1) collision-free lookups for 100k - 2M+ assets without lost keys.
pub fn generate_chd_perfect_hash_table(files: &[ProcessedFile]) -> (Vec<FileEntry>, Vec<i32>) {
    let num_keys = files.len();
    if num_keys == 0 {
        return (Vec::new(), Vec::new());
    }

    // Minimal Perfect Hash: Capacity is exactly equal to the number of keys (Load Factor = 1.0)
    let capacity = num_keys;
    let lambda = 4usize;
    let num_buckets = (num_keys / lambda).max(1);

    let mut hash_table = vec![FileEntry::zeroed(); capacity];
    let mut displacements = vec![0i32; num_buckets];
    let mut buckets: Vec<Vec<&ProcessedFile>> = vec![Vec::new(); num_buckets];

    // 1. Distribute keys into buckets via primary hash h0
    for f in files {
        let hash = u64::from_le_bytes(f.asset_id.as_bytes()[0..8].try_into().unwrap());
        let bucket_idx = (hash % num_buckets as u64) as usize;
        buckets[bucket_idx].push(f);
    }

    // 2. Sort buckets by size descending (hardest/most colliding buckets placed first)
    let mut bucket_order: Vec<usize> = (0..num_buckets).collect();
    bucket_order.sort_by(|&a, &b| buckets[b].len().cmp(&buckets[a].len()));

    let mut occupied = vec![false; capacity];

    // Helper closure to write a file entry into the hash table
    let place_entry = |table: &mut [FileEntry], slot: usize, f: &ProcessedFile| {
        table[slot] = FileEntry {
            asset_id: *f.asset_id.as_bytes(),
            data_offset: f.chunks.first().map_or(0, |c| c.offset),
            chunk_table_offset: 0,
            name_offset: 0,
            compressed_size: f.compressed_size,
            original_size: f.original_size,
            flags: f.flags,
            meta1: f.meta1,
            meta2: f.meta2,
            tags: f.tags,
            partition_id: f.partition_id,
            chunk_count: f.chunks.len() as i32,
            sub_chunk_offset: f.sub_chunk_offset,
            sub_chunk_size: f.sub_chunk_size,
        };
    };

    // 3. Find displacement seed d for each bucket
    for b_idx in bucket_order {
        let bucket = &buckets[b_idx];
        if bucket.is_empty() {
            continue;
        }

        // Single-element bucket: map directly to the first available free slot via negative index
        if bucket.len() == 1 {
            let f = bucket[0];
            if let Some(free_slot) = occupied.iter().position(|&occ| !occ) {
                occupied[free_slot] = true;
                displacements[b_idx] = -((free_slot as i32) + 1);
                place_entry(&mut hash_table, free_slot, f);
            }
            continue;
        }

        // Multi-element bucket: search for collision-free positive displacement d >= 1
        let mut d = 1u32;
        let mut slots = Vec::with_capacity(bucket.len());
        let mut found = false;

        // Phase 1: Standard linear displacement search
        while d <= MAX_DISPLACEMENT_ATTEMPTS {
            slots.clear();
            let mut collision = false;

            for f in bucket {
                let slot =
                    (hash_asset_id_with_seed(d, f.asset_id.as_bytes()) % capacity as u64) as usize;
                if occupied[slot] || slots.contains(&slot) {
                    collision = true;
                    break;
                }
                slots.push(slot);
            }

            if !collision {
                displacements[b_idx] = d as i32;
                for (i, f) in bucket.iter().enumerate() {
                    let slot = slots[i];
                    occupied[slot] = true;
                    place_entry(&mut hash_table, slot, f);
                }
                found = true;
                break;
            }

            d += 1;
        }

        // Phase 2: Dynamic expansion with secondary prime hashing stride if Phase 1 was exhausted
        if !found {
            let mut prime_stride_d = MAX_DISPLACEMENT_ATTEMPTS + 1;
            while prime_stride_d <= SECONDARY_SEARCH_ATTEMPTS {
                slots.clear();
                let mut collision = false;

                for f in bucket {
                    let slot = (hash_asset_id_with_seed(prime_stride_d, f.asset_id.as_bytes())
                        % capacity as u64) as usize;
                    if occupied[slot] || slots.contains(&slot) {
                        collision = true;
                        break;
                    }
                    slots.push(slot);
                }

                if !collision {
                    displacements[b_idx] = prime_stride_d as i32;
                    for (i, f) in bucket.iter().enumerate() {
                        let slot = slots[i];
                        occupied[slot] = true;
                        place_entry(&mut hash_table, slot, f);
                    }
                    found = true;
                    break;
                }

                // Stride by a large prime to break clustering
                prime_stride_d = prime_stride_d.wrapping_add(10007);
            }
        }

        assert!(
            found,
            "[CHD Fatal Error] Failed to resolve collision-free displacement for bucket of size {}. Seed space exhausted.",
            bucket.len()
        );
    }

    (hash_table, displacements)
}

pub fn write_master_toc(
    files: &[ProcessedFile],
    gtoc_path: &Path,
    key: Option<[u8; 32]>,
) -> GpckResult<()> {
    let mut gtoc = File::create(gtoc_path).map_err(GpckError::Io)?;
    gtoc.seek(SeekFrom::Start(64)).map_err(GpckError::Io)?;

    let mut file_chunk_tables = HashMap::new();

    for f in files {
        let chunk_infos: Vec<ChunkInfo> = f
            .chunks
            .iter()
            .map(|c| ChunkInfo {
                offset: c.offset,
                compressed_size: c.compressed_size,
                original_size: c.original_size,
                hash: c.hash,
            })
            .collect();

        let mut table_bytes = bytemuck::cast_slice(&chunk_infos).to_vec();
        if let Some(k) = key {
            #[cfg(feature = "crypto")]
            {
                table_bytes = crate::crypto::aes_gcm::encrypt_chunk_table(&table_bytes, &k)?;
            }
            #[cfg(not(feature = "crypto"))]
            {
                let _ = k;
                return Err(GpckError::Crypto(
                    "Archive encryption requested, but 'crypto' feature is not enabled in this build"
                        .to_string(),
                ));
            }
        }
        file_chunk_tables.insert(f.asset_id, table_bytes);
    }

    let chunk_table_start = gtoc.stream_position().map_err(GpckError::Io)? as i64;
    let mut file_chunk_table_offsets = HashMap::new();
    for f in files {
        file_chunk_table_offsets.insert(
            f.asset_id,
            gtoc.stream_position().map_err(GpckError::Io)? as i64,
        );
        gtoc.write_all(&file_chunk_tables[&f.asset_id])
            .map_err(GpckError::Io)?;
    }
    let chunk_table_size =
        gtoc.stream_position().map_err(GpckError::Io)? as i64 - chunk_table_start;

    let name_table_start = gtoc.stream_position().map_err(GpckError::Io)? as i64;
    let mut file_name_offsets = HashMap::new();
    for f in files {
        file_name_offsets.insert(
            f.asset_id,
            gtoc.stream_position().map_err(GpckError::Io)? as i64,
        );
        gtoc.write_all(f.asset_id.as_bytes())
            .map_err(GpckError::Io)?;
        let name_bytes = f.original_path.as_bytes();
        gtoc.write_all(&(name_bytes.len() as u16).to_le_bytes())
            .map_err(GpckError::Io)?;
        gtoc.write_all(name_bytes).map_err(GpckError::Io)?;
    }
    let name_table_size = gtoc.stream_position().map_err(GpckError::Io)? as i64 - name_table_start;

    // Generate CHD Minimal Perfect Hash Table
    let (mut hash_table, displacements) = generate_chd_perfect_hash_table(files);

    for entry in &mut hash_table {
        if entry.asset_id != [0u8; 16] {
            let id = Uuid::from_bytes(entry.asset_id);
            entry.chunk_table_offset = file_chunk_table_offsets[&id];
            entry.name_offset = file_name_offsets[&id];
        }
    }

    // Align TOC to 64 bytes for direct SIMD/vmovdqa loading
    let current_pos = gtoc.stream_position().map_err(GpckError::Io)?;
    let aligned_pos = (current_pos + 63) & !63;
    let pad = (aligned_pos - current_pos) as usize;
    if pad > 0 {
        gtoc.write_all(&vec![0u8; pad]).map_err(GpckError::Io)?;
    }

    let hash_table_start = gtoc.stream_position().map_err(GpckError::Io)? as i64;
    gtoc.write_all(bytemuck::cast_slice(&hash_table))
        .map_err(GpckError::Io)?;
    let hash_table_size = gtoc.stream_position().map_err(GpckError::Io)? as i64 - hash_table_start;

    let seed_table_start = gtoc.stream_position().map_err(GpckError::Io)? as i64;
    gtoc.write_all(bytemuck::cast_slice(&displacements))
        .map_err(GpckError::Io)?;

    let master_toc_offset = gtoc.stream_position().map_err(GpckError::Io)? as i64;
    let bundle = BundleEntry {
        bundle_id: 0,
        toc_offset: hash_table_start,
        toc_size: hash_table_size,
        name_table_offset: name_table_start,
        name_table_size,
        chunk_table_offset: chunk_table_start,
        chunk_table_size,
        file_count: files.len() as i32,
        hash_table_capacity: hash_table.len() as i32,
        seed_table_offset: seed_table_start,
        seed_count: displacements.len() as i32,
        _pad0: 0,
    };
    gtoc.write_all(bytemuck::bytes_of(&bundle))
        .map_err(GpckError::Io)?;

    let total_uncompressed = files.iter().map(|f| f.original_size as i64).sum();
    let header = ArchiveHeader {
        magic: MAGIC_INT,
        version: 2,
        master_toc_offset,
        bundle_count: 1,
        _pad0: 0,
        total_uncompressed_size: total_uncompressed,
        padding_longs: [0; 4],
    };

    gtoc.seek(SeekFrom::Start(0)).map_err(GpckError::Io)?;
    gtoc.write_all(bytemuck::bytes_of(&header))
        .map_err(GpckError::Io)?;

    Ok(())
}
