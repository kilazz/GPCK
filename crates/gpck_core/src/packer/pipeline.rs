// crates/gpck_core/src/packer/pipeline.rs
//! # Staged Asset Packaging Pipeline & PBR Material Auto-Clustering

use super::chunker;
use super::emitter;
use super::ntc_packer::NtcBundlePacker;
use super::texture;
use super::types::{
    DEFAULT_MAX_PARTITION_SIZE, GaclFormatOverrides, NtcPackerOptions, PackerOptions, PipGap,
    PipTocEntry, ProcessedFile,
};
use crate::compression::codecs::CompressionMethod;
use crate::compression::ntc::NtcPbrMaterialBundle;
use crate::core::error::{GpckError, GpckResult};
use crate::format::archive::{FLAG_BOOT_TAIL, TAG_BASE_GAME};
use crossbeam_channel::bounded;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub struct PackingPipeline;

#[derive(Default, Debug)]
struct PbrMaterialSlot {
    base_rel_path: String,
    albedo_path: Option<PathBuf>,
    normal_path: Option<PathBuf>,
    metallic_path: Option<PathBuf>,
    roughness_path: Option<PathBuf>,
    ao_path: Option<PathBuf>,
    displacement_path: Option<PathBuf>,
}

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

        let mut pending_files: Vec<(PathBuf, String)> = file_map
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let mut all_processed_files: Vec<ProcessedFile> = Vec::new();

        // ====================================================================
        // PBR Material Auto-Clustering Pass (.gntc Bundling)
        // ====================================================================
        if options.ntc.enabled && options.ntc.auto_bundle_pbr {
            let (pbr_bundles, remaining) = Self::cluster_pbr_materials(&pending_files, options);
            pending_files = remaining;

            for bundle_slot in pbr_bundles {
                log_fn(&format!(
                    "[Neural PBR] Bundling standard PBR material: {}.gntc",
                    bundle_slot.base_rel_path
                ));

                if let Ok(processed_gntc) = Self::pack_clustered_material(&bundle_slot, options) {
                    all_processed_files.push(processed_gntc);
                }
            }
        }

        // ====================================================================
        // Classify Remaining Files (Standalone Textures, LUTs, Meshes)
        // ====================================================================
        let mut small_files = Vec::new();
        let mut large_files = Vec::new();
        let allow_grouping = options.method != CompressionMethod::Store;

        for (abs_p, rel_p) in pending_files {
            if let Ok(meta) = std::fs::metadata(&abs_p) {
                if allow_grouping && meta.len() < 64 * 1024 {
                    small_files.push((abs_p, rel_p));
                } else {
                    large_files.push((abs_p, rel_p));
                }
            }
        }

        // Process Large & Standalone Textures in Parallel
        let processed_standalone: Vec<ProcessedFile> = large_files
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

        all_processed_files.extend(processed_standalone);

        // Process Small File Groups into Super-Chunks
        if !small_files.is_empty() {
            let (small_tx, small_rx) = crossbeam_channel::unbounded();
            chunker::process_small_file_groups(&small_files, &small_tx, options)?;
            drop(small_tx);
            while let Ok(f) = small_rx.recv() {
                all_processed_files.push(f);
            }
        }

        // ====================================================================
        // Strict 3-Tier NVMe Streaming Layout Sorting
        // ====================================================================
        all_processed_files.sort_by(|a, b| {
            let is_tail_a = (a.flags & FLAG_BOOT_TAIL) != 0 || a.original_path.ends_with(".tail");
            let is_tail_b = (b.flags & FLAG_BOOT_TAIL) != 0 || b.original_path.ends_with(".tail");

            // Tier 1: Boot Tails MUST be placed first (Partition 0)
            if is_tail_a != is_tail_b {
                return is_tail_b.cmp(&is_tail_a);
            }

            let is_highmips_a = a.original_path.ends_with(".highmips");
            let is_highmips_b = b.original_path.ends_with(".highmips");

            // Tier 3: Highmips placed last (Soft streaming queue)
            if is_highmips_a != is_highmips_b {
                return is_highmips_a.cmp(&is_highmips_b);
            }

            // Tier 2: Deterministic alphanumeric path ordering
            a.original_path.cmp(&b.original_path)
        });

        // Stream Sorted Layout into GDAT Writer
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

        // Write Master Table of Contents (TOC)
        processed_files.sort_by(|a, b| a.original_path.cmp(&b.original_path));
        emitter::write_master_toc(&processed_files, &gtoc_path, options.key)
    }

    /// Clusters loose PBR maps using standard customizable suffix rules.
    fn cluster_pbr_materials(
        files: &[(PathBuf, String)],
        options: &PackerOptions,
    ) -> (Vec<PbrMaterialSlot>, Vec<(PathBuf, String)>) {
        let mut groups: HashMap<String, PbrMaterialSlot> = HashMap::new();
        let mut consumed_indices = std::collections::HashSet::new();

        let sfx = &options.ntc.pbr_suffixes;

        let mut rule_list: Vec<(&str, u8)> = Vec::new();
        for s in &sfx.albedo {
            rule_list.push((s.as_str(), 0));
        }
        for s in &sfx.normal {
            rule_list.push((s.as_str(), 1));
        }
        for s in &sfx.metallic {
            rule_list.push((s.as_str(), 2));
        }
        for s in &sfx.roughness {
            rule_list.push((s.as_str(), 3));
        }
        for s in &sfx.ao {
            rule_list.push((s.as_str(), 4));
        }
        for s in &sfx.displacement {
            rule_list.push((s.as_str(), 5));
        }

        // Sort suffix rules by length descending so longer matching suffixes take priority
        rule_list.sort_by_key(|b| std::cmp::Reverse(b.0.len()));

        for (idx, (abs_path, rel_path)) in files.iter().enumerate() {
            let lower = rel_path.to_lowercase();
            if !lower.ends_with(".dds") && !lower.ends_with(".png") && !lower.ends_with(".tga") {
                continue;
            }

            let stem = rel_path.rsplit('.').next_back().unwrap_or(rel_path);
            let lower_stem = stem.to_lowercase();

            for &(suffix, map_type) in &rule_list {
                let trimmed_suffix = suffix.trim();
                if !trimmed_suffix.is_empty() && lower_stem.ends_with(trimmed_suffix) {
                    let base_name = &stem[..stem.len() - trimmed_suffix.len()];
                    let entry =
                        groups
                            .entry(base_name.to_string())
                            .or_insert_with(|| PbrMaterialSlot {
                                base_rel_path: base_name.to_string(),
                                ..Default::default()
                            });

                    match map_type {
                        0 => entry.albedo_path = Some(abs_path.clone()),
                        1 => entry.normal_path = Some(abs_path.clone()),
                        2 => entry.metallic_path = Some(abs_path.clone()),
                        3 => entry.roughness_path = Some(abs_path.clone()),
                        4 => entry.ao_path = Some(abs_path.clone()),
                        5 => entry.displacement_path = Some(abs_path.clone()),
                        _ => {}
                    }

                    consumed_indices.insert(idx);
                    break;
                }
            }
        }

        let mut valid_bundles = Vec::new();
        let mut unclustered = Vec::new();

        for (base_name, slot) in groups {
            let mut channel_count = 0;
            if slot.albedo_path.is_some() {
                channel_count += 1;
            }
            if slot.normal_path.is_some() {
                channel_count += 1;
            }
            if slot.metallic_path.is_some() {
                channel_count += 1;
            }
            if slot.roughness_path.is_some() {
                channel_count += 1;
            }
            if slot.ao_path.is_some() {
                channel_count += 1;
            }

            if channel_count >= 2 {
                valid_bundles.push(slot);
            } else {
                for (idx, (p, r)) in files.iter().enumerate() {
                    if r.starts_with(&base_name) && consumed_indices.contains(&idx) {
                        unclustered.push((p.clone(), r.clone()));
                    }
                }
            }
        }

        for (idx, item) in files.iter().enumerate() {
            if !consumed_indices.contains(&idx) {
                unclustered.push(item.clone());
            }
        }

        (valid_bundles, unclustered)
    }

    /// Reads raw channels from clustered maps and encodes into a .gntc ProcessedFile
    fn pack_clustered_material(
        slot: &PbrMaterialSlot,
        options: &PackerOptions,
    ) -> GpckResult<ProcessedFile> {
        let width = 2048u32;
        let height = 2048u32;

        let albedo = slot
            .albedo_path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok());

        let normal = slot
            .normal_path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok());

        let metallic = slot
            .metallic_path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok());

        let roughness = slot
            .roughness_path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok());

        let ao = slot.ao_path.as_ref().and_then(|p| std::fs::read(p).ok());

        let mut bundle = NtcPbrMaterialBundle::new(width, height);
        bundle.albedo = albedo;
        bundle.normal = normal;
        bundle.metallic = metallic;
        bundle.roughness = roughness;
        bundle.ao = ao;

        let target_rel_path = format!("{}.gntc", slot.base_rel_path);
        NtcBundlePacker::pack_pbr_bundle(
            &bundle,
            &target_rel_path,
            options,
            Some(30),
            Some(options.ntc.target_bpp),
        )
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
            gacl: GaclFormatOverrides::default(),
            ntc: NtcPackerOptions::default(),
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
