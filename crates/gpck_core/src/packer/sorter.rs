// crates/gpck_core/src/packer/sorter.rs
//! # Multi-Tier Streaming & Format Affinity Sorter

use std::collections::HashMap;
use std::path::PathBuf;

pub fn sort_for_streaming(file_map: &HashMap<PathBuf, String>) -> Vec<(PathBuf, String)> {
    let mut files: Vec<(PathBuf, String)> = file_map
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    files.sort_by(|a, b| {
        let is_tail_a = a.1.ends_with(".tail");
        let is_tail_b = b.1.ends_with(".tail");

        // Tier 1: Boot Tails first (Partition 0)
        if is_tail_a != is_tail_b {
            return is_tail_b.cmp(&is_tail_a);
        }

        let is_highmips_a = a.1.ends_with(".highmips");
        let is_highmips_b = b.1.ends_with(".highmips");

        // Tier 3: Highmips last (Background streaming)
        if is_highmips_a != is_highmips_b {
            return is_highmips_a.cmp(&is_highmips_b);
        }

        // Tier 2: Compression affinity (group identical formats together)
        let ext_a = a.1.split('.').next_back().unwrap_or("");
        let ext_b = b.1.split('.').next_back().unwrap_or("");
        if ext_a != ext_b {
            return ext_a.cmp(ext_b);
        }

        // Tier 4: Spatial / Directory locality
        a.1.cmp(&b.1)
    });

    files
}
