// crates/gpck_core/build/zstdgpu.rs
//! # Microsoft ATG ZstdGPU SRT Tool & C-Header Generator

use super::dxc::{SdkEnvironment, resolve_external_path};
use std::fs;
use std::path::Path;
use std::process::Command;

pub fn ensure_zstdgpu_generated_headers(env: &SdkEnvironment, zstdgpu_root: &Path) {
    let generated_dir = zstdgpu_root.join(".generated");
    let srt_header = generated_dir.join("zstdgpu_srt_structs.h");
    if srt_header.exists() {
        return;
    }

    let srt_tool_c = zstdgpu_root.join("zstdgpu_srt_tool.c");
    if !srt_tool_c.exists() {
        return;
    }

    let _ = fs::create_dir_all(&generated_dir);

    let exe_path = env.out_dir.join(if cfg!(windows) {
        "zstdgpu_srt_tool.exe"
    } else {
        "zstdgpu_srt_tool"
    });

    let build = cc::Build::new();
    let compiler = build.get_compiler();
    let mut cmd = Command::new(compiler.path());

    for (k, v) in compiler.env() {
        cmd.env(k, v);
    }

    cmd.arg(&srt_tool_c);
    cmd.arg(format!("-I{}", zstdgpu_root.display()));

    if let Some(tp) = resolve_external_path(env, "external/DirectStorage/zstd/ThirdParty") {
        cmd.arg(format!("-I{}", tp.display()));
    }
    if let Some(pf) = resolve_external_path(env, "external/DirectStorage/zstd/platform") {
        cmd.arg(format!("-I{}", pf.display()));
    }

    #[cfg(windows)]
    {
        cmd.arg(format!("/Fe:{}", exe_path.display()));
        cmd.arg(format!("/Fo:{}\\", env.out_dir.display()));
    }
    #[cfg(not(windows))]
    {
        cmd.arg("-o").arg(&exe_path);
    }

    if let Ok(out) = cmd.current_dir(zstdgpu_root).output()
        && !out.status.success()
    {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        panic!(
            "\n======================================================================\n\
             [Failed to compile zstdgpu_srt_tool.c]\n\
             Compiler: {}\n\
             Error:\n{}{}\n\
             ======================================================================\n",
            compiler.path().display(),
            stdout,
            stderr
        );
    }

    if exe_path.exists() {
        let run_res = Command::new(&exe_path).current_dir(zstdgpu_root).output();

        if let Ok(run_out) = run_res
            && !srt_header.exists()
            && !run_out.stdout.is_empty()
        {
            let _ = fs::write(&srt_header, &run_out.stdout);
        }
    }
}

pub fn sync_zstdgpu_headers_for_shaders(env: &SdkEnvironment, zstdgpu_root: &Path) {
    let shaders_dir = env.manifest_dir.join("shaders");
    if !shaders_dir.exists() {
        return;
    }

    if let Ok(entries) = fs::read_dir(zstdgpu_root) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("h") {
                let file_name = path.file_name().unwrap();
                let _ = fs::copy(&path, shaders_dir.join(file_name));
            }
        }
    }

    if let Some(tp) = resolve_external_path(env, "external/DirectStorage/zstd/ThirdParty")
        && let Ok(entries) = fs::read_dir(&tp)
    {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("h") {
                let file_name = path.file_name().unwrap();
                let _ = fs::copy(&path, shaders_dir.join(file_name));
            }
        }
    }

    let src_generated = zstdgpu_root.join(".generated");
    let dst_generated = shaders_dir.join(".generated");
    if src_generated.exists() {
        let _ = fs::create_dir_all(&dst_generated);
        if let Ok(entries) = fs::read_dir(&src_generated) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() {
                    let file_name = path.file_name().unwrap();
                    let _ = fs::copy(&path, dst_generated.join(file_name));
                    let _ = fs::copy(&path, shaders_dir.join(file_name));
                    let _ = fs::copy(&path, zstdgpu_root.join(file_name));
                }
            }
        }
    }
}

pub fn generate_d3d12_zstd_headers(env: &SdkEnvironment, out_headers_dir: &Path) {
    let zstdgpu_root = match resolve_external_path(env, "external/DirectStorage/zstd/zstdgpu") {
        Some(p) => p,
        None => return,
    };
    let zstd_external_shaders = zstdgpu_root.join("Shaders");

    let zstdgpu_shaders_inc = zstdgpu_root.join("Shaders");
    let thirdparty_dir = resolve_external_path(env, "external/DirectStorage/zstd/ThirdParty");
    let platform_dir = resolve_external_path(env, "external/DirectStorage/zstd/platform");
    let local_include = env.manifest_dir.join("src_cpp/include");

    if let Ok(entries) = fs::read_dir(&zstd_external_shaders) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("hlsl") {
                let file_stem = path.file_stem().unwrap().to_string_lossy();
                let dst_header = out_headers_dir.join(format!("{}.h", file_stem));
                let var_name = format!("g_{}", file_stem);

                let mut cmd = Command::new(&env.dxc_compiler);
                cmd.arg("-T")
                    .arg("cs_6_0")
                    .arg("-E")
                    .arg("main")
                    .arg("-HV")
                    .arg("2021")
                    .arg("-Fh")
                    .arg(&dst_header)
                    .arg("-Vn")
                    .arg(&var_name)
                    .arg("-I")
                    .arg(&zstd_external_shaders)
                    .arg("-I")
                    .arg(&zstdgpu_root)
                    .arg("-I")
                    .arg(&zstdgpu_shaders_inc);

                if let Some(ref tp) = thirdparty_dir {
                    cmd.arg("-I").arg(tp);
                }
                if let Some(ref pf) = platform_dir {
                    cmd.arg("-I").arg(pf);
                }

                cmd.arg("-I")
                    .arg(&local_include)
                    .arg("-I")
                    .arg(env.manifest_dir.join("src_cpp"))
                    .arg(&path);

                let out = cmd.output().unwrap_or_else(|e| {
                    panic!(
                        "Failed to execute DXC on D3D12 header '{}': {}",
                        path.display(),
                        e
                    );
                });

                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    panic!(
                        "\n======================================================================\n\
                         [D3D12 Shader Header Generation Failed]\n\
                         File: {}\n\
                         Compiler Output:\n{}\n\
                         ======================================================================\n",
                        path.display(),
                        stderr.trim()
                    );
                }
            }
        }
    }
}
