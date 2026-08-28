// crates/gpck_core/tests/test_corrupted_payloads.rs
//! # Codec Robustness, Fuzzing & Corrupted Payload Resilience Tests

use gpck_core::compression::codecs::{Codec, CompressionMethod};
use gpck_core::compression::{brotlig, gdeflate};

#[test]
fn test_corrupted_bitstreams_graceful_failure() {
    let methods = [
        CompressionMethod::Lz4,
        CompressionMethod::Zstd,
        CompressionMethod::Rans,
        CompressionMethod::GDeflate,
        CompressionMethod::BrotliG,
    ];

    let original_size = 64 * 1024;
    let valid_data: Vec<u8> = (0..original_size)
        .map(|i| ((i as f64 * 0.05).sin() * 120.0 + 128.0) as u8)
        .collect();

    for &method in &methods {
        if method == CompressionMethod::GDeflate && !gdeflate::is_gdeflate_available() {
            continue;
        }
        if method == CompressionMethod::BrotliG && !brotlig::is_brotlig_available() {
            continue;
        }

        let compressed = Codec::compress(&valid_data, method, 5, false).unwrap();

        // Truncated stream test (must handle safely without crashing or memory overrun)
        let truncated = &compressed[..compressed.len() / 2];
        let trunc_res = Codec::decompress(truncated, original_size, method);
        match trunc_res {
            Ok(decomp) => {
                assert!(
                    decomp.len() <= original_size,
                    "Decompressed size exceeded original target size for {:?}",
                    method
                );
            }
            Err(_) => {
                // Decompression error is the standard expected response
            }
        }

        // Bit-flipped corrupted payload (must handle safely without panicking)
        if compressed.len() > 16 {
            let mut corrupted = compressed.clone();
            for offset in [4, 8, 12, corrupted.len() - 4] {
                corrupted[offset] ^= 0xFF;
            }
            let corrupt_res = Codec::decompress(&corrupted, original_size, method);
            match corrupt_res {
                Ok(decomp) => {
                    assert!(
                        decomp.len() <= original_size,
                        "Decompressed size exceeded original target size for {:?}",
                        method
                    );
                }
                Err(_) => {
                    // Expected error
                }
            }
        }

        // Garbage / Pseudo-random payload (must not crash)
        let mut noise = vec![0u8; 1024];
        for (i, b) in noise.iter_mut().enumerate() {
            *b = ((i * 199 + 17) & 0xFF) as u8;
        }
        let noise_res = Codec::decompress(&noise, original_size, method);
        match noise_res {
            Ok(decomp) => {
                assert!(decomp.len() <= original_size);
            }
            Err(_) => {
                // Expected error
            }
        }
    }
}
