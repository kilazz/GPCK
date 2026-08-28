// crates/gpck_core/tests/test_tile_validation_debug.rs
//! # Deep Byte-Level Diagnostic Test for Texture Tile Compression & Validation

use gpck_core::benchmark::generators::{
    generate_realistic_bc1_texture, generate_realistic_bc5_texture,
    generate_realistic_bc7_orm_texture,
};
use gpck_core::compression::codecs::{Codec, CompressionMethod};
use gpck_core::graphics::dxgi_format::{D3D12FormatTable, dxgi};
use gpck_core::graphics::recombine::TextureRecombiner;
use gpck_core::packer::PackerOptions;
use gpck_core::packer::tiler::{D3D12_TILE_SIZE, TiledTexturePacker};

#[test]
fn test_ultra_compressible_flat_and_zero_tiles() {
    println!("\n=== Testing Ultra-Compressible 64KB Flat/Zero Tiles (<16 bytes compressed) ===");

    let test_cases: Vec<(&str, Vec<u8>)> = vec![
        ("All Zeroes (Empty Tile / Padding)", vec![0u8; 65536]),
        ("Flat Specular Black (0x01)", vec![1u8; 65536]),
        ("Flat Normal Map Midpoint (0x80)", vec![128u8; 65536]),
    ];

    for (name, data) in test_cases {
        println!("Testing tile: {}", name);
        for &method in &[
            CompressionMethod::GDeflate,
            CompressionMethod::Zstd,
            CompressionMethod::BrotliG,
            CompressionMethod::Lz4,
        ] {
            let compressed = Codec::compress(&data, method, 9, false).unwrap_or_else(|e| {
                panic!("Compression failed on '{}' for {:?}: {}", name, method, e)
            });

            println!(
                "  [{:?}] Compressed 65536 bytes -> {} bytes",
                method,
                compressed.len()
            );

            let decompressed =
                Codec::decompress(&compressed, data.len(), method).unwrap_or_else(|e| {
                    panic!("Decompression failed on '{}' for {:?}: {}", name, method, e)
                });

            assert_eq!(
                decompressed.len(),
                data.len(),
                "Length mismatch on '{}' for {:?}",
                name,
                method
            );
            assert_eq!(
                decompressed, data,
                "Bit-exact mismatch on '{}' for {:?}",
                name, method
            );
            println!("  [{:?}] Bit-exact 100% MATCH!", method);
        }
        println!();
    }
}

