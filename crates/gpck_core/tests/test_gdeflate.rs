// crates/gpck_core/tests/test_gdeflate.rs
//! # Multithreaded GDeflate Stress & Roundtrip Verification Tests

use gpck_core::compression::codecs::{Codec, CompressionMethod};
use gpck_core::compression::gdeflate::is_gdeflate_available;
use rayon::prelude::*;

#[test]
fn test_gdeflate_multithreaded_stress() {
    println!("Is GDeflate Available: {}", is_gdeflate_available());

    if !is_gdeflate_available() {
        println!("GDeflate native library not available. Skipping test.");
        return;
    }

    (0..100).into_par_iter().for_each(|i| {
        let size = 256 * 1024; // 256 KB
        let mut input = vec![0u8; size];

        // Generate compressible structured wave patterns with per-thread phase shifts
        for (k, byte) in input.iter_mut().enumerate() {
            let pattern = ((k as f64 + i as f64 * 17.0) * 0.03).sin() * 120.0 + 128.0;
            *byte = pattern as u8;
        }

        // Compression
        let compressed = Codec::compress(&input, CompressionMethod::GDeflate, 9, false)
            .expect("GDeflate compression failed");
        assert!(!compressed.is_empty(), "Compressed output is empty!");
        assert!(
            compressed.len() < input.len(),
            "Compressed size ({} B) must be smaller than raw input ({} B)",
            compressed.len(),
            input.len()
        );

        // Decompression & Byte-for-Byte Verification
        let decompressed = Codec::decompress(&compressed, input.len(), CompressionMethod::GDeflate)
            .expect("GDeflate decompression failed");
        assert_eq!(
            decompressed.len(),
            input.len(),
            "Decompressed size mismatch"
        );
        assert_eq!(
            decompressed, input,
            "Decompressed payload must match original input byte-for-byte"
        );
    });

    println!("GDeflate multithreaded stress test completed successfully!");
}
