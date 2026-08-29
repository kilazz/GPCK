// crates/gpck_core/src/packer/sorter.rs
//! # Multi-Tier Spatial, Sector & Format Affinity Sorter

use crate::format::archive::FLAG_BOOT_TAIL;
use crate::packer::types::ProcessedFile;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Primary streaming tier priority
#[inline(always)]
fn get_tier_rank(path: &str, flags: u32) -> u8 {
    let lower = path.to_lowercase();
    if (flags & FLAG_BOOT_TAIL) != 0 || lower.ends_with(".tail") {
        0 // Tier 0: Boot Partition (Partition 0 Instant Spawn)
    } else if lower.ends_with(".json")
        || lower.ends_with(".xml")
        || lower.ends_with(".cfg")
        || lower.ends_with(".lua")
        || lower.ends_with(".chd")
    {
        1 // Tier 1: Critical configs and scripts
    } else if lower.ends_with(".highmips") {
        3 // Tier 3: Soft background detail streaming
    } else {
        2 // Tier 2: Standard Sector Gameplay Assets (Meshes & Core Textures)
    }
}

/// Texture & Material sub-type affinity (groups PBR channels contiguous in LBA)
#[inline(always)]
fn get_pbr_affinity_rank(path: &str) -> u8 {
    let lower = path.to_lowercase();
    if lower.contains("_albedo") || lower.contains("_diff") || lower.contains("_basecolor") {
        0 // 1. Base Color
    } else if lower.contains("_normal")
        || lower.contains("_norm")
        || lower.contains("_nrm")
        || lower.contains("_ddn")
    {
        1 // 2. Tangent Normal
    } else if lower.contains("_orm")
        || lower.contains("_rough")
        || lower.contains("_spec")
        || lower.contains("_metal")
        || lower.contains("_ao")
    {
        2 // 3. ORM / PBR Masks
    } else if lower.ends_with(".gmesh")
        || lower.ends_with(".gdmm")
        || lower.ends_with(".dgf")
        || lower.ends_with(".obj")
    {
        3 // 4. Geometry & Meshlets
    } else if lower.ends_with(".gntc") || lower.ends_with(".ntex") {
        4 // 5. Neural PBR Containers
    } else {
        5 // 6. Other data
    }
}

/// Extracts sector/directory hierarchy prefix for spatial clustering.
fn extract_sector_prefix(path: &str) -> String {
    let clean = path.replace('\\', "/");
    let p = Path::new(&clean);
    if let Some(parent) = p.parent() {
        parent.to_string_lossy().to_lowercase()
    } else {
        String::new()
    }
}

/// Slices and orders raw discovery map files for streaming.
pub fn sort_for_streaming(file_map: &HashMap<PathBuf, String>) -> Vec<(PathBuf, String)> {
    let mut files: Vec<(PathBuf, String)> = file_map
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    files.sort_by(|a, b| {
        let tier_a = get_tier_rank(&a.1, 0);
        let tier_b = get_tier_rank(&b.1, 0);
        if tier_a != tier_b {
            return tier_a.cmp(&tier_b);
        }

        let sector_a = extract_sector_prefix(&a.1);
        let sector_b = extract_sector_prefix(&b.1);
        if sector_a != sector_b {
            return sector_a.cmp(&sector_b);
        }

        let aff_a = get_pbr_affinity_rank(&a.1);
        let aff_b = get_pbr_affinity_rank(&b.1);
        if aff_a != aff_b {
            return aff_a.cmp(&aff_b);
        }

        a.1.cmp(&b.1)
    });

    files
}

/// Sorts fully processed files applying spatial locality and 3-tier streaming constraints.
pub fn sort_processed_files_for_streaming(files: &mut [ProcessedFile]) {
    files.sort_by(|a, b| {
        let tier_a = get_tier_rank(&a.original_path, a.flags);
        let tier_b = get_tier_rank(&b.original_path, b.flags);
        if tier_a != tier_b {
            return tier_a.cmp(&tier_b);
        }

        let sector_a = extract_sector_prefix(&a.original_path);
        let sector_b = extract_sector_prefix(&b.original_path);
        if sector_a != sector_b {
            return sector_a.cmp(&sector_b);
        }

        let aff_a = get_pbr_affinity_rank(&a.original_path);
        let aff_b = get_pbr_affinity_rank(&b.original_path);
        if aff_a != aff_b {
            return aff_a.cmp(&aff_b);
        }

        a.original_path.cmp(&b.original_path)
    });
}
