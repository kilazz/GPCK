// crates/gpck_core/tests/test_tiled_texture.rs
//! # 64KB Sparse Tile Slicing & Packaging Integration Tests

use gpck_core::compression::codecs::{Codec, CompressionMethod};
use gpck_core::format::archive::TYPE_TILED_RESOURCE;
use gpck_core::graphics::dxgi_format::dxgi;
use gpck_core::graphics::recombine::TextureRecombiner;
use gpck_core::packer::PackerOptions;
use gpck_core::packer::texture::process_file;
use gpck_core::packer::tiler::{D3D12_TILE_SIZE, TiledTexturePacker};
use std::fs;

#[test]
fn test_slice_and_reconstruct_64k_tiles() {
    let width = 512u32;
    let height = 512u32;
    let dxgi_fmt = dxgi::BC7_UNORM;

    // BC7 is 16 bytes per 4x4 block -> 512x512 = 128x128 blocks = 262,144 bytes
    let raw_pixels: Vec<u8> = (0..262_144).map(|i| (i % 255) as u8).collect();
    let dds_data = TextureRecombiner::wrap_in_dds_header(width, height, dxgi_fmt, &raw_pixels);

    let options = PackerOptions {
        method: CompressionMethod::Zstd,
        level: 3,
        tiled_streaming: true,
        min_tiled_resolution: 0,
        min_tiled_tile_count: 0,
        ..Default::default()
    };

    let tile_res = TiledTexturePacker::slice_and_compress_texture_tiles(
        &dds_data, 148, // DX10 Header size
        dxgi_fmt, width, height, 1, // 1 Mip level
        &options,
    )
    .unwrap();

    // 512x512 BC7 has 4 tiles of 256x256 (64 KB each)
    assert_eq!(tile_res.total_tiles, 4);
    assert_eq!(
        tile_res.standard_chunks.len() + tile_res.tail_chunks.len(),
        4
    );

    // Verify every compressed tile chunk decompresses back to exactly 64 KB
    for chunk in tile_res.standard_chunks.iter().chain(&tile_res.tail_chunks) {
        assert_eq!(chunk.original_size as usize, D3D12_TILE_SIZE);
        let decompressed =
            Codec::decompress(&chunk.data, D3D12_TILE_SIZE, CompressionMethod::Zstd).unwrap();
        assert_eq!(decompressed.len(), D3D12_TILE_SIZE);
    }
}

#[test]
fn test_tiled_texture_packaging_pipeline() {
    let temp_dir = std::env::temp_dir().join("gpck_tiled_test");
    fs::create_dir_all(&temp_dir).unwrap();

    let width = 512u32;
    let height = 512u32;
    let raw_pixels = vec![0xABu8; 262_144];
    let dds_data =
        TextureRecombiner::wrap_in_dds_header(width, height, dxgi::BC7_UNORM, &raw_pixels);

    let dds_path = temp_dir.join("test_texture_4k.dds");
    fs::write(&dds_path, &dds_data).unwrap();

    let options = PackerOptions {
        method: CompressionMethod::Zstd,
        level: 3,
        tiled_streaming: true,
        min_tiled_resolution: 0,
        min_tiled_tile_count: 0,
        ..Default::default()
    };

    let processed_files =
        process_file(&dds_path, "materials/test_texture_4k.dds", &options).unwrap();
    assert_eq!(processed_files.len(), 1);

    let file = &processed_files[0];
    assert!((file.flags & TYPE_TILED_RESOURCE) != 0);
    assert_eq!(file.alignment, 65536);
    assert_eq!(file.chunks.len(), 4);

    let _ = fs::remove_dir_all(&temp_dir);
}
