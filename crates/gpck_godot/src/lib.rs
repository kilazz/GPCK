// crates/gpck_godot/src/lib.rs
//! # GPCK Godot 4 GDExtension Entry Point & Native Library Lifecycle
//!
//! Registers the native ResourceFormatLoader and automatically discovers and mounts
//! VFS archive packages in both editor sessions and standalone release exports.

#![allow(clippy::result_large_err)]

use godot::classes::ResourceLoader;
use godot::init::{ExtensionLibrary, InitLevel};
use godot::prelude::*;

mod archive;
mod loader;
mod vfs;

pub use archive::GpckArchive;
pub use loader::GpckResourceFormatLoader;
pub use vfs::{GpckVfs, get_global_vfs};

struct GpckGodotExtension;

#[gdextension]
unsafe impl ExtensionLibrary for GpckGodotExtension {
    fn on_level_init(level: InitLevel) {
        if level == InitLevel::Scene {
            // Register the native ResourceFormatLoader with Godot's core loader chain
            let loader = GpckResourceFormatLoader::new_gd();
            ResourceLoader::singleton().add_resource_format_loader(&loader);

            // Discover and auto-mount default archive packages (.gtoc)
            let mut candidate_paths = Vec::new();

            // Check next to the executable (Standalone / Exported release builds)
            if let Ok(exe_path) = std::env::current_exe()
                && let Some(exe_dir) = exe_path.parent()
            {
                candidate_paths.push(exe_dir.join("game_data.gtoc"));
                candidate_paths.push(exe_dir.join("main.gtoc"));
            }

            // Check the current working directory / project root (Editor / Debug runs)
            candidate_paths.push(std::path::PathBuf::from("game_data.gtoc"));
            candidate_paths.push(std::path::PathBuf::from("main.gtoc"));
            candidate_paths.push(std::path::PathBuf::from("res://game_data.gtoc"));
            candidate_paths.push(std::path::PathBuf::from("res://main.gtoc"));

            let vfs = get_global_vfs();
            for path in candidate_paths {
                let path_str = path.to_string_lossy().to_string();
                let clean_path = path_str.trim_start_matches("res://");

                if std::path::Path::new(clean_path).exists()
                    && let Ok(mut guard) = vfs.write()
                    && guard.mount_archive(clean_path).is_ok()
                {
                    godot_print!("[GPCK] Auto-mounted default package: {}", clean_path);
                    break;
                }
            }

            godot_print!("[GPCK] Native VFS & ResourceFormatLoader initialized successfully.");
        }
    }

    fn on_level_deinit(_level: InitLevel) {
        // ResourceLoader automatically cleans up registered loaders on engine shutdown
    }
}
