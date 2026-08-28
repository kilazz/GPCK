// crates/gpck_core/tests/test_brotlig_realworld_fuzz.rs
//! # Deep Real-World Texture & Non-Aligned Stress Tests for AMD Brotli-G

use gpck_core::compression::brotlig;
use gpck_core::compression::codecs::{Codec, CompressionMethod};
use rayon::prelude::*;

/// Helper that verifies byte-exact compression-decompression roundtrip
fn verify_roundtrip(name: &str, data: &[u8], level: i32) {
    let compressed = match Codec::compress(data, CompressionMethod::BrotliG, level, false) {
        Ok(c) => c,
        Err(e) => panic!("[FAIL COMPRESS] '{}' at level {}: {}", name, level, e),
    };

    let decompressed = match Codec::decompress(&compressed, data.len(), CompressionMethod::BrotliG)
    {
        Ok(d) => d,
        Err(e) => panic!("[FAIL DECOMPRESS] '{}' at level {}: {}", name, level, e),
    };

    if decompressed != data {
        // Find first mismatch
        let mut mismatch_idx = 0;
        for i in 0..data.len().min(decompressed.len()) {
            if decompressed[i] != data[i] {
                mismatch_idx = i;
                break;
            }
        }

        let start = mismatch_idx.saturating_sub(8);
        let end_orig = (mismatch_idx + 16).min(data.len());
        let end_dec = (mismatch_idx + 16).min(decompressed.len());

        panic!(
            "\n==================== [ROUNDTRIP MISMATCH: {} (Level {})] ====================\n\
             Original Size   : {} bytes\n\
             Decomp Size     : {} bytes\n\
             Mismatch Offset : Byte {} (0x{:04X})\n\
             Expected Around : {:02X?}\n\
             Got Around      : {:02X?}\n\
             ================================================================================",
            name,
            level,
            data.len(),
            decompressed.len(),
            mismatch_idx,
            mismatch_idx,
            &data[start..end_orig],
            &decompressed[start..end_dec]
        );
    }
}

#[test]
fn test_brotlig_exact_gui_problem_chunks() {
    if !brotlig::is_brotlig_available() {
        println!("[SKIP] Brotli-G SDK not available.");
        return;
    }

    // 1. Exact pattern from log: Chunk 86712CE1EFF03563 (Non-64KB size: 61076 bytes)
    let mut chunk_61076 = vec![0u8; 61076];
    for i in 0..66 {
        chunk_61076[i] = (i * 3 + 7) as u8;
    }
    // Repeating 1.0f (0x3F800000) floats across byte 66..120
    for i in 66..120 {
        let pattern = [0x80, 0x3F, 0x00, 0x00];
        chunk_61076[i] = pattern[(i - 66) % 4];
    }
    for i in 120..61076 {
        chunk_61076[i] = ((i * 17) ^ (i >> 3)) as u8;
    }

    verify_roundtrip("Log Chunk 61076 bytes", &chunk_61076, 9);
    verify_roundtrip("Log Chunk 61076 bytes", &chunk_61076, 11);

    // 2. Exact pattern from log: Chunk 1F94923A26ECE8ED (Sparse 64KB with terminal tail at 56311)
    let mut chunk_sparse = vec![0u8; 65536];
    for i in 0..56310 {
        chunk_sparse[i] = if i % 128 < 4 { (i & 0xFF) as u8 } else { 0 };
    }
    chunk_sparse[56311] = 0x24;
    chunk_sparse[56312] = 0xFF;
    chunk_sparse[56313] = 0xFF;
    chunk_sparse[56314] = 0xFF;
    chunk_sparse[56315] = 0xFF;

    verify_roundtrip("Log Chunk Sparse 56311 tail", &chunk_sparse, 9);
    verify_roundtrip("Log Chunk Sparse 56311 tail", &chunk_sparse, 11);

    // 3. BC1/BC3 Texture Block pattern (Periodic 3-byte and 4-byte endpoints)
    let mut bc1_pattern = vec![0u8; 65536];
    for block in 0..(65536 / 8) {
        let base = block * 8;
        // 5:6:5 endpoints (4 bytes)
        bc1_pattern[base] = 0x49;
        bc1_pattern[base + 1] = 0x4A;
        bc1_pattern[base + 2] = 0xC7;
        bc1_pattern[base + 3] = 0x39;
        // 2-bit indices (4 bytes)
        bc1_pattern[base + 4] = 0x92;
        bc1_pattern[base + 5] = 0x24;
        bc1_pattern[base + 6] = 0x49;
        bc1_pattern[base + 7] = 0x92;
    }

    for level in [1, 5, 9, 11] {
        verify_roundtrip("BC1 Structured 64KB Tile", &bc1_pattern, level);
    }
}

#[test]
fn test_brotlig_arbitrary_unaligned_sizes() {
    if !brotlig::is_brotlig_available() {
        return;
    }

    // Test a wide range of odd, non-page-aligned sizes
    let test_sizes = [
        1, 2, 3, 7, 15, 31, 64, 127, 255, 513, 1024, 4095, 8191, 16383, 32767, 49151, 61076, 65535,
        65536, 65537, 100000, 131071, 131072, 131073, 200000, 262143, 262144,
    ];

    for &size in &test_sizes {
        let mut data = vec![0u8; size];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = ((i * 37 + 11) ^ (i >> 2)) as u8;
        }

        verify_roundtrip(&format!("Arbitrary Size {}B", size), &data, 9);
    }
}

#[test]
fn test_brotlig_fuzz_multithreaded_matrix() {
    if !brotlig::is_brotlig_available() {
        return;
    }

    // 128 parallel randomized texture tiles with various entropy profiles
    (0..128).into_par_iter().for_each(|seed| {
        let size = 65536;
        let mut buffer = vec![0u8; size];

        let mode = seed % 4;
        match mode {
            0 => {
                // Highly repetitive runs (LZ77 self-overlapping)
                for (i, b) in buffer.iter_mut().enumerate() {
                    *b = ((i / 3) % 7) as u8;
                }
            }
            1 => {
                // BC5 normal map gradient
                for (i, b) in buffer.iter_mut().enumerate() {
                    let u = ((i % 256) as f64 - 128.0) / 128.0;
                    let v = ((i / 256) as f64 - 128.0) / 128.0;
                    *b = (((u * u + v * v).sqrt() * 127.0) as u8) ^ (seed as u8);
                }
            }
            2 => {
                // Interleaved sparse zeroes and endpoints
                for (i, b) in buffer.iter_mut().enumerate() {
                    if i % 16 < 4 {
                        *b = (i ^ seed) as u8;
                    } else {
                        *b = 0;
                    }
                }
            }
            _ => {
                // Random high entropy noise
                for (i, b) in buffer.iter_mut().enumerate() {
                    *b = ((i * 1337 + seed * 97) ^ (i >> 4)) as u8;
                }
            }
        }

        verify_roundtrip(
            &format!("Fuzz Tile (Seed {}, Mode {})", seed, mode),
            &buffer,
            9,
        );
    });
}
