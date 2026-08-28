// crates/gpck_core/src/benchmark/mod.rs
//! # Modular Asset Streaming Telemetry & Benchmark Orchestrator
//!
//! Orchestrates the multi-stage diagnostic and telemetry benchmarking suite:
//! - Part 1: Raw Algorithm Throughput & Codec Latency (CPU vs Vulkan GPU vs DirectStorage)
//! - Part 2: Real-World 4K Texture Streaming Waterfall (BC1–BC7 DMA & GPU Unshuffle)
//! - Part 3: Mip-Drop Texture Streaming & LOD Transition Latency
//! - Part 4: Open-World Sector Burst & Queue Priority Contention
//! - Part 5: Meshlet Geometry Conditioning, Quantization & Task/Mesh Shader Culling

pub mod codecs;
pub mod generators;
pub mod gpu_timestamps;
pub mod hardware;
pub mod meshlet;
pub mod mip_streaming;
pub mod sector_burst;
pub mod texture_waterfall;

use self::codecs::{run_algorithm_suite, run_directstorage_benchmark, run_test};
use self::generators::format_size;
use self::hardware::{
    measure_disk_cold_vs_warm_io, measure_host_memory_bandwidth, measure_pcie_bandwidth,
};
use self::meshlet::run_meshlet_suite;
use self::mip_streaming::run_mip_streaming_suite;
use self::sector_burst::run_sector_burst_suite;
use self::texture_waterfall::run_real_world_texture_suite;

use crate::compression::brotlig;
use crate::compression::codecs::CompressionMethod;
use crate::compression::gdeflate;
use crate::core::error::GpckResult;
use crate::format::archive::GameArchive;
#[cfg(windows)]
use crate::gpu::directstorage::{GpuDirectStorage, QueuePriority};
#[cfg(windows)]
use crate::gpu::directstorage_sys::{
    DSTORAGE_COMPRESSION_FORMAT_GDEFLATE, DSTORAGE_COMPRESSION_FORMAT_NONE,
    DSTORAGE_COMPRESSION_FORMAT_ZSTD, DSTORAGE_REQUEST,
};
use crate::gpu::vulkan::VulkanDecompressor;
use rayon::prelude::*;
use std::fmt::Write;
use std::fs;
use std::io::Read as IoRead;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

#[cfg(windows)]
use windows::core::Interface;

/// Runs the standard benchmark suite and prints the result to stdout.
pub fn run_benchmark_suite() -> GpckResult<()> {
    let result = run_benchmark_suite_string(None)?;
    println!("{}", result);
    Ok(())
}

/// Executes the benchmark suite or inspects a specific custom asset, returning the formatted report string.
pub fn run_benchmark_suite_string(custom_path: Option<&Path>) -> GpckResult<String> {
    let mut out = String::new();
    writeln!(
        &mut out,
        "================================================================================"
    )
    .unwrap();
    writeln!(
        &mut out,
        " GPCK Industrial Asset Streaming & GPU Telemetry Diagnostic Suite"
    )
    .unwrap();
    if let Ok(dir) = std::env::current_dir() {
        writeln!(&mut out, " Execution Working Directory: {:?}", dir).unwrap();
    }
    writeln!(
        &mut out,
        "================================================================================"
    )
    .unwrap();

    print_system_report(&mut out)?;

    if let Some(path) = custom_path {
        if path.extension().and_then(|s| s.to_str()) == Some("gtoc") {
            run_archive_benchmark(path, &mut out)?;
        } else {
            run_file_benchmark(path, &mut out)?;
        }
        return Ok(out);
    }

    crate::core::logger::log_info("Profiling system memory and PCIe bus bandwidth...");
    let host_mem_speed = measure_host_memory_bandwidth();
    let pcie_speed_str = match measure_pcie_bandwidth() {
        Some(speed) => format!("{:.1} GB/s (PCIe Host-to-Device DMA)", speed),
        None => "[GPU DMA not available]".to_string(),
    };

    let temp_dir = std::env::temp_dir();
    let (cold_io_mb, warm_io_mb) = measure_disk_cold_vs_warm_io(&temp_dir);

    writeln!(out, "--- Hardware Bus & Storage Bandwidth Limits ---").unwrap();
    writeln!(
        out,
        "Host RAM Bandwidth          : {:.1} GB/s (Multi-threaded memcpy limit)",
        host_mem_speed
    )
    .unwrap();
    writeln!(out, "PCIe Transfer Bandwidth     : {}", pcie_speed_str).unwrap();
    writeln!(
        out,
        "Storage Cold I/O (Bypass)   : {:.1} MB/s (Direct NVMe hardware read)",
        cold_io_mb
    )
    .unwrap();
    writeln!(
        out,
        "Storage Warm I/O (RAM Cache): {:.1} MB/s (OS Standby List)\n",
        warm_io_mb
    )
    .unwrap();

    // 5-Stage Benchmark Pipeline Execution
    run_algorithm_suite(&mut out)?;
    run_real_world_texture_suite(&mut out)?;
    run_mip_streaming_suite(&mut out)?;
    run_sector_burst_suite(&mut out)?;
    run_meshlet_suite(&mut out)?;

    Ok(out)
}

