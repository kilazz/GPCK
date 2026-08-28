// crates/gpck_core/src/packer/discovery.rs
//! # Pipeline Stage 1: Asset Discovery & Path Normalization

use crate::core::error::{GpckError, GpckResult};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct AssetDiscovery;

impl AssetDiscovery {
    /// Recursively scans files from a directory or single file path, producing a normalized virtual path map.
    pub fn build_file_map<P: AsRef<Path>>(input_path: P) -> GpckResult<HashMap<PathBuf, String>> {
        let mut map = HashMap::new();
        let path = input_path.as_ref();

        if path.is_file() {
            if let Some(file_name) = path.file_name() {
                map.insert(path.to_path_buf(), file_name.to_string_lossy().to_string());
            }
            return Ok(map);
        }

        let root = fs::canonicalize(path).map_err(GpckError::Io)?;
        for entry in walkdir::WalkDir::new(&root)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let abs_path = entry.path().to_path_buf();
                if let Ok(rel_path) = abs_path.strip_prefix(&root) {
                    let rel_str = rel_path.to_string_lossy().replace('\\', "/");
                    map.insert(abs_path, rel_str);
                }
            }
        }

        Ok(map)
    }
}
