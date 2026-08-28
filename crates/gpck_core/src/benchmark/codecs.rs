// crates/gpck_core/src/benchmark/codecs.rs
//! # Raw Algorithm Performance & Codec Latency Benchmark Suite
//!
//! Profiles in-memory throughput, compression efficiency, and decompression latency across
//! three primary execution domains with pixel-perfect table alignment:
//! 1. **CPU / Host Memory Engine** (Store, LZ4, Zstd ATG/Standard, Brotli-G, GDeflate, rANS)
//! 2. **GPU / DirectStorage 1.4 D3D12** (NVMe BypassIO, Driver Metacommands & Custom Queues)
//! 3. **GPU / Vulkan Compute** (Direct-to-VRAM Wavefront Compute Pipelines)

use super::generators::generate_realistic_game_corpus;
use crate::compression::brotlig;
use crate::compression::codecs::{Codec, CompressionMethod};
use crate::compression::gdeflate;
use crate::core::error::GpckResult;
#[cfg(windows)]
use crate::gpu::directstorage::GpuDirectStorage;
use crate::gpu::vulkan::VulkanDecompressor;
use std::fmt::Write;
use std::time::Instant;

const ALGORITHM_PAYLOAD_SIZE: usize = 32 * 1024 * 1024; // 32 MB

/// Executes the full multi-backend algorithm throughput and latency benchmark suite.
pub fn run_algorithm_suite(out: &mut String) -> GpckResult<()> {
    crate::core::logger::log_info(
        "Profiling raw in-memory algorithm throughput across CPU, DirectStorage, and Vulkan backends...",
    );

    writeln!(
        out,
        "=================================================================================================="
    )
    .unwrap();
    writeln!(
        out,
        " Part 1: Raw Algorithm Throughput & Codec Latency (Real Game Corpus 32 MB)"
    )
    .unwrap();
    writeln!(
        out,
        "=================================================================================================="
    )
    .unwrap();
    writeln!(
        out,
        "{:<32} | {:<7} | {:<12} | {:<12} | {:<22}",
        "Method / Execution Backend", "Ratio", "Comp Speed", "Decomp Speed", "Decomp Latency"
    )
    .unwrap();
    writeln!(
        out,
        "---------------------------------+---------+--------------+--------------+------------------------"
    )
    .unwrap();

    // Generate realistic mixed-entropy game asset payload
    let raw_data = generate_realistic_game_corpus(ALGORITHM_PAYLOAD_SIZE);

    // ========================================================================
    // 1. CPU / Host Memory Engine Section
    // ========================================================================
    writeln!(
        out,
        "[CPU / Host Memory Engine]       |         |              |              |"
    )
    .unwrap();
    run_test(
        out,
        "  Store (Baseline)",
        &raw_data,
        CompressionMethod::Store,
        0,
        false,
    );
    run_test(
        out,
        "  LZ4 (HC Level 9)",
        &raw_data,
        CompressionMethod::Lz4,
        9,
        false,
    );
    run_test(
        out,
        "  Zstd (Standard Level 22)",
        &raw_data,
        CompressionMethod::Zstd,
        22,
        false,
    );
    run_test(
        out,
        "  Zstd (ATG Window L9)",
        &raw_data,
        CompressionMethod::Zstd,
        9,
        true,
    );

    if brotlig::is_brotlig_available() {
        run_test(
            out,
            "  Brotli-G (CPU Level 5)",
            &raw_data,
            CompressionMethod::BrotliG,
            5,
            false,
        );
    } else {
        writeln!(
            out,
            "{:<32} | {:<7} | {:>12} | [Unavailable] | -",
            "  Brotli-G (CPU)", "-", "N/A"
        )
        .unwrap();
    }

    if gdeflate::is_gdeflate_available() {
        run_test(
            out,
            "  GDeflate (CPU Level 9)",
            &raw_data,
            CompressionMethod::GDeflate,
            9,
            true,
        );
    } else {
        writeln!(
            out,
            "{:<32} | {:<7} | {:>12} | [Unavailable] | -",
            "  GDeflate (CPU)", "-", "N/A"
        )
        .unwrap();
    }

    run_test(
        out,
        "  rANS (4-Way Interleaved)",
        &raw_data,
        CompressionMethod::Rans,
        1,
        false,
    );

    writeln!(
        out,
        "---------------------------------+---------+--------------+--------------+------------------------"
    )
    .unwrap();

    // ========================================================================
    // 2. DirectStorage 1.4 D3D12 Native GPU Section
    // ========================================================================
    #[cfg(windows)]
    {
        writeln!(
            out,
            "[GPU / DirectStorage 1.4 D3D12]  |         |              |              |"
        )
        .unwrap();
        run_directstorage_benchmark(out, &raw_data);
        writeln!(
            out,
            "---------------------------------+---------+--------------+--------------+------------------------"
        )
        .unwrap();
    }

    // ========================================================================
    // 3. Vulkan Compute GPU (Direct-to-VRAM) Section
    // ========================================================================
    writeln!(
        out,
        "[GPU / Vulkan Compute (VRAM)]    |         |              |              |"
    )
    .unwrap();
    run_vulkan_benchmark(out, &raw_data);

    writeln!(
        out,
        "=================================================================================================="
    )
    .unwrap();
    writeln!(out).unwrap();
    Ok(())
}

