// crates/gpck_core/src/packer/emitter.rs
//! # Pipeline Stage 5: Two-Pass GDAT Emission & CHD Minimal Perfect Hashing
//!
//! Features:
//! - Pass 1: Global frequency and reference counting for deduplicated chunks.
//! - Pass 2: Partition 0 Global Shared Pool isolation (shared tiles & boot tails).
//! - Clean, sequential sector partitions (P1..N) with 100% contiguous LBA stream layout.

use crate::core::error::{GpckError, GpckResult};
use crate::format::archive::{
    ArchiveHeader, BundleEntry, ChunkInfo, FLAG_BOOT_TAIL, FileEntry, MAGIC_INT,
    calculate_primary_hash_with_seed, hash_asset_id_with_seed,
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

const MAX_PER_BUCKET_DISPLACEMENT_SEARCH: u32 = 50_000;
const MAX_MASTER_SEED_TRIALS: u32 = 1_000;

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

        // Pass 1: Collect incoming files and analyze chunk reference frequencies
        let mut incoming_files = Vec::new();
        let mut hash_frequency: HashMap<u64, usize> = HashMap::new();

        while let Ok(file) = rx.recv() {
            if enable_dedup {
                for chunk in &file.chunks {
                    *hash_frequency.entry(chunk.hash).or_insert(0) += 1;
                }
            }
            incoming_files.push(file);
        }

        // Pass 2 - Phase A: Emit Partition 0 (Global Shared Pool + Boot Tier)
        // All chunks referenced by >= 2 files or flagged as BOOT_TAIL are written first into P0.
        if enable_dedup {
            for file in &mut incoming_files {
                let is_boot_tier =
                    (file.flags & FLAG_BOOT_TAIL) != 0 || file.original_path.ends_with(".tail");

                for chunk in &mut file.chunks {
                    let is_shared = hash_frequency.get(&chunk.hash).copied().unwrap_or(0) > 1;

                    if (is_shared || is_boot_tier)
                        && !chunk_offset_map.contains_key(&chunk.hash)
                        && !chunk.data.is_empty()
                    {
                        let chunk_aligned = (current_data_ptr + 3) & !3;
                        let chunk_padding = (chunk_aligned - current_data_ptr) as usize;
                        if chunk_padding > 0 {
                            gdat_file
                                .write_all(&vec![0u8; chunk_padding])
                                .map_err(GpckError::Io)?;
                        }

                        gdat_file.write_all(&chunk.data).map_err(GpckError::Io)?;
                        chunk_offset_map.insert(chunk.hash, chunk_aligned);
                        current_data_ptr = chunk_aligned + chunk.data.len() as i64;
                        current_partition_bytes += chunk_padding + chunk.data.len();
                    }
                }
            }
        }

        // Advance to Partition 1 for sector-specific streaming assets
        if current_partition_bytes > 0 {
            current_partition_id = 1;
            current_partition_bytes = 0;
        }

        // Pass 2 - Phase B: Emit Sector Partitions (Partitions 1..N)
        // Unique sector chunks are written strictly sequential.
        let mut written_files = Vec::with_capacity(incoming_files.len());

        for mut file in incoming_files {
            let is_pure_boot_file =
                (file.flags & FLAG_BOOT_TAIL) != 0 || file.original_path.ends_with(".tail");
            file.partition_id = if is_pure_boot_file {
                0
            } else {
                current_partition_id
            };

            for chunk in &mut file.chunks {
                // If chunk is in Partition 0 (Shared Pool) or already written
                if let Some(&existing_offset) = chunk_offset_map.get(&chunk.hash) {
                    chunk.offset = existing_offset;
                    chunk.data.clear();
                    continue;
                }

                // Partition boundary check
                if current_partition_bytes > 0
                    && current_partition_bytes + chunk.data.len() > max_partition_size
                {
                    current_partition_id += 1;
                    current_partition_bytes = 0;
                    file.partition_id = current_partition_id;
                }

                let chunk_aligned = (current_data_ptr + 3) & !3;
                let chunk_padding = (chunk_aligned - current_data_ptr) as usize;
                if chunk_padding > 0 {
                    gdat_file
                        .write_all(&vec![0u8; chunk_padding])
                        .map_err(GpckError::Io)?;
                }

                gdat_file.write_all(&chunk.data).map_err(GpckError::Io)?;
                chunk.offset = chunk_aligned;
                current_data_ptr = chunk_aligned + chunk.data.len() as i64;
                current_partition_bytes += chunk_padding + chunk.data.len();

                chunk_offset_map.insert(chunk.hash, chunk_aligned);
                chunk.data.clear();
            }

            written_files.push(file);
        }

        Ok(written_files)
    })
}