fn print_system_report(out: &mut String) -> GpckResult<()> {
    writeln!(out, "--- System & Hardware Runtime Report ---").unwrap();
    writeln!(out, "Operating System       : {}", std::env::consts::OS).unwrap();
    writeln!(out, "CPU Architecture       : {}", std::env::consts::ARCH).unwrap();
    writeln!(
        out,
        "Available CPU Threads  : {}",
        rayon::current_num_threads()
    )
    .unwrap();

    #[cfg(windows)]
    let ds_status = match GpuDirectStorage::new() {
        Ok(ds) if ds.is_supported() => {
            "ACTIVE (Agility SDK 721 + DirectStorage 1.4 Native BypassIO)".to_string()
        }
        Ok(_) => "UNSUPPORTED HARDWARE".to_string(),
        Err(e) => format!("UNAVAILABLE ({})", e),
    };
    #[cfg(not(windows))]
    let ds_status = "UNAVAILABLE (Non-Windows)".to_string();

    let vulkan_info = match VulkanDecompressor::new() {
        Ok(vk) => format!(
            "READY (Device: {}, Subgroup: {})",
            vk.device_name(),
            vk.subgroup_size()
        ),
        Err(_) => "UNAVAILABLE".to_string(),
    };

    writeln!(out, "DirectStorage 1.4 GPU  : {}", ds_status).unwrap();
    writeln!(out, "Vulkan Compute Engine  : {}", vulkan_info).unwrap();
    writeln!(out, "----------------------------------------\n").unwrap();
    Ok(())
}

fn run_file_benchmark(path: &Path, out: &mut String) -> GpckResult<()> {
    let raw_data = fs::read(path)?;
    let filename = path.file_name().unwrap_or_default().to_string_lossy();

    writeln!(out, "--- Custom File Deep Benchmark: {} ---", filename).unwrap();
    writeln!(out, "File Size: {}\n", format_size(raw_data.len() as u64)).unwrap();

    writeln!(
        out,
        "{:<26} | {:<7} | {:<13} | {:<14} | {:<12}",
        "Method", "Ratio", "Comp Speed", "Decomp Speed", "Decomp Latency"
    )
    .unwrap();
    writeln!(
        out,
        "--------------------------------------------------------------------------------"
    )
    .unwrap();

    run_test(
        out,
        "LZ4 (HC L9)",
        &raw_data,
        CompressionMethod::Lz4,
        9,
        false,
    );
    run_test(
        out,
        "Zstd (ATG L9)",
        &raw_data,
        CompressionMethod::Zstd,
        9,
        true,
    );
    run_test(
        out,
        "Zstd (Standard L22)",
        &raw_data,
        CompressionMethod::Zstd,
        22,
        false,
    );
    run_test(
        out,
        "rANS (4-Way)",
        &raw_data,
        CompressionMethod::Rans,
        1,
        false,
    );

    if gdeflate::is_gdeflate_available() {
        run_test(
            out,
            "GDeflate (CPU L9)",
            &raw_data,
            CompressionMethod::GDeflate,
            9,
            true,
        );
    }

    if brotlig::is_brotlig_available() {
        run_test(
            out,
            "Brotli-G (CPU L5)",
            &raw_data,
            CompressionMethod::BrotliG,
            5,
            false,
        );
    }

    #[cfg(windows)]
    run_directstorage_benchmark(out, &raw_data);

    writeln!(out).unwrap();
    Ok(())
}