/// Runs a standardized CPU compression and decompression benchmark on a given memory buffer.
pub fn run_test(
    out: &mut String,
    name: &str,
    input: &[u8],
    method: CompressionMethod,
    level: i32,
    atg_profile: bool,
) {
    let start = Instant::now();
    let compressed = match Codec::compress(input, method, level, atg_profile) {
        Ok(c) => c,
        Err(_) => {
            let _ = writeln!(
                out,
                "{:<32} | ERROR   | ERROR        | ERROR        | ERROR",
                name
            );
            return;
        }
    };
    let comp_time = start.elapsed().as_secs_f64();

    let iterations = 5;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = Codec::decompress(&compressed, input.len(), method);
    }
    let decomp_time = start.elapsed().as_secs_f64() / iterations as f64;

    let comp_speed = (input.len() as f64 / 1024.0 / 1024.0) / comp_time.max(1e-6);
    let decomp_speed = (input.len() as f64 / 1024.0 / 1024.0) / decomp_time.max(1e-6);
    let latency_ms = decomp_time * 1000.0;
    let ratio = (compressed.len() as f64 / input.len() as f64) * 100.0;

    let _ = writeln!(
        out,
        "{:<32} | {:>5.1}%  | {:>7.0} MB/s | {:>7.0} MB/s | {:>8.2} ms",
        name, ratio, comp_speed, decomp_speed, latency_ms
    );
}