#[test]
fn test_diagnose_all_codecs_on_crysis_texture_patterns() {
    println!("\n================================================================================");
    println!(" GPCK Tile Validation Deep Diagnostic Runner (Realistic Compressible PBR Data)");
    println!("================================================================================\n");

    let test_textures = [
        (
            "helmet_damaged_diff (2048x2048 BC7)",
            2048u32,
            2048u32,
            dxgi::BC7_UNORM,
            12u32,
            10,
        ),
        (
            "hand_ddna (2048x2048 BC5)",
            2048u32,
            2048u32,
            dxgi::BC5_UNORM,
            12u32,
            4,
        ),
        (
            "collar_ddn (1024x1024 BC5)",
            1024u32,
            1024u32,
            dxgi::BC5_UNORM,
            11u32,
            4,
        ),
    ];

    let codecs = [
        ("GDeflate (Level 9)", CompressionMethod::GDeflate, 9),
        ("Zstandard ATG (Level 9)", CompressionMethod::Zstd, 9),
        ("Brotli-G (Level 11)", CompressionMethod::BrotliG, 11),
        ("LZ4 (Level 9)", CompressionMethod::Lz4, 9),
    ];

    for &(tex_name, width, height, dxgi_fmt, mips, gacl_id) in &test_textures {
        println!(
            "--------------------------------------------------------------------------------"
        );
        println!(
            "Inspecting Texture: {} | Format DXGI: {}",
            tex_name, dxgi_fmt
        );
        println!(
            "--------------------------------------------------------------------------------"
        );

        let mip0_bytes = match dxgi_fmt {
            dxgi::BC7_UNORM => generate_realistic_bc7_orm_texture(width as usize, height as usize),
            dxgi::BC5_UNORM => generate_realistic_bc5_texture(width as usize, height as usize),
            _ => generate_realistic_bc1_texture(width as usize, height as usize),
        };

        let element_size = D3D12FormatTable::get_element_size(dxgi_fmt).unwrap_or(16);
        let mut full_mip_chain_bytes = mip0_bytes;

        for m in 1..mips {
            let (mw, mh, _) = D3D12FormatTable::get_mip_dimensions(m, width, height, 1);
            let mip_bytes_len =
                (mw.div_ceil(4) as usize) * (mh.div_ceil(4) as usize) * element_size;

            let mip_slice = match dxgi_fmt {
                dxgi::BC7_UNORM => generate_realistic_bc7_orm_texture(mw as usize, mh as usize),
                dxgi::BC5_UNORM => generate_realistic_bc5_texture(mw as usize, mh as usize),
                _ => generate_realistic_bc1_texture(mw as usize, mh as usize),
            };

            if mip_slice.len() >= mip_bytes_len {
                full_mip_chain_bytes.extend_from_slice(&mip_slice[..mip_bytes_len]);
            } else {
                full_mip_chain_bytes.extend_from_slice(&vec![0x40u8; mip_bytes_len]);
            }
        }

        let dds_data =
            TextureRecombiner::wrap_in_dds_header(width, height, gacl_id, &full_mip_chain_bytes);

        for &(codec_name, method, level) in &codecs {
            println!("  [Codec: {}]", codec_name);

            let options = PackerOptions {
                method,
                level,
                tiled_streaming: true,
                validate_chunks: true,
                atg_profile: true,
                ..Default::default()
            };

            let tile_res = TiledTexturePacker::slice_and_compress_texture_tiles(
                &dds_data, 148, dxgi_fmt, width, height, mips, &options,
            );

            match tile_res {
                Ok(res) => {
                    println!(
                        "    Total Tiles Generated: {} (Standard Mips: {}, Packed Mips: {})",
                        res.total_tiles, res.num_standard_mips, res.num_packed_mips
                    );
                    println!("    GACL Uniform Transform: {:?}", res.gacl_transform);

                    let mut failed_tiles = 0;
                    // Combine standard mip tiles and packed tail tiles for validation
                    for (tile_idx, chunk) in res
                        .standard_chunks
                        .iter()
                        .chain(&res.tail_chunks)
                        .enumerate()
                    {
                        let decompressed = Codec::decompress(&chunk.data, D3D12_TILE_SIZE, method);
                        match decompressed {
                            Ok(data) => {
                                let hash_decomp = twox_hash::XxHash64::oneshot(0, &data);
                                if hash_decomp != chunk.hash {
                                    failed_tiles += 1;
                                    println!(
                                        "    [FAIL] Tile #{} (Chunk Size: {} -> {} B, Hash: {:016X})",
                                        tile_idx,
                                        chunk.original_size,
                                        chunk.compressed_size,
                                        chunk.hash
                                    );
                                }
                            }
                            Err(e) => {
                                failed_tiles += 1;
                                println!(
                                    "    [ERROR] Tile #{} decompression failed: {}",
                                    tile_idx, e
                                );
                            }
                        }
                    }

                    if failed_tiles == 0 {
                        println!(
                            "    >>> SUCCESS: All {} tiles passed validation for {}! <<<\n",
                            res.total_tiles, codec_name
                        );
                    } else {
                        println!(
                            "    >>> FAILED: {}/{} tiles corrupted for {}! <<<\n",
                            failed_tiles, res.total_tiles, codec_name
                        );
                    }
                }
                Err(e) => {
                    println!("    [FATAL] Slicing and compression failed: {}\n", e);
                }
            }
        }
    }
}
