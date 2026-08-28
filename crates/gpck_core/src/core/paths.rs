// crates/gpck_core/src/core/paths.rs
//! # Centralized Path Resolver for Logs, Crashes, and Configuration
//!
//! Automatically resolves the workspace/project root directory (F:\Dev\Edit\GPCK\)
//! to prevent scattering log and config folders across individual crate subdirectories.

use std::fs;
use std::path::{Path, PathBuf};

pub struct GpckPaths;

impl GpckPaths {
    /// Returns the single centralized root data directory in the project/workspace root.
    pub fn get_root_dir() -> PathBuf {
        // 1. Environment variable override (highest priority)
        if let Ok(custom) = std::env::var("GPCK_DATA_DIR") {
            let p = PathBuf::from(custom);
            let _ = fs::create_dir_all(&p);
            return p;
        }

        // 2. Discover workspace root (F:\Dev\Edit\GPCK\)
        if let Some(ws_root) = Self::find_workspace_root() {
            let p = ws_root.join("GPCK_Data");
            let _ = fs::create_dir_all(&p);
            return p;
        }

        // 3. Fallback: Portable mode next to the executable
        if let Ok(exe_path) = std::env::current_exe()
            && let Some(exe_dir) = exe_path.parent()
        {
            let portable_dir = exe_dir.join("GPCK_Data");
            if fs::create_dir_all(&portable_dir).is_ok() {
                return portable_dir;
            }
        }

        let fallback = PathBuf::from("GPCK_Data");
        let _ = fs::create_dir_all(&fallback);
        fallback
    }

    /// Recursively scans upwards to locate the root workspace directory.
    fn find_workspace_root() -> Option<PathBuf> {
        // Start from current working directory
        if let Ok(cwd) = std::env::current_dir() {
            let mut curr = cwd.as_path();
            loop {
                let cargo_toml = curr.join("Cargo.toml");
                if cargo_toml.exists() && is_root_workspace_manifest(&cargo_toml) {
                    return Some(curr.to_path_buf());
                }
                if curr.join(".git").exists() || curr.join("addons").exists() {
                    return Some(curr.to_path_buf());
                }
                match curr.parent() {
                    Some(parent) => curr = parent,
                    None => break,
                }
            }
        }

        // Search upwards from target/debug or target/release executable path
        if let Ok(exe_path) = std::env::current_exe() {
            let mut curr = exe_path.as_path();
            while let Some(parent) = curr.parent() {
                let cargo_toml = parent.join("Cargo.toml");
                if cargo_toml.exists() && is_root_workspace_manifest(&cargo_toml) {
                    return Some(parent.to_path_buf());
                }
                if parent.join(".git").exists() || parent.join("addons").exists() {
                    return Some(parent.to_path_buf());
                }
                curr = parent;
            }
        }

        None
    }

    /// Returns the directory where log files are stored (`<workspace_root>/GPCK_Data/logs`).
    pub fn get_logs_dir() -> PathBuf {
        let p = Self::get_root_dir().join("logs");
        let _ = fs::create_dir_all(&p);
        p
    }

    /// Returns the directory where crash reports and minidumps are stored (`<workspace_root>/GPCK_Data/crashes`).
    pub fn get_crashes_dir() -> PathBuf {
        let p = Self::get_root_dir().join("crashes");
        let _ = fs::create_dir_all(&p);
        p
    }

    /// Returns the directory where settings and preset JSONs are stored (`<workspace_root>/GPCK_Data/config`).
    pub fn get_config_dir() -> PathBuf {
        let p = Self::get_root_dir().join("config");
        let _ = fs::create_dir_all(&p);
        p
    }
}

fn is_root_workspace_manifest(cargo_toml: &Path) -> bool {
    if let Ok(content) = fs::read_to_string(cargo_toml) {
        content.contains("[workspace]")
    } else {
        false
    }
}