/// Benchmarks Microsoft DirectStorage 1.4 hardware decompression pipelines on Windows.
#[cfg(windows)]
pub fn run_directstorage_benchmark(out: &mut String, raw_data: &[u8]) {
    if let Ok(ds) = GpuDirectStorage::new()
        && ds.is_supported()
    {
        // DirectStorage GPU GDeflate Decompression (Driver Metacommand)
        let gdef_iterations = 15;
        if let Ok(compressed_gdef) = Codec::compress(raw_data, CompressionMethod::GDeflate, 9, true)
            && compressed_gdef.len() < raw_data.len()
            && ds
                .decompress_batch_gpu(&compressed_gdef, raw_data.len())
                .is_ok()
        {
            let start = Instant::now();
            let mut success = true;

            for _ in 0..gdef_iterations {
                if ds
                    .decompress_batch_gpu(&compressed_gdef, raw_data.len())
                    .is_err()
                {
                    success = false;
                    break;
                }
            }

            if success {
                let elapsed = start.elapsed().as_secs_f64() / gdef_iterations as f64;
                let speed = (raw_data.len() as f64 / 1024.0 / 1024.0) / elapsed.max(1e-6);
                let latency_ms = elapsed * 1000.0;
                let ratio = (compressed_gdef.len() as f64 / raw_data.len() as f64) * 100.0;

                let _ = writeln!(
                    out,
                    "{:<32} | {:>5.1}%  | {:>12} | {:>7.0} MB/s | {:>8.2} ms (D3D12 HW)",
                    "  GDeflate (DirectStorage GPU)", ratio, "N/A", speed, latency_ms
                );
            }
        }

        // DirectStorage GPU Brotli-G Decompression (Custom Queue)
        let brotli_iterations = 15;
        if let Ok(compressed_bg) = Codec::compress(raw_data, CompressionMethod::BrotliG, 5, false)
            && compressed_bg.len() < raw_data.len()
            && ds
                .decompress_batch_gpu_brotlig(&compressed_bg, raw_data.len())
                .is_ok()
        {
            let start = Instant::now();
            let mut success = true;

            for _ in 0..brotli_iterations {
                if ds
                    .decompress_batch_gpu_brotlig(&compressed_bg, raw_data.len())
                    .is_err()
                {
                    success = false;
                    break;
                }
            }

            if success {
                let elapsed = start.elapsed().as_secs_f64() / brotli_iterations as f64;
                let speed = (raw_data.len() as f64 / 1024.0 / 1024.0) / elapsed.max(1e-6);
                let latency_ms = elapsed * 1000.0;
                let ratio = (compressed_bg.len() as f64 / raw_data.len() as f64) * 100.0;

                let _ = writeln!(
                    out,
                    "{:<32} | {:>5.1}%  | {:>12} | {:>7.0} MB/s | {:>8.2} ms (Custom Queue)",
                    "  Brotli-G (DirectStorage GPU)", ratio, "N/A", speed, latency_ms
                );
            }
        }

        // DirectStorage GPU Zstandard Decompression
        let zstd_iterations = 3;
        if let Ok(compressed_zstd) = Codec::compress(raw_data, CompressionMethod::Zstd, 9, true)
            && compressed_zstd.len() < raw_data.len()
            && ds
                .decompress_batch_gpu_zstd(&compressed_zstd, raw_data.len(), 0)
                .is_ok()
        {
            let start = Instant::now();
            let mut success = true;

            for _ in 0..zstd_iterations {
                if ds
                    .decompress_batch_gpu_zstd(&compressed_zstd, raw_data.len(), 0)
                    .is_err()
                {
                    success = false;
                    break;
                }
            }

            if success {
                let elapsed = start.elapsed().as_secs_f64() / zstd_iterations as f64;
                let speed = (raw_data.len() as f64 / 1024.0 / 1024.0) / elapsed.max(1e-6);
                let latency_ms = elapsed * 1000.0;
                let ratio = (compressed_zstd.len() as f64 / raw_data.len() as f64) * 100.0;

                let _ = writeln!(
                    out,
                    "{:<32} | {:>5.1}%  | {:>12} | {:>7.0} MB/s | {:>8.2} ms (Fallback)",
                    "  Zstd (DirectStorage GPU)", ratio, "N/A", speed, latency_ms
                );
            }
        }
    } else {
        let _ = writeln!(
            out,
            "{:<32} | {:<7} | {:>12} | [Unavailable] | -",
            "  DirectStorage 1.4 (GPU)", "-", "N/A"
        );
    }
}

