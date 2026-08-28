// crates/gpck_core/src/io/extract.rs
//! # Asset Extraction & Mip-Tail Recombination Pipeline

use crate::core::error::GpckResult;
use crate::format::archive::{FileEntry, GameArchive};
use crate::graphics::recombine::TextureRecombiner;
use std::fs;
use uuid::Uuid;

/// Extracts an asset to disk, automatically recombining split texture mips if present.
pub fn extract_asset_recombined(
    archive: &GameArchive,
    entry: &FileEntry,
    target_dir: &std::path::Path,
    raw_mode: bool,
) -> GpckResult<bool> {
    let rel_path = archive
        .get_path_for_asset(entry)
        .unwrap_or_else(|| Uuid::from_bytes(entry.asset_id).to_string());

    let out_file_path = target_dir.join(&rel_path);

    if raw_mode {
        if let Some(parent) = out_file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = archive.read_asset(entry)?;
        fs::write(&out_file_path, &data)?;
        return Ok(true);
    }

    if rel_path.to_lowercase().ends_with(".highmips") {
        return Ok(false);
    }

    if let Some(parent) = out_file_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let raw_base_data = archive.read_asset(entry)?;
    let highmips_rel_path = format!("{}.highmips", rel_path);
    let highmips_id = crate::core::asset_id::AssetIdGenerator::generate(&highmips_rel_path);

    let (highmips_bytes, high_transform) = if let Some(high_entry) =
        archive.try_get_entry(highmips_id)
        && let Ok(raw_high_data) = archive.read_asset(&high_entry)
    {
        (Some(raw_high_data), high_entry.gacl_transform())
    } else {
        (None, 0)
    };

    let full_data = TextureRecombiner::recombine_dds(
        &rel_path,
        &raw_base_data,
        highmips_bytes.as_deref(),
        entry,
        high_transform,
        true,
    )?;

    fs::write(&out_file_path, &full_data)?;
    Ok(true)
}

pub fn unshuffle_payload(
    path: &str,
    data: &[u8],
    gacl_transform: u32,
    width_pixels: usize,
    decondition_gacl: bool,
) -> Vec<u8> {
    TextureRecombiner::unshuffle_payload(path, data, gacl_transform, width_pixels, decondition_gacl)
}
