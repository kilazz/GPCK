// crates/gpck_core/build/dxc.rs
//! # Toolchain & SDK Discovery
//!
//! Locates and validates SPIR-V and DXIL capable DXC compiler instances across
//! environment variables, Vulkan SDK installations, Windows SDK paths, and system PATH.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct SdkEnvironment {
    pub dxc_compiler: PathBuf,
    pub manifest_dir: PathBuf,
    pub workspace_root: PathBuf,
    pub out_dir: PathBuf,
}

impl SdkEnvironment {
    pub fn discover() -> Self {
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let workspace_root = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| manifest_dir.clone());

        let dxc_compiler = Self::find_dxc().expect(
            "\n[GPCK Build Error] SPIR-V and DXIL capable DXC compiler was not found!\n\
             Please install the Vulkan SDK (https://vulkan.lunarg.com/) or set the VULKAN_SDK / GPCK_DXC_PATH environment variable.\n",
        );

        Self {
            dxc_compiler,
            manifest_dir,
            workspace_root,
            out_dir,
        }
    }

    fn find_dxc() -> Option<PathBuf> {
        // Explicit environment variable override (highest priority)
        if let Ok(path) = env::var("GPCK_DXC_PATH") {
            let p = PathBuf::from(path);
            if p.exists() && Self::verify_dxc_spirv(&p) {
                return Some(p);
            }
        }

        // Vulkan SDK root directory variable
        if let Ok(vk_sdk) = env::var("VULKAN_SDK") {
            let win_dxc = PathBuf::from(&vk_sdk).join("Bin/dxc.exe");
            let unix_dxc = PathBuf::from(&vk_sdk).join("bin/dxc");
            if win_dxc.exists() && Self::verify_dxc_spirv(&win_dxc) {
                return Some(win_dxc);
            }
            if unix_dxc.exists() && Self::verify_dxc_spirv(&unix_dxc) {
                return Some(unix_dxc);
            }
        }

        // Windows standard Vulkan SDK installation directory scanning
        #[cfg(target_os = "windows")]
        {
            let base_vk = PathBuf::from("C:\\VulkanSDK");
            if base_vk.exists()
                && let Ok(entries) = fs::read_dir(&base_vk)
            {
                let mut sdk_versions: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect();
                sdk_versions.sort();
                for sdk_dir in sdk_versions.iter().rev() {
                    let candidate = sdk_dir.join("Bin/dxc.exe");
                    if candidate.exists() && Self::verify_dxc_spirv(&candidate) {
                        return Some(candidate);
                    }
                }
            }
        }

        // System PATH search
        if let Some(path) = find_in_path(if cfg!(windows) { "dxc.exe" } else { "dxc" })
            && Self::verify_dxc_spirv(&path)
        {
            return Some(path);
        }

        // Standard Unix installation locations
        #[cfg(unix)]
        {
            for sys_dir in [
                "/usr/bin/dxc",
                "/usr/local/bin/dxc",
                "/opt/vulkansdk/bin/dxc",
            ] {
                let p = PathBuf::from(sys_dir);
                if p.exists() && Self::verify_dxc_spirv(&p) {
                    return Some(p);
                }
            }
        }

        None
    }

    fn verify_dxc_spirv(dxc: &Path) -> bool {
        if let Ok(out) = Command::new(dxc).arg("-help").output() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            stdout.contains("-spirv") || stderr.contains("-spirv")
        } else {
            false
        }
    }
}

pub fn resolve_external_path(env: &SdkEnvironment, subpath: &str) -> Option<PathBuf> {
    let local = env.manifest_dir.join(subpath);
    if local.exists() {
        return Some(local);
    }
    let workspace = env.workspace_root.join(subpath);
    if workspace.exists() {
        return Some(workspace);
    }
    None
}

pub fn find_in_path(exe: &str) -> Option<PathBuf> {
    if let Some(paths) = env::var_os("PATH") {
        for mut p in env::split_paths(&paths) {
            p.push(exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

pub fn collect_files_recursive(dir: &Path, extension: &str, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                collect_files_recursive(&path, extension, files);
            } else if path.extension().and_then(|s| s.to_str()) == Some(extension) {
                files.push(path);
            }
        }
    }
}
