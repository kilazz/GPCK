// crates/gpck_core/tests/test_brotlig.rs
use gpck_core::compression::brotlig;
use gpck_core::compression::codecs::{Codec, CompressionMethod};

#[test]
fn test_brotlig_header_decompressed_size() {
    if !brotlig::is_brotlig_available() {
        println!("AMD Brotli-G SDK not available. Skipping test.");
        return;
    }

    let raw_data = vec![0x77u8; 128 * 1024];
    let compressed = brotlig::compress(&raw_data, 5).unwrap();

    if let Some(decomp_size) = brotlig::get_decompressed_size(&compressed) {
        assert_eq!(decomp_size, raw_data.len());
    }
}

#[test]
fn test_brotlig_roundtrip_256kb_stream() {
    if !brotlig::is_brotlig_available() {
        println!("AMD Brotli-G SDK not available. Skipping test.");
        return;
    }

    let size = 256 * 1024; // 256 KB payload (4 x 64KB pages)
    let raw_data: Vec<u8> = (0..size)
        .map(|i| ((i as f64 * 0.05).sin() * 120.0 + 128.0) as u8)
        .collect();

    let compressed = Codec::compress(&raw_data, CompressionMethod::BrotliG, 5, false).unwrap();
    assert!(!compressed.is_empty());
    assert!(compressed.len() < raw_data.len());

    let decompressed =
        Codec::decompress(&compressed, raw_data.len(), CompressionMethod::BrotliG).unwrap();
    assert_eq!(decompressed.len(), raw_data.len());
    assert_eq!(decompressed, raw_data);
}

#[test]
fn test_brotlig_packed_mip_tail_64kb() {
    if !brotlig::is_brotlig_available() {
        return;
    }

    let mut tail_tile = vec![0u8; 65536];
    for i in 0..21504 {
        tail_tile[i] = ((i * 37 + 13) ^ (i >> 2)) as u8;
    }
    for i in 21504..65536 {
        tail_tile[i] = ((i - 21504) & 0xFF) as u8;
    }

    let compressed = brotlig::compress(&tail_tile, 9).unwrap();
    assert!(!compressed.is_empty());
    println!(
        "Compressed 64KB tail: {} -> {} bytes (Ratio: {:.1}%)",
        tail_tile.len(),
        compressed.len(),
        (compressed.len() as f64 / tail_tile.len() as f64) * 100.0
    );

    let decompressed = brotlig::decompress(&compressed, tail_tile.len()).unwrap();

    if decompressed != tail_tile {
        for i in 0..tail_tile.len().min(decompressed.len()) {
            if decompressed[i] != tail_tile[i] {
                println!(
                    "\n[MISMATCH AT BYTE {} (0x{:04X})]:\n\
                     Expected: 0x{:02X}\n\
                     Got:      0x{:02X}\n\
                     Expected slice: {:02X?}\n\
                     Decomp slice:   {:02X?}\n",
                    i,
                    i,
                    tail_tile[i],
                    decompressed[i],
                    &tail_tile[i.saturating_sub(4)..std::cmp::min(i + 16, tail_tile.len())],
                    &decompressed[i.saturating_sub(4)..std::cmp::min(i + 16, decompressed.len())]
                );
                break;
            }
        }
    }

    assert_eq!(decompressed.len(), tail_tile.len());
    assert_eq!(decompressed, tail_tile);
}

#[test]
fn test_brotlig_extreme_rle_64kb() {
    if !brotlig::is_brotlig_available() {
        return;
    }

    let raw_data = vec![0xABu8; 65536];
    let compressed = brotlig::compress(&raw_data, 11).unwrap();
    let decompressed = brotlig::decompress(&compressed, raw_data.len()).unwrap();
    assert_eq!(decompressed.len(), raw_data.len());
    assert_eq!(decompressed, raw_data);
}
