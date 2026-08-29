// crates/gpck_core/build.rs
//! # GPCK Core Native Build Script Driver

use std::fs;

#[path = "build/dxc.rs"]
mod dxc;

#[path = "build/zstdgpu.rs"]
mod zstdgpu;

#[path = "build/codecs.rs"]
mod codecs;

#[path = "build/shaders.rs"]
mod shaders;

#[path = "build/windows_dlls.rs"]
mod windows_dlls;

fn main() {
    println!("cargo:rerun-if-changed=shaders");
    println!("cargo:rerun-if-changed=src_cpp");
    println!("cargo:rerun-if-env-changed=VULKAN_SDK");
    println!("cargo:rerun-if-env-changed=GPCK_DXC_PATH");

    // Discover DXC compiler toolchain & SDK paths
    let env = dxc::SdkEnvironment::discover();

    let d3d12_headers_dir = env.out_dir.join("zstd_d3d12_headers");
    let target_shaders_dir = env.out_dir.join("shaders");
    let _ = fs::create_dir_all(&target_shaders_dir);
    let _ = fs::create_dir_all(&d3d12_headers_dir);

    // Build and execute zstdgpu_srt_tool to generate SRT structures
    if let Some(zstdgpu_root) =
        dxc::resolve_external_path(&env, "external/DirectStorage/zstd/zstdgpu")
    {
        zstdgpu::ensure_zstdgpu_generated_headers(&env, &zstdgpu_root);
        zstdgpu::sync_zstdgpu_headers_for_shaders(&env, &zstdgpu_root);
    }

    // Compile D3D12 C-Headers for ZstdGPU
    zstdgpu::generate_d3d12_zstd_headers(&env, &d3d12_headers_dir);

    // Build native C++ static libraries (GDeflate / Brotli-G / ZstdGPU)
    codecs::build_native_codecs(&env, &d3d12_headers_dir);

    // Compile all HLSL compute shaders to SPIR-V (Vulkan Compute)
    shaders::compile_spirv_and_generate_registry(&env, &target_shaders_dir);

    // Compile all HLSL compute shaders to native DXIL (DirectX 12 SM 6.6)
    shaders::compile_dxil_and_generate_registry(&env, &target_shaders_dir);

    // Copy DirectStorage and D3D12 DLLs on Windows
    windows_dlls::copy_windows_dlls(&env);
}