fn run_archive_benchmark(gtoc_path: &Path, out: &mut String) -> GpckResult<()> {
    let archive = Arc::new(GameArchive::open(gtoc_path)?);
    let entries = archive.get_all_entries()?;
    let total_uncompressed = archive.total_uncompressed_size() as u64;

    let filename = gtoc_path.file_name().unwrap_or_default().to_string_lossy();
    writeln!(out, "--- Archive Telemetry Inspection: {} ---", filename).unwrap();
    writeln!(out, "Total Assets in TOC     : {}", entries.len()).unwrap();
    writeln!(
        out,
        "Total Uncompressed Size : {}\n",
        format_size(total_uncompressed)
    )
    .unwrap();

    let start = Instant::now();
    let total_decompressed: usize = entries
        .par_iter()
        .map(|entry| {
            if let Ok(mut stream) = archive.open_stream(entry) {
                let mut buf = vec![0u8; 64 * 1024];
                let mut read_acc = 0;
                while let Ok(n) = stream.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    read_acc += n;
                }
                read_acc
            } else {
                0
            }
        })
        .sum();

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0001 {
        (total_decompressed as f64 / 1024.0 / 1024.0) / elapsed
    } else {
        0.0
    };

    writeln!(
        out,
        "Parallel CPU VFS Read Speed : {:.0} MB/s ({:.2} s total)",
        speed, elapsed
    )
    .unwrap();

    #[cfg(windows)]
    if let Ok(ds) = GpuDirectStorage::new()
        && ds.is_supported()
    {
        let gdat_path = gtoc_path.with_extension("gdat");
        if gdat_path.exists()
            && let Ok(dstorage_file) = ds.open_file(&gdat_path)
        {
            let vram_size = total_uncompressed.min(1024 * 1024 * 1024);

            if let Ok(vram_buffer) = ds.create_vram_buffer(vram_size) {
                let mut current_offset = 0u64;
                let start = Instant::now();

                let dest_ptr = Interface::as_raw(&vram_buffer);

                for entry in &entries {
                    let method = CompressionMethod::from_flags(entry.flags);
                    let ds_format = match method {
                        CompressionMethod::GDeflate => DSTORAGE_COMPRESSION_FORMAT_GDEFLATE,
                        CompressionMethod::Zstd => DSTORAGE_COMPRESSION_FORMAT_ZSTD,
                        _ => DSTORAGE_COMPRESSION_FORMAT_NONE,
                    };
                    let gacl_transform = (entry.flags
                        & crate::format::archive::MASK_GACL_TRANSFORM)
                        >> crate::format::archive::SHIFT_GACL_TRANSFORM;

                    if let Ok(chunks) = archive.get_chunk_table(entry) {
                        for chunk in chunks {
                            if chunk.offset >= 0 {
                                if current_offset + chunk.original_size as u64 > vram_size {
                                    current_offset = 0;
                                }

                                let mut req: DSTORAGE_REQUEST = unsafe { std::mem::zeroed() };
                                req.set_file_to_buffer(
                                    dstorage_file.ptr(),
                                    chunk.offset as u64,
                                    chunk.compressed_size,
                                    dest_ptr,
                                    current_offset,
                                    chunk.original_size,
                                    ds_format,
                                    gacl_transform as u8,
                                );
                                ds.enqueue_buffer_request(QueuePriority::Normal, &req);
                                current_offset += chunk.original_size as u64;
                            }
                        }
                    }
                }

                if let Ok(fence_val) = ds.flush_and_signal(QueuePriority::Normal)
                    && ds.wait_for_fence(QueuePriority::Normal, fence_val).is_ok()
                {
                    let elapsed = start.elapsed().as_secs_f64();
                    let ds_speed = if elapsed > 0.0001 {
                        (total_uncompressed as f64 / 1024.0 / 1024.0) / elapsed
                    } else {
                        0.0
                    };
                    writeln!(
                        out,
                        "DirectStorage Direct-to-VRAM: {:.0} MB/s ({:.2} s total)",
                        ds_speed, elapsed
                    )
                    .unwrap();
                }
            }
        }
    }

    writeln!(out).unwrap();
    Ok(())
}
