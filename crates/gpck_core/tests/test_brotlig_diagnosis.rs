// crates/gpck_core/tests/test_brotlig_diagnosis.rs
use gpck_core::compression::brotlig;
use gpck_core::compression::codecs::{Codec, CompressionMethod};
use rayon::prelude::*;

#[test]
fn test_brotlig_tiled_patterns_stress() {
    if !brotlig::is_brotlig_available() {
        println!("[SKIP] AMD Brotli-G SDK not available.");
        return;
    }

    let patterns: Vec<(&str, Vec<u8>)> = vec![
        // 1. Boundary tile with structured padding (mimicking partial boundary mipmaps)
        (
            "Interleaved Half-Pattern (Boundary Tile)",
            (0..65536)
                .map(|i| {
                    if (i / 1024) % 2 == 0 {
                        (i % 255) as u8
                    } else {
                        (i & 0xFF) as u8
                    }
                })
                .collect(),
        ),
        // 2. High entropy pseudo-random noise (mimicking BC5/normal maps)
        (
            "BC5 High Frequency Normal Map Noise",
            (0..65536)
                .map(|i| ((i * 1337 + 73) ^ (i >> 3) ^ (i * 31)) as u8)
                .collect(),
        ),
        // 3. Extreme RLE repeating pattern (single byte runs)
        ("Extreme RLE Block Sequence", vec![0xAB; 65536]),
        // 4. Multi-page texture (256 KB spanning across 4 x 64KB pages)
        (
            "Multi-Page 256KB PBR Stream",
            (0..262144)
                .map(|i| ((i as f64 * 0.0125).cos() * 110.0 + 128.0) as u8)
                .collect(),
        ),
        // 5. Sub-page chunk (32 KB tail)
        (
            "Sub-Page 32KB Tail Chunk",
            (0..32768).map(|i| ((i ^ (i >> 4)) + 42) as u8).collect(),
        ),
    ];

    println!("\n=== Starting Exhaustive Brotli-G Matrix Diagnosis Suite ===");

    for (name, data) in patterns {
        for level in [1, 5, 9, 11] {
            println!(
                "Testing Brotli-G at Level {} on pattern '{}'...",
                level, name
            );

            let compressed = Codec::compress(&data, CompressionMethod::BrotliG, level, false)
                .unwrap_or_else(|e| {
                    panic!("Compression failed on '{}' at level {}: {}", name, level, e)
                });

            println!(
                "  Compressed {} -> {} bytes (Ratio: {:.2}%)",
                data.len(),
                compressed.len(),
                (compressed.len() as f64 / data.len() as f64) * 100.0
            );

            assert!(
                !compressed.is_empty(),
                "Empty compressed stream for '{}'",
                name
            );

            let decompressed =
                Codec::decompress(&compressed, data.len(), CompressionMethod::BrotliG)
                    .unwrap_or_else(|e| {
                        panic!(
                            "Decompression failed on '{}' at level {}: {}",
                            name, level, e
                        )
                    });

            if decompressed != data {
                for i in 0..data.len().min(decompressed.len()) {
                    if decompressed[i] != data[i] {
                        println!(
                            "\n[DIAGNOSTIC MISMATCH] First mismatch at byte {} (0x{:04X}):\n\
                             Expected: 0x{:02X}\n\
                             Got:      0x{:02X}\n\
                             Expected around:   {:02X?}\n\
                             Decomp around:     {:02X?}\n",
                            i,
                            i,
                            data[i],
                            decompressed[i],
                            &data[i.saturating_sub(8)..std::cmp::min(i + 16, data.len())],
                            &decompressed
                                [i.saturating_sub(8)..std::cmp::min(i + 16, decompressed.len())]
                        );
                        break;
                    }
                }
            }

            assert_eq!(
                decompressed.len(),
                data.len(),
                "Length mismatch on '{}' at level {}",
                name,
                level
            );
            assert_eq!(
                decompressed, data,
                "Content mismatch on '{}' at level {}",
                name, level
            );
        }
        println!(
            "  -> Pattern '{}' PASSED all compression levels (1..11)!\n",
            name
        );
    }
}

#[test]
fn test_brotlig_multithreaded_rayon_concurrency() {
    if !brotlig::is_brotlig_available() {
        println!("[SKIP] AMD Brotli-G SDK not available.");
        return;
    }

    println!(
        "Testing Brotli-G under heavy multi-threaded Rayon contention (64 concurrent tasks)..."
    );

    (0..64).into_par_iter().for_each(|i| {
        let size = 65536; // 64 KB per tile
        let mut buffer = vec![0u8; size];

        for (k, byte) in buffer.iter_mut().enumerate() {
            let pattern = ((k as f64 + (i as f64 * 31.0)) * 0.04).sin() * 120.0 + 128.0;
            *byte = pattern as u8;
        }

        let compressed = Codec::compress(&buffer, CompressionMethod::BrotliG, 11, false)
            .unwrap_or_else(|e| panic!("Thread {} compression failed: {}", i, e));

        let decompressed = Codec::decompress(&compressed, size, CompressionMethod::BrotliG)
            .unwrap_or_else(|e| panic!("Thread {} decompression failed: {}", i, e));

        assert_eq!(decompressed.len(), size);
        assert_eq!(decompressed, buffer);
    });

    println!("  -> 64/64 Rayon concurrent tasks PASSED without race conditions!\n");
}

#[test]
fn test_packed_tail_tile_decompression_all_codecs() {
    let mut tail_tile = vec![0u8; 65536];
    for i in 0..21504 {
        tail_tile[i] = ((i * 37 + 13) ^ (i >> 2)) as u8;
    }
    for i in 21504..65536 {
        tail_tile[i] = ((i - 21504) & 0xFF) as u8;
    }

    println!("=== Testing 64KB Packed Mip-Tail (21.5KB Data + 42.5KB Structured Padding) ===");

    for &method in &[
        CompressionMethod::Zstd,
        CompressionMethod::GDeflate,
        CompressionMethod::Lz4,
        CompressionMethod::BrotliG,
    ] {
        let compressed = Codec::compress(&tail_tile, method, 9, false)
            .unwrap_or_else(|e| panic!("Compression failed for {:?}: {}", method, e));

        let decompressed = Codec::decompress(&compressed, tail_tile.len(), method)
            .unwrap_or_else(|e| panic!("Decompression failed for {:?}: {}", method, e));

        assert_eq!(
            decompressed.len(),
            tail_tile.len(),
            "Length mismatch for {:?}",
            method
        );
        assert_eq!(
            decompressed, tail_tile,
            "Bit-exact content mismatch for {:?}",
            method
        );

        println!("  -> {:?}: Bit-exact 100% verification PASSED!", method);
    }
    println!();
}
