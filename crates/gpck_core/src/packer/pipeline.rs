// crates/gpck_core/src/packer/pipeline.rs
//! # Staged Asset Packaging Pipeline & 3-Tier NVMe Layout Emitter

use super::chunker;
use super::emitter;
use super::texture;
use super::types::{DEFAULT_MAX_PARTITION_SIZE, PackerOptions, PipGap, PipTocEntry, ProcessedFile};
use crate::compression::codecs::CompressionMethod;
use crate::core::error::{GpckError, GpckResult};
use crate::format::archive::{FLAG_BOOT_TAIL, TAG_BASE_GAME};
use crossbeam_channel::bounded;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub struct PackingPipeline;

impl PackingPipeline {
    pub fn execute<P: AsRef<Path>, F>(
        file_map: &HashMap<PathBuf, String>,
        output_path: P,
        options: &PackerOptions,
        log_fn: F,
    ) -> GpckResult<()>
    where
        F: Fn(&str) + Sync + Send + 'static,
    {
        let output_ref = output_path.as_ref();
        let gtoc_path = output_ref.with_extension("gtoc");
        let gdat_path = output_ref.with_extension("gdat");

        let mut small_files = Vec::new();
        let mut large_files = Vec::new();
        let allow_grouping = options.method != CompressionMethod::Store;

        for (abs_p, rel_p) in file_map {
            if let Ok(meta) = std::fs::metadata(abs_p) {
                if allow_grouping && meta.len() < 64 * 1024 {
                    small_files.push((abs_p.clone(), rel_p.clone()));
                } else {
                    large_files.push((abs_p.clone(), rel_p.clone()));
                }
            }
        }

        // 1. Process Large & Tiled Files in Parallel across Rayon Workers
        let mut all_processed_files: Vec<ProcessedFile> = large_files
            .par_iter()
            .map(|(abs_path, rel_path)| texture::process_file(abs_path, rel_path, options))
            .filter_map(|res| match res {
                Ok(files) => Some(files),
                Err(err) => {
                    log_fn(&format!("[Packing Warning] Skipped file: {}", err));
                    None
                }
            })
            .flatten()
            .collect();

        // 2. Process Small File Groups into Super-Chunks
        if !small_files.is_empty() {
            let (small_tx, small_rx) = crossbeam_channel::unbounded();
            chunker::process_small_file_groups(&small_files, &small_tx, options)?;
            drop(small_tx);
            while let Ok(f) = small_rx.recv() {
                all_processed_files.push(f);
            }
        }

        // ====================================================================
        // 3. Strict 3-Tier NVMe Streaming Layout Sorting
        // ====================================================================
        // Guarantees all Boot-Tail mips and startup metadata are physically placed
        // at Block 0..K (Partition 0 at the very start of .gdat) for sub-100ms startup.
        all_processed_files.sort_by(|a, b| {
            let is_tail_a = (a.flags & FLAG_BOOT_TAIL) != 0 || a.original_path.ends_with(".tail");
            let is_tail_b = (b.flags & FLAG_BOOT_TAIL) != 0 || b.original_path.ends_with(".tail");

            // Tier 1: Boot Tails MUST be placed first (Partition 0)
            if is_tail_a != is_tail_b {
                return is_tail_b.cmp(&is_tail_a); // true comes before false
            }

            let is_highmips_a = a.original_path.ends_with(".highmips");
            let is_highmips_b = b.original_path.ends_with(".highmips");

            // Tier 3: Highmips placed last (Soft streaming queue)
            if is_highmips_a != is_highmips_b {
                return is_highmips_a.cmp(&is_highmips_b); // false before true
            }

            // Tier 2: Deterministic alphanumeric path ordering
            a.original_path.cmp(&b.original_path)
        });

        // 4. Stream Sorted Layout into GDAT Writer
        let (tx, rx) = bounded::<ProcessedFile>(128);
        let writer_handle = emitter::spawn_gdat_writer(
            gdat_path,
            rx,
            options.enable_dedup,
            options.max_partition_size,
        );

        for file in all_processed_files {
            tx.send(file).map_err(|_| {
                GpckError::InvalidFormat("GDAT writer channel disconnected".to_string())
            })?;
        }
        drop(tx);

        let mut processed_files = writer_handle
            .join()
            .map_err(|_| GpckError::InvalidFormat("GDAT writer thread panicked".to_string()))??;

        // 5. Write Master Table of Contents (TOC)
        processed_files.sort_by(|a, b| a.original_path.cmp(&b.original_path));
        emitter::write_master_toc(&processed_files, &gtoc_path, options.key)
    }

