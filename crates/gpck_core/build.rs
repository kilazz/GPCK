// crates/gpck_core/build.rs
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

// ============================================================================
// Single Source of Truth: SDK & Toolchain Discovery
// ============================================================================

struct SdkEnvironment {
    dxc_compiler: PathBuf,
    manifest_dir: PathBuf,
    workspace_root: PathBuf,
    out_dir: PathBuf,
}

impl SdkEnvironment {
    fn discover() -> Self {
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
        if let Ok(path) = env::var("GPCK_DXC_PATH") {
            let p = PathBuf::from(path);
            if p.exists() && Self::verify_dxc_spirv(&p) {
                return Some(p);
            }
        }

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

        if let Some(path) = find_in_path(if cfg!(windows) { "dxc.exe" } else { "dxc" })
            && Self::verify_dxc_spirv(&path)
        {
            return Some(path);
        }

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

// ============================================================================
// Main Build Driver
// ============================================================================

fn main() {
    println!("cargo:rerun-if-changed=shaders");
    println!("cargo:rerun-if-changed=src_cpp");
    println!("cargo:rerun-if-env-changed=VULKAN_SDK");
    println!("cargo:rerun-if-env-changed=GPCK_DXC_PATH");

    let env = SdkEnvironment::discover();

    let d3d12_headers_dir = env.out_dir.join("zstd_d3d12_headers");
    let target_shaders_dir = env.out_dir.join("shaders");
    fs::create_dir_all(&target_shaders_dir).ok();
    fs::create_dir_all(&d3d12_headers_dir).ok();

    // Build and execute zstdgpu_srt_tool to generate SRT structures
    if let Some(zstdgpu_root) = resolve_external_path(&env, "external/DirectStorage/zstd/zstdgpu") {
        ensure_zstdgpu_generated_headers(&env, &zstdgpu_root);
        sync_zstdgpu_headers_for_shaders(&env, &zstdgpu_root);
    }

    // Compile D3D12 C-Headers for ZstdGPU
    generate_d3d12_zstd_headers(&env, &env.dxc_compiler, &d3d12_headers_dir);

    // Build native C++ static libraries (GDeflate / Brotli-G / ZstdGPU)
    build_static_compression_libraries(&env, &d3d12_headers_dir);

    // Compile all HLSL compute shaders to SPIR-V for Vulkan Compute
    compile_spirv_and_generate_registry(&env, &target_shaders_dir);

    // Compile all HLSL compute shaders to native DXIL (SM 6.6) for DirectX 12
    compile_dxil_and_generate_registry(&env, &target_shaders_dir);

    // Copy DirectStorage and D3D12 DLLs on Windows
    #[cfg(target_os = "windows")]
    copy_windows_dlls(&env);
}

fn resolve_external_path(env: &SdkEnvironment, subpath: &str) -> Option<PathBuf> {
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

fn ensure_zstdgpu_generated_headers(env: &SdkEnvironment, zstdgpu_root: &Path) {
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

fn sync_zstdgpu_headers_for_shaders(env: &SdkEnvironment, zstdgpu_root: &Path) {
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

fn detect_entry_point(path: &Path) -> &'static str {
    if let Ok(content) = fs::read_to_string(path)
        && (content.contains("void CSMain(") || content.contains("void CSMain ("))
    {
        return "CSMain";
    }
    "main"
}

fn compile_spirv_and_generate_registry(env: &SdkEnvironment, target_shaders_dir: &Path) {
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
        collect_hlsl_files_recursive(&shaders_dir, &mut hlsl_files);

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
    let mut f = fs::File::create(&registry_file).expect("Failed to create embedded_shaders.rs");

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

fn compile_dxil_and_generate_registry(env: &SdkEnvironment, target_shaders_dir: &Path) {
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
        collect_hlsl_files_recursive(&shaders_dir, &mut hlsl_files);

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
    let mut f =
        fs::File::create(&registry_file).expect("Failed to create embedded_dxil_shaders.rs");

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

fn generate_d3d12_zstd_headers(env: &SdkEnvironment, dxc: &Path, out_headers_dir: &Path) {
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

                let mut cmd = Command::new(dxc);
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

fn build_static_compression_libraries(env: &SdkEnvironment, d3d12_headers_dir: &Path) {
    println!("cargo:rustc-check-cfg=cfg(gdeflate_native)");
    println!("cargo:rustc-check-cfg=cfg(brotlig_native)");

    let gdeflate_dir = resolve_external_path(env, "external/DirectStorage/GDeflate/GDeflate");
    let gdeflate_wrapper = env.manifest_dir.join("src_cpp/gdeflate_wrapper.cpp");
    let libdeflate_root =
        resolve_external_path(env, "external/DirectStorage/GDeflate/3rdparty/libdeflate");
    let libdeflate_lib_dir = libdeflate_root.as_ref().map(|p| p.join("lib"));

    let brotlig_sdk_root = resolve_external_path(env, "external/brotli_g_sdk");
    let brotlig_wrapper = env.manifest_dir.join("src_cpp/brotlig_wrapper.cpp");

    let zstd_root = resolve_external_path(env, "external/DirectStorage/zstd");
    let zstdgpu_dir = resolve_external_path(env, "external/DirectStorage/zstd/zstdgpu");
    let zstdgpu_thirdparty = resolve_external_path(env, "external/DirectStorage/zstd/ThirdParty");
    let zstdgpu_platform = resolve_external_path(env, "external/DirectStorage/zstd/platform");
    let local_include = env.manifest_dir.join("src_cpp/include");
    let src_cpp = env.manifest_dir.join("src_cpp");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .static_crt(false)
        .warnings(false)
        .flag_if_supported("/EHsc")
        .define("_CRT_SECURE_NO_WARNINGS", None)
        .define("NDEBUG", "1")
        .include(d3d12_headers_dir)
        .include(&local_include)
        .include(&src_cpp);

    if let Some(ref gdef) = gdeflate_dir {
        build.include(gdef);
    }
    if let Some(ref ldef) = libdeflate_root {
        build.include(ldef);
        build.include(ldef.join("common"));
    }
    if let Some(ref ldef_lib) = libdeflate_lib_dir {
        build.include(ldef_lib);
    }
    if let Some(ref zroot) = zstd_root {
        build.include(zroot);
    }
    if let Some(ref zgpu) = zstdgpu_dir {
        build.include(zgpu);
    }
    if let Some(ref ztp) = zstdgpu_thirdparty {
        build.include(ztp);
    }
    if let Some(ref zpf) = zstdgpu_platform {
        build.include(zpf);
    }

    let mut has_cpp_sources = false;

    // Compile GDeflate
    if let Some(ref gdef) = gdeflate_dir
        && gdeflate_wrapper.exists()
    {
        println!("cargo:rustc-cfg=gdeflate_native");
        build.file(&gdeflate_wrapper);
        add_cpp_files_recursive(&mut build, gdef);
        has_cpp_sources = true;
    }

    // Compile AMD Brotli-G SDK
    if let Some(ref bg_root) = brotlig_sdk_root {
        let brotli_c_constants = bg_root.join("external/brotli/c/common/constants.h");
        let brotli_c_available = brotli_c_constants.exists();

        if brotli_c_available {
            build.include(bg_root.join("inc"));
            build.include(bg_root.join("inc/common"));
            build.include(bg_root.join("inc/encoder"));
            build.include(bg_root.join("inc/decoder"));
            build.include(bg_root.join("src"));
            build.include(bg_root.join("external"));
            build.include(bg_root.join("external/brotli/c/include"));
            build.include(bg_root.join("external/brotli/c/common"));
            build.include(bg_root.join("external/brotli/c/enc"));
            build.include(bg_root.join("external/brotli/c/dec"));
            build.define("BROTLIG_SDK_AVAILABLE", "1");
            println!("cargo:rustc-cfg=brotlig_native");

            let src_dir = bg_root.join("src");
            if src_dir.exists() {
                add_cpp_files_recursive(&mut build, &src_dir);
                has_cpp_sources = true;
            }

            let brotli_c_dir = bg_root.join("external/brotli/c");
            if brotli_c_dir.exists() {
                for sub in &["common", "dec", "enc"] {
                    let p = brotli_c_dir.join(sub);
                    if p.exists() {
                        add_c_files_recursive(&mut build, &p);
                    }
                }
                has_cpp_sources = true;
            }
        }
    }

    if brotlig_wrapper.exists() {
        build.file(&brotlig_wrapper);
        has_cpp_sources = true;
    }

    // Compile ZstdGPU Wrapper
    let zstdgpu_wrapper = env.manifest_dir.join("src_cpp/zstdgpu_wrapper.cpp");
    if zstdgpu_wrapper.exists() {
        build.file(&zstdgpu_wrapper);
        has_cpp_sources = true;
    }

    if let Some(ref ldef_lib) = libdeflate_lib_dir
        && ldef_lib.exists()
    {
        add_c_files_recursive(&mut build, ldef_lib);
        has_cpp_sources = true;
    }

    if let Some(ref zgpu) = zstdgpu_dir {
        let zstdgpu_cpp = zgpu.join("zstdgpu.cpp");
        let zstdgpu_ref_store = zgpu.join("zstdgpu_reference_store.cpp");

        if zstdgpu_cpp.exists() {
            build.file(&zstdgpu_cpp);
            println!("cargo:rerun-if-changed={}", zstdgpu_cpp.display());
            has_cpp_sources = true;
        }
        if zstdgpu_ref_store.exists() {
            build.file(&zstdgpu_ref_store);
            println!("cargo:rerun-if-changed={}", zstdgpu_ref_store.display());
            has_cpp_sources = true;
        }
    }

    if has_cpp_sources {
        build.compile("gpck_native_codecs");
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn add_c_files_recursive(build: &mut cc::Build, dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                add_c_files_recursive(build, &path);
            } else if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("c") {
                build.file(&path);
            }
        }
    }
}

fn add_cpp_files_recursive(build: &mut cc::Build, dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                add_cpp_files_recursive(build, &path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("cpp") {
                build.file(&path);
            }
        }
    }
}

fn collect_hlsl_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                collect_hlsl_files_recursive(&path, files);
            } else if path.extension().and_then(|s| s.to_str()) == Some("hlsl") {
                files.push(path);
            }
        }
    }
}

fn find_in_path(exe: &str) -> Option<PathBuf> {
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

#[cfg(target_os = "windows")]
fn copy_windows_dlls(env: &SdkEnvironment) {
    let nuget_search = match resolve_external_path(env, "nuget") {
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
                fs::create_dir_all(parent).ok();
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