fn try_generate_chd_table(
    files: &[ProcessedFile],
    master_seed: u32,
    max_displacement_attempts: u32,
) -> Option<(Vec<FileEntry>, Vec<i32>)> {
    let num_keys = files.len();
    let capacity = num_keys;
    let lambda = 4usize;
    let num_buckets = (num_keys / lambda).max(1);

    let mut hash_table = vec![FileEntry::zeroed(); capacity];
    let mut displacements = vec![0i32; num_buckets];
    let mut buckets: Vec<Vec<&ProcessedFile>> = vec![Vec::new(); num_buckets];

    for f in files {
        let hash = calculate_primary_hash_with_seed(&f.asset_id, master_seed);
        let bucket_idx = (hash % num_buckets as u64) as usize;
        buckets[bucket_idx].push(f);
    }

    let mut bucket_order: Vec<usize> = (0..num_buckets).collect();
    bucket_order.sort_by(|&a, &b| buckets[b].len().cmp(&buckets[a].len()));

    let mut occupied = vec![false; capacity];

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

    for b_idx in bucket_order {
        let bucket = &buckets[b_idx];
        if bucket.is_empty() {
            continue;
        }

        if bucket.len() == 1 {
            let f = bucket[0];
            if let Some(free_slot) = occupied.iter().position(|&occ| !occ) {
                occupied[free_slot] = true;
                displacements[b_idx] = -((free_slot as i32) + 1);
                place_entry(&mut hash_table, free_slot, f);
            }
            continue;
        }

        let mut d = 1u32;
        let mut slots = Vec::with_capacity(bucket.len());
        let mut found = false;

        while d <= max_displacement_attempts {
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

        if !found {
            return None;
        }
    }

    Some((hash_table, displacements))
}

pub fn generate_chd_perfect_hash_table(files: &[ProcessedFile]) -> (Vec<FileEntry>, Vec<i32>, u32) {
    let num_keys = files.len();
    if num_keys == 0 {
        return (Vec::new(), Vec::new(), 0);
    }

    for trial in 0..MAX_MASTER_SEED_TRIALS {
        let master_seed = trial;
        if let Some((hash_table, displacements)) =
            try_generate_chd_table(files, master_seed, MAX_PER_BUCKET_DISPLACEMENT_SEARCH)
        {
            return (hash_table, displacements, master_seed);
        }
    }

    let fallback_master_seed = 0x9E3779B9u32;
    if let Some((hash_table, displacements)) =
        try_generate_chd_table(files, fallback_master_seed, 500_000)
    {
        return (hash_table, displacements, fallback_master_seed);
    }

    panic!(
        "[CHD Fatal Error] Failed to resolve perfect hash table after {} master-seed restarts.",
        MAX_MASTER_SEED_TRIALS
    );
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

    let (mut hash_table, displacements, master_seed) = generate_chd_perfect_hash_table(files);

    for entry in &mut hash_table {
        if entry.asset_id != [0u8; 16] {
            let id = Uuid::from_bytes(entry.asset_id);
            entry.chunk_table_offset = file_chunk_table_offsets[&id];
            entry.name_offset = file_name_offsets[&id];
        }
    }

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
        master_seed,
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
