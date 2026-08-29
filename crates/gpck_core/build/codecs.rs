// crates/gpck_core/build/codecs.rs
//! # Native C++ Compression Library Compiler
//!
//! Compiles static libraries for Microsoft GDeflate, AMD Brotli-G SDK,
//! ZstdGPU, and libdeflate using the `cc` build utility.

use super::dxc::{SdkEnvironment, collect_files_recursive, resolve_external_path};
use std::path::Path;

pub fn build_native_codecs(env: &SdkEnvironment, d3d12_headers_dir: &Path) {
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

    // Compile Microsoft GDeflate
    if let Some(ref gdef) = gdeflate_dir
        && gdeflate_wrapper.exists()
    {
        println!("cargo:rustc-cfg=gdeflate_native");
        build.file(&gdeflate_wrapper);

        let mut cpp_files = Vec::new();
        collect_files_recursive(gdef, "cpp", &mut cpp_files);
        for f in cpp_files {
            build.file(f);
        }
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
                let mut cpp_files = Vec::new();
                collect_files_recursive(&src_dir, "cpp", &mut cpp_files);
                for f in cpp_files {
                    build.file(f);
                }
                has_cpp_sources = true;
            }

            let brotli_c_dir = bg_root.join("external/brotli/c");
            if brotli_c_dir.exists() {
                for sub in &["common", "dec", "enc"] {
                    let p = brotli_c_dir.join(sub);
                    if p.exists() {
                        let mut c_files = Vec::new();
                        collect_files_recursive(&p, "c", &mut c_files);
                        for f in c_files {
                            build.file(f);
                        }
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

    // Compile Microsoft ATG ZstdGPU Wrapper
    let zstdgpu_wrapper = env.manifest_dir.join("src_cpp/zstdgpu_wrapper.cpp");
    if zstdgpu_wrapper.exists() {
        build.file(&zstdgpu_wrapper);
        has_cpp_sources = true;
    }

    if let Some(ref ldef_lib) = libdeflate_lib_dir
        && ldef_lib.exists()
    {
        let mut c_files = Vec::new();
        collect_files_recursive(ldef_lib, "c", &mut c_files);
        for f in c_files {
            build.file(f);
        }
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
