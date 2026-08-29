// crates/gpck_core/build/shaders.rs
//! # HLSL to SPIR-V & DXIL Shader Compiler
//!
//! Compiles compute shaders into Vulkan SPIR-V and DirectX 12 DXIL (SM 6.6)
//! bytecode and auto-generates embedded runtime lookup registries.

use super::dxc::{SdkEnvironment, collect_files_recursive, resolve_external_path};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::process::Command;

pub fn detect_entry_point(path: &Path) -> &'static str {
    if let Ok(content) = fs::read_to_string(path)
        && (content.contains("void CSMain(") || content.contains("void CSMain ("))
    {
        return "CSMain";
    }
    "main"
}

pub fn compile_spirv_and_generate_registry(env: &SdkEnvironment, target_shaders_dir: &Path) {
    let shaders_dir = env.manifest_dir.join("shaders");
    let mut compiled_shaders = Vec::new();

    let zstdgpu_root = resolve_external_path(env, "external/DirectStorage/zstd/zstdgpu");
    let zstdgpu_shaders_inc =
        resolve_external_path(env, "external/DirectStorage/zstd/zstdgpu/Shaders");
    let thirdparty_dir = resolve_external_path(env, "external/DirectStorage/zstd/ThirdParty");
    let platform_dir = resolve_external_path(env, "external/DirectStorage/zstd/platform");
    let local_include = env.manifest_dir.join("src_cpp/include");
    let local_zstd_shaders = env.manifest_dir.join("shaders/ZSTD");
    let local_brotli_shaders = env.manifest_dir.join("shaders/BrotliG");
    let local_gacl_shaders = env.manifest_dir.join("shaders/GACL");
    let local_gdeflate_shaders = env.manifest_dir.join("shaders/GDeflate");
    let local_geometry_shaders = env.manifest_dir.join("shaders/Geometry");
    let local_ntc_shaders = env.manifest_dir.join("shaders/NTC");

    if shaders_dir.exists() {
        let mut hlsl_files = Vec::new();
        collect_files_recursive(&shaders_dir, "hlsl", &mut hlsl_files);

        for path in &hlsl_files {
            let file_stem = path.file_stem().unwrap().to_string_lossy();
            let spv_name = format!("{}.spv", file_stem);
            let dst_spv = target_shaders_dir.join(&spv_name);
            let parent_dir = path.parent().unwrap_or(&shaders_dir);
            let entry_point = detect_entry_point(path);

            let mut cmd = Command::new(&env.dxc_compiler);
            cmd.arg("-T")
                .arg("cs_6_0")
                .arg("-E")
                .arg(entry_point)
                .arg("-O3")
                .arg("-spirv")
                .arg("-fspv-target-env=vulkan1.2")
                .arg("-fspv-use-vulkan-memory-model")
                .arg("-Vd")
                .arg("-fvk-t-shift")
                .arg("0")
                .arg("0")
                .arg("-fvk-u-shift")
                .arg("1")
                .arg("0")
                .arg("-fvk-b-shift")
                .arg("2")
                .arg("0")
                .arg("-fvk-s-shift")
                .arg("3")
                .arg("0")
                .arg("-HV")
                .arg("2021")
                .arg("-D__SPIRV__=1")
                .arg("-D__spirv__=1")
                .arg("-Wno-ignored-attributes")
                .arg("-I")
                .arg(&shaders_dir)
                .arg("-I")
                .arg(parent_dir)
                .arg("-I")
                .arg(&local_include)
                .arg("-I")
                .arg(&local_zstd_shaders)
                .arg("-I")
                .arg(&local_brotli_shaders)
                .arg("-I")
                .arg(&local_gacl_shaders)
                .arg("-I")
                .arg(&local_gdeflate_shaders)
                .arg("-I")
                .arg(&local_geometry_shaders)
                .arg("-I")
                .arg(&local_ntc_shaders);

            if let Some(ref zroot) = zstdgpu_root {
                cmd.arg("-I").arg(zroot);
                cmd.arg("-I").arg(zroot.join("Shaders"));
            }
            if let Some(ref zsh) = zstdgpu_shaders_inc {
                cmd.arg("-I").arg(zsh);
            }
            if let Some(ref tp) = thirdparty_dir {
                cmd.arg("-I").arg(tp);
            }
            if let Some(ref pf) = platform_dir {
                cmd.arg("-I").arg(pf);
            }

            cmd.arg("-Fo").arg(&dst_spv).arg(path);

            let out = cmd.output().unwrap_or_else(|e| {
                panic!(
                    "\n[DXC Spawn Error] Failed to execute DXC compiler on '{}': {}\n",
                    path.display(),
                    e
                );
            });

            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                panic!(
                    "\n======================================================================\n\
                     [HLSL -> SPIR-V COMPILATION FAILED]\n\
                     File:        {}\n\
                     Entry Point: {}\n\
                     Compiler:    {}\n\
                     ----------------------------------------------------------------------\n\
                     COMPILER OUTPUT:\n{}{}\n\
                     ======================================================================\n",
                    path.display(),
                    entry_point,
                    env.dxc_compiler.display(),
                    stdout.trim(),
                    stderr.trim()
                );
            }

            compiled_shaders.push((spv_name, dst_spv));
        }
    }

    let registry_file = env.out_dir.join("embedded_shaders.rs");
    let mut f = File::create(&registry_file).expect("Failed to create embedded_shaders.rs");

    writeln!(
        f,
        "// Auto-generated embedded SPIR-V shader lookup registry"
    )
    .unwrap();
    writeln!(
        f,
        "pub fn get_embedded_shader(name: &str) -> Option<&'static [u8]> {{"
    )
    .unwrap();
    writeln!(f, "    match name {{").unwrap();

    for (name, path) in compiled_shaders {
        let path_str = path.to_string_lossy().replace('\\', "/");
        writeln!(
            f,
            "        \"{}\" => Some(include_bytes!(\"{}\")),",
            name, path_str
        )
        .unwrap();
    }

    writeln!(f, "        _ => None,").unwrap();
    writeln!(f, "    }}").unwrap();
    writeln!(f, "}}").unwrap();
}

