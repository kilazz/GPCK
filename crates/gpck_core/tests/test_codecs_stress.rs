// crates/gpck_core/tests/test_codecs_stress.rs
//! # Comprehensive Multi-Codec Stress & Data Integrity Test Suite

use gpck_core::compression::codecs::{Codec, CompressionMethod};
use gpck_core::compression::{brotlig, gdeflate};

fn generate_test_pattern(size: usize, pattern_type: usize) -> Vec<u8> {
    let mut data = vec![0u8; size];
    match pattern_type {
        // 1. Smooth sinusoidal gradient (Textures / Audio)
        0 => {
            for (i, b) in data.iter_mut().enumerate() {
                *b = ((i as f64 * 0.05).sin() * 120.0 + 128.0) as u8;
            }
        }
        // 2. Repetitive structure with periodic tokens (JSON / Mesh indices)
        1 => {
            let token = b"{\"transform\":[1.0,0.0,0.0],\"id\":12345},";
            for (i, b) in data.iter_mut().enumerate() {
                *b = token[i % token.len()];
            }
        }
        // 3. Sparse zero-heavy buffer (Masks / Lightmaps)
        2 => {
            for (i, b) in data.iter_mut().enumerate() {
                *b = if i % 64 == 0 { (i & 0xFF) as u8 } else { 0 };
            }
        }
        // 4. Stepped high-frequency ramp
        _ => {
            for (i, b) in data.iter_mut().enumerate() {
                *b = ((i / 16) & 0xFF) as u8;
            }
        }
    }
    data
}

#[test]
fn test_all_codecs_multi_size_roundtrip() {
    let methods = [
        ("Store", CompressionMethod::Store, 0, false),
        ("LZ4", CompressionMethod::Lz4, 9, false),
        ("Zstd_ATG", CompressionMethod::Zstd, 9, true),
        ("Zstd_Std", CompressionMethod::Zstd, 19, false),
        ("rANS", CompressionMethod::Rans, 1, false),
        ("GDeflate", CompressionMethod::GDeflate, 9, false),
        ("BrotliG", CompressionMethod::BrotliG, 5, false),
    ];

    let test_sizes = [4 * 1024, 64 * 1024, 1024 * 1024, 4 * 1024 * 1024];

    for &(name, method, level, atg) in &methods {
        if method == CompressionMethod::GDeflate && !gdeflate::is_gdeflate_available() {
            println!("[SKIP] GDeflate native library not compiled.");
            continue;
        }
        if method == CompressionMethod::BrotliG && !brotlig::is_brotlig_available() {
            println!("[SKIP] AMD Brotli-G SDK not compiled. Ensure git submodules are updated.");
            continue;
        }

        for &size in &test_sizes {
            for pattern_id in 0..4 {
                let original = generate_test_pattern(size, pattern_id);

                let compressed =
                    Codec::compress(&original, method, level, atg).unwrap_or_else(|e| {
                        panic!("Compression failed for {} (size {}): {}", name, size, e)
                    });

                assert!(!compressed.is_empty(), "Empty output for {}", name);

                let decompressed = Codec::decompress(&compressed, original.len(), method)
                    .unwrap_or_else(|e| {
                        panic!("Decompression failed for {} (size {}): {}", name, size, e)
                    });

                assert_eq!(
                    decompressed.len(),
                    original.len(),
                    "Size mismatch for {} on pattern {}",
                    name,
                    pattern_id
                );
                assert_eq!(
                    decompressed, original,
                    "Byte-for-byte mismatch for {} (size {}, pattern {})",
                    name, size, pattern_id
                );
            }
        }
    }
}