/// Benchmarks Vulkan Compute GPU decompression shaders (Direct-to-VRAM).
pub fn run_vulkan_benchmark(out: &mut String, raw_data: &[u8]) {
    if let Ok(gpu) = VulkanDecompressor::new() {
        // Vulkan GPU Brotli-G Direct-to-VRAM (AMD RDNA Wave32 Optimized)
        let mut vulkan_brotli_tested = false;
        if let Ok(compressed_bg) = Codec::compress(raw_data, CompressionMethod::BrotliG, 5, false) {
            let iterations = 15;
            let start = Instant::now();
            let mut success = true;

            for _ in 0..iterations {
                if gpu
                    .decompress_to_vram(&compressed_bg, raw_data.len(), CompressionMethod::BrotliG)
                    .is_err()
                {
                    success = false;
                    break;
                }
            }

            if success {
                let elapsed = start.elapsed().as_secs_f64() / iterations as f64;
                let speed = (raw_data.len() as f64 / 1024.0 / 1024.0) / elapsed.max(1e-6);
                let latency_ms = elapsed * 1000.0;
                let ratio = (compressed_bg.len() as f64 / raw_data.len() as f64) * 100.0;

                let _ = writeln!(
                    out,
                    "{:<32} | {:>5.1}%  | {:>12} | {:>7.0} MB/s | {:>8.2} ms (RDNA Wave32)",
                    "  Brotli-G (Vulkan GPU)", ratio, "N/A", speed, latency_ms
                );
                vulkan_brotli_tested = true;
            }
        }

        if !vulkan_brotli_tested {
            let _ = writeln!(
                out,
                "{:<32} | {:<7} | {:>12} | [Unavailable] | -",
                "  Brotli-G (Vulkan GPU)", "-", "N/A"
            );
        }

        // Vulkan GPU Zstandard Multi-Pass Compute
        let mut vulkan_zstd_tested = false;
        if let Ok(compressed_zstd) = Codec::compress(raw_data, CompressionMethod::Zstd, 9, true) {
            let iterations = 15;
            let start = Instant::now();
            let mut success = true;

            for _ in 0..iterations {
                if gpu
                    .decompress_to_vram(&compressed_zstd, raw_data.len(), CompressionMethod::Zstd)
                    .is_err()
                {
                    success = false;
                    break;
                }
            }

            if success {
                let elapsed = start.elapsed().as_secs_f64() / iterations as f64;
                let speed = (raw_data.len() as f64 / 1024.0 / 1024.0) / elapsed.max(1e-6);
                let latency_ms = elapsed * 1000.0;
                let ratio = (compressed_zstd.len() as f64 / raw_data.len() as f64) * 100.0;

                let _ = writeln!(
                    out,
                    "{:<32} | {:>5.1}%  | {:>12} | {:>7.0} MB/s | {:>8.2} ms (Multi-Pass)",
                    "  Zstd (Vulkan GPU)", ratio, "N/A", speed, latency_ms
                );
                vulkan_zstd_tested = true;
            }
        }

        if !vulkan_zstd_tested {
            let _ = writeln!(
                out,
                "{:<32} | {:<7} | {:>12} | [Unavailable] | -",
                "  Zstd (Vulkan GPU)", "-", "N/A"
            );
        }

        // Vulkan GPU GDeflate Direct-to-VRAM (64KB Hardware Tiles)
        let mut vulkan_gdef_tested = false;
        if let Ok(compressed_gdef) = Codec::compress(raw_data, CompressionMethod::GDeflate, 9, true)
        {
            let iterations = 15;
            let start = Instant::now();
            let mut success = true;

            for _ in 0..iterations {
                if gpu
                    .decompress_to_vram(
                        &compressed_gdef,
                        raw_data.len(),
                        CompressionMethod::GDeflate,
                    )
                    .is_err()
                {
                    success = false;
                    break;
                }
            }

            if success {
                let elapsed = start.elapsed().as_secs_f64() / iterations as f64;
                let speed = (raw_data.len() as f64 / 1024.0 / 1024.0) / elapsed.max(1e-6);
                let latency_ms = elapsed * 1000.0;
                let ratio = (compressed_gdef.len() as f64 / raw_data.len() as f64) * 100.0;

                let _ = writeln!(
                    out,
                    "{:<32} | {:>5.1}%  | {:>12} | {:>7.0} MB/s | {:>8.2} ms (Compute 64K)",
                    "  GDeflate (Vulkan GPU)", ratio, "N/A", speed, latency_ms
                );
                vulkan_gdef_tested = true;
            }
        }

        if !vulkan_gdef_tested {
            let _ = writeln!(
                out,
                "{:<32} | {:<7} | {:>12} | [Unavailable] | -",
                "  GDeflate (Vulkan GPU)", "-", "N/A"
            );
        }
    } else {
        let _ = writeln!(
            out,
            "{:<32} | {:<7} | {:>12} | [Unavailable] | -",
            "  Vulkan Compute (GPU)", "-", "N/A"
        );
    }
}