pub fn compile_dxil_and_generate_registry(env: &SdkEnvironment, target_shaders_dir: &Path) {
    let shaders_dir = env.manifest_dir.join("shaders");
    let mut compiled_dxil_shaders = Vec::new();

    let zstdgpu_root = resolve_external_path(env, "external/DirectStorage/zstd/zstdgpu");
    let zstdgpu_shaders_inc =
        resolve_external_path(env, "external/DirectStorage/zstd/zstdgpu/Shaders");
    let thirdparty_dir = resolve_external_path(env, "external/DirectStorage/zstd/ThirdParty");
    let platform_dir = resolve_external_path(env, "external/DirectStorage/zstd/platform");
    let local_include = env.manifest_dir.join("src_cpp/include");
    let local_zstd_shaders = env.manifest_dir.join("shaders/ZSTD");
    let local_brotli_shaders = env.manifest_dir.join("shaders/BrotliG");
    let local_gacl_shaders = env.manifest_dir.join("shaders/GACL");
    let local_gdeflate_shaders = env.manifest_dir.join("shaders/GDeflate");
    let local_geometry_shaders = env.manifest_dir.join("shaders/Geometry");
    let local_ntc_shaders = env.manifest_dir.join("shaders/NTC");

    if shaders_dir.exists() {
        let mut hlsl_files = Vec::new();
        collect_files_recursive(&shaders_dir, "hlsl", &mut hlsl_files);

        for path in &hlsl_files {
            let file_stem = path.file_stem().unwrap().to_string_lossy();
            let dxil_name = format!("{}.dxil", file_stem);
            let dst_dxil = target_shaders_dir.join(&dxil_name);
            let parent_dir = path.parent().unwrap_or(&shaders_dir);
            let entry_point = detect_entry_point(path);

            let mut cmd = Command::new(&env.dxc_compiler);
            cmd.arg("-T")
                .arg("cs_6_6")
                .arg("-E")
                .arg(entry_point)
                .arg("-O3")
                .arg("-HV")
                .arg("2021")
                .arg("-Wno-ignored-attributes")
                .arg("-I")
                .arg(&shaders_dir)
                .arg("-I")
                .arg(parent_dir)
                .arg("-I")
                .arg(&local_include)
                .arg("-I")
                .arg(&local_zstd_shaders)
                .arg("-I")
                .arg(&local_brotli_shaders)
                .arg("-I")
                .arg(&local_gacl_shaders)
                .arg("-I")
                .arg(&local_gdeflate_shaders)
                .arg("-I")
                .arg(&local_geometry_shaders)
                .arg("-I")
                .arg(&local_ntc_shaders);

            if let Some(ref zroot) = zstdgpu_root {
                cmd.arg("-I").arg(zroot);
                cmd.arg("-I").arg(zroot.join("Shaders"));
            }
            if let Some(ref zsh) = zstdgpu_shaders_inc {
                cmd.arg("-I").arg(zsh);
            }
            if let Some(ref tp) = thirdparty_dir {
                cmd.arg("-I").arg(tp);
            }
            if let Some(ref pf) = platform_dir {
                cmd.arg("-I").arg(pf);
            }

            cmd.arg("-Fo").arg(&dst_dxil).arg(path);

            if let Ok(out) = cmd.output()
                && out.status.success()
            {
                compiled_dxil_shaders.push((dxil_name, dst_dxil));
            }
        }
    }

    let registry_file = env.out_dir.join("embedded_dxil_shaders.rs");
    let mut f = File::create(&registry_file).expect("Failed to create embedded_dxil_shaders.rs");

    writeln!(f, "// Auto-generated embedded DXIL shader lookup registry").unwrap();
    writeln!(
        f,
        "pub fn get_embedded_dxil_shader(name: &str) -> Option<&'static [u8]> {{"
    )
    .unwrap();
    writeln!(f, "    match name {{").unwrap();

    for (name, path) in compiled_dxil_shaders {
        let path_str = path.to_string_lossy().replace('\\', "/");
        writeln!(
            f,
            "        \"{}\" => Some(include_bytes!(\"{}\")),",
            name, path_str
        )
        .unwrap();
    }

    writeln!(f, "        _ => None,").unwrap();
    writeln!(f, "    }}").unwrap();
    writeln!(f, "}}").unwrap();
}
