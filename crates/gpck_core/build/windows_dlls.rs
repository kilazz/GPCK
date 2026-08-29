// crates/gpck_core/build/windows_dlls.rs
//! # Windows DirectX 12 & DirectStorage NuGet Deployment

#[cfg(target_os = "windows")]
use super::dxc::{SdkEnvironment, resolve_external_path};
#[cfg(target_os = "windows")]
use std::fs;
#[cfg(target_os = "windows")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
pub fn copy_windows_dlls(env: &SdkEnvironment) {
    let nuget_search = match resolve_external_path(env, "external/nuget")
        .or_else(|| resolve_external_path(env, "nuget"))
    {
        Some(p) => p,
        None => return,
    };

    let d3d12_dir = find_nuget_package(&nuget_search, "microsoft.direct3d.d3d12");
    let ds_dir = find_nuget_package(&nuget_search, "microsoft.direct3d.directstorage");

    let mut files_to_copy = Vec::new();

    if let Some(d3d12) = d3d12_dir {
        files_to_copy.push((
            d3d12.join("build/native/bin/x64/D3D12Core.dll"),
            "D3D12/D3D12Core.dll",
        ));
        files_to_copy.push((
            d3d12.join("build/native/bin/x64/d3d12SDKLayers.dll"),
            "D3D12/d3d12SDKLayers.dll",
        ));
    }

    if let Some(ds) = ds_dir {
        files_to_copy.push((ds.join("native/bin/x64/dstorage.dll"), "dstorage.dll"));
        files_to_copy.push((
            ds.join("native/bin/x64/dstoragecore.dll"),
            "dstoragecore.dll",
        ));
    }

    let mut target_dirs = Vec::new();
    let mut curr = env.out_dir.as_path();
    while let Some(parent) = curr.parent() {
        if curr.file_name().and_then(|s| s.to_str()) == Some("build") {
            target_dirs.push(parent.to_path_buf());
            break;
        }
        curr = parent;
    }

    target_dirs.push(env.workspace_root.join("target/release"));
    target_dirs.push(env.workspace_root.join("target/debug"));

    for target_dir in target_dirs {
        for (src, dst_rel) in &files_to_copy {
            let dst = target_dir.join(dst_rel);
            if let Some(parent) = dst.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if src.exists() {
                let _ = fs::copy(src, &dst);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn find_nuget_package(base: &Path, prefix: &str) -> Option<PathBuf> {
    let prefix_lower = prefix.to_lowercase();
    if let Ok(entries) = fs::read_dir(base) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir()
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
                && name.to_lowercase().starts_with(&prefix_lower)
            {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn copy_windows_dlls(_env: &super::dxc::SdkEnvironment) {}