    pub fn execute_delta_patch<P: AsRef<Path>>(
        base_archive_path: P,
        file_map: &HashMap<PathBuf, String>,
        output_path: P,
        level: i32,
        key: Option<[u8; 32]>,
        force_method: CompressionMethod,
    ) -> GpckResult<()> {
        let base_archive = crate::format::archive::GameArchive::open(base_archive_path)?;
        let base_entries = base_archive.get_all_entries()?;

        let mut ref_chunks: HashMap<u64, (i64, usize)> = HashMap::new();
        for entry in &base_entries {
            if let Ok(chunks) = base_archive.get_chunk_table(entry) {
                for chunk in chunks {
                    if chunk.offset >= 0 {
                        ref_chunks
                            .insert(chunk.hash, (chunk.offset, chunk.compressed_size as usize));
                    }
                }
            }
        }

        let output_ref = output_path.as_ref();
        let gtoc_path = output_ref.with_extension("gtoc");
        let gdat_path = output_ref.with_extension("gdat");

        let patch_options = PackerOptions {
            method: force_method,
            level,
            chunk_size: crate::packer::DEFAULT_CHUNK_SIZE,
            enable_dedup: false,
            key,
            mip_split: false,
            max_tail_dim: 128,
            tags: TAG_BASE_GAME,
            validate_chunks: true,
            max_partition_size: DEFAULT_MAX_PARTITION_SIZE,
            gacl: super::types::GaclFormatOverrides::default(),
            atg_profile: false,
            tiled_streaming: false,
            min_tiled_resolution: 0,
            min_tiled_tile_count: 0,
        };

        let (tx, rx) = bounded::<ProcessedFile>(128);

        let writer_handle = std::thread::spawn(move || -> GpckResult<Vec<ProcessedFile>> {
            let mut gdat_file = File::create(&gdat_path).map_err(GpckError::Io)?;
            let mut current_data_ptr = 0i64;
            let mut written_files = Vec::new();
            let mut pip_entries = Vec::new();

            while let Ok(mut file) = rx.recv() {
                for chunk in &mut file.chunks {
                    if let Some(&(ref_offset, ref_size)) = ref_chunks.get(&chunk.hash) {
                        chunk.offset = ref_offset;
                        pip_entries.push(PipTocEntry {
                            id: file.asset_id,
                            hash: chunk.hash,
                            offset: ref_offset,
                            original_offset: ref_offset,
                            size: ref_size,
                            is_pinned: true,
                        });
                        chunk.data.clear();
                    } else {
                        pip_entries.push(PipTocEntry {
                            id: file.asset_id,
                            hash: chunk.hash,
                            offset: -1,
                            original_offset: current_data_ptr,
                            size: chunk.data.len(),
                            is_pinned: false,
                        });
                        current_data_ptr += chunk.data.len() as i64;
                    }
                }
                written_files.push(file);
            }

            let (final_entries, _total_bytes_written) =
                Self::run_pip_bfd_layout(&mut pip_entries, 0.05);

            let mut entry_map: HashMap<u64, i64> = HashMap::new();
            for entry in &final_entries {
                entry_map.insert(entry.hash, entry.offset);
            }

            for file in &mut written_files {
                for chunk in &mut file.chunks {
                    if let Some(&final_off) = entry_map.get(&chunk.hash)
                        && chunk.offset == -1
                    {
                        chunk.offset = final_off;
                        if !chunk.data.is_empty() {
                            gdat_file
                                .seek(SeekFrom::Start(final_off as u64))
                                .map_err(GpckError::Io)?;
                            gdat_file.write_all(&chunk.data).map_err(GpckError::Io)?;
                            chunk.data.clear();
                        }
                    }
                }
            }

            Ok(written_files)
        });

        file_map
            .par_iter()
            .for_each_with(
                tx,
                |sender, (abs_path, rel_path)| match texture::process_file(
                    abs_path,
                    rel_path,
                    &patch_options,
                ) {
                    Ok(processed_files) => {
                        for processed in processed_files {
                            let _ = sender.send(processed);
                        }
                    }
                    Err(err) => {
                        eprintln!("[Patch Warning] Skipped file {:?}: {}", abs_path, err);
                    }
                },
            );

        let mut processed_files = writer_handle
            .join()
            .map_err(|_| GpckError::InvalidFormat("Writer thread panicked".to_string()))??;

        processed_files.sort_by(|a, b| a.original_path.cmp(&b.original_path));
        emitter::write_master_toc(&processed_files, &gtoc_path, patch_options.key)
    }

    fn run_pip_bfd_layout(
        entries: &mut [PipTocEntry],
        _max_growth_pct: f32,
    ) -> (Vec<PipTocEntry>, usize) {
        let mut pinned: Vec<PipTocEntry> =
            entries.iter().filter(|e| e.is_pinned).cloned().collect();
        let mut unpinned: Vec<PipTocEntry> =
            entries.iter().filter(|e| !e.is_pinned).cloned().collect();

        pinned.sort_by_key(|e| e.offset);
        unpinned.sort_by_key(|b| std::cmp::Reverse(b.size));

        let mut gaps = Vec::new();
        let mut prev_end = 0usize;

        for entry in &pinned {
            if entry.offset as usize > prev_end {
                gaps.push(PipGap {
                    begin: prev_end,
                    end: entry.offset as usize,
                });
            }
            prev_end = entry.offset as usize + entry.size;
        }

        gaps.push(PipGap {
            begin: prev_end,
            end: usize::MAX,
        });

        let mut placed_entries = pinned.clone();
        let mut bytes_written = 0usize;

        for mut item in unpinned {
            gaps.sort_by_key(|g| g.size());

            let mut placed = false;
            for gap in &mut gaps {
                if gap.size() >= item.size {
                    item.offset = gap.begin as i64;
                    gap.begin += item.size;
                    placed_entries.push(item.clone());
                    bytes_written += item.size;
                    placed = true;
                    break;
                }
            }

            if !placed && let Some(tail_gap) = gaps.last_mut() {
                item.offset = tail_gap.begin as i64;
                tail_gap.begin += item.size;
                placed_entries.push(item.clone());
                bytes_written += item.size;
            }
        }

        placed_entries.sort_by_key(|e| e.offset);
        (placed_entries, bytes_written)
    }
}
