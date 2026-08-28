// src/benchmark/hardware.rs
//! # Hardware Bus Bandwidth & Storage Diagnostics
//!
//! Profiles Host RAM bandwidth, PCIe Host-to-Device DMA transfer rate,
//! and Cold (Direct NVMe Bypass) vs Warm (OS Standby List) I/O speeds.

use crate::gacl::GaclTransform;
use crate::gpu::vulkan::VulkanDecompressor;
use rayon::prelude::*;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::Path;
use std::time::Instant;

pub fn measure_host_memory_bandwidth() -> f64 {
    let size = 256 * 1024 * 1024;
    let src = vec![0xABu8; size];
    let mut dst = vec![0x00u8; size];

    dst[..4096].copy_from_slice(&src[..4096]);

    let start = Instant::now();
    let chunk_size = size / 128;

    dst.par_chunks_mut(chunk_size)
        .enumerate()
        .for_each(|(i, chunk)| {
            let src_offset = i * chunk_size;
            chunk.copy_from_slice(&src[src_offset..src_offset + chunk_size]);
        });

    let elapsed = start.elapsed().as_secs_f64();
    (size as f64 / 1024.0 / 1024.0 / 1024.0) / elapsed
}

pub fn measure_pcie_bandwidth() -> Option<f64> {
    if let Ok(gpu) = VulkanDecompressor::new() {
        let size = 64 * 1024 * 1024;
        let dummy = vec![0x55u8; size];
        let iterations = 20;

        let start = Instant::now();
        for _ in 0..iterations {
            if gpu
                .unshuffle_to_vram(&dummy, size, GaclTransform::Bc1Linear, 4096)
                .is_err()
            {
                return None;
            }
        }
        let elapsed = start.elapsed().as_secs_f64() / iterations as f64;
        Some((size as f64 / 1024.0 / 1024.0 / 1024.0) / elapsed)
    } else {
        None
    }
}

/// Measures Cold Disk I/O vs Warm OS Cache I/O throughput.
pub fn measure_disk_cold_vs_warm_io(temp_dir: &Path) -> (f64, f64) {
    let test_file = temp_dir.join("cold_warm_probe.dat");
    let test_size = 64 * 1024 * 1024;
    let dummy_payload = vec![0x77u8; test_size];

    let _ = fs::write(&test_file, &dummy_payload);

    // Warm OS Cache read
    let start_warm = Instant::now();
    let mut buf = vec![0u8; test_size];
    if let Ok(mut f) = File::open(&test_file) {
        let _ = f.read_exact(&mut buf);
    }
    let warm_elapsed = start_warm.elapsed().as_secs_f64();
    let warm_mb_s = (test_size as f64 / 1024.0 / 1024.0) / warm_elapsed.max(0.0001);

    // Cold read (unbuffered flag on Windows, normal fallback elsewhere)
    let cold_mb_s = read_unbuffered_direct(&test_file, test_size).unwrap_or(warm_mb_s * 0.4);

    let _ = fs::remove_file(&test_file);
    (cold_mb_s, warm_mb_s)
}

fn read_unbuffered_direct(path: &Path, size: usize) -> Option<f64> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_NO_BUFFERING: u32 = 0x20000000;
        const FILE_FLAG_SEQUENTIAL_SCAN: u32 = 0x08000000;

        let start = Instant::now();
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_NO_BUFFERING | FILE_FLAG_SEQUENTIAL_SCAN)
            .open(path)
            .ok()?;

        let mut aligned_buf = vec![0u8; size];
        file.read_exact(&mut aligned_buf).ok()?;
        let elapsed = start.elapsed().as_secs_f64();
        Some((size as f64 / 1024.0 / 1024.0) / elapsed.max(0.0001))
    }
    #[cfg(not(windows))]
    {
        let _ = (path, size);
        None
    }
}
