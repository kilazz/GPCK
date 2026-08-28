// crates/gpck_core/tests/test_vfs_end_to_end.rs
//! # End-to-End VFS Package Assembly, Encryption & Async Streaming Tests

use gpck_core::compression::codecs::CompressionMethod;
use gpck_core::crypto::aes_gcm::derive_key;
use gpck_core::format::archive::TAG_BASE_GAME;
use gpck_core::io::vfs::VirtualFileSystem;
use gpck_core::packer::{AssetPacker, GaclFormatOverrides, PackerOptions};
use std::collections::HashMap;
use std::fs;

#[tokio::test]
async fn test_vfs_package_encrypt_mount_and_stream() {
    let temp_dir = std::env::temp_dir().join("gpck_e2e_test_src");
    let out_gtoc = std::env::temp_dir().join("gpck_e2e_test.gtoc");

    if temp_dir.exists() {
        let _ = fs::remove_dir_all(&temp_dir);
    }
    fs::create_dir_all(&temp_dir).unwrap();

    let passphrase = "GPCK_Super_Secure_Passphrase_2026";
    let key_bytes = derive_key(passphrase);

    // 1. Create simulated game assets
    let files_to_create = [
        ("configs/game_settings.json", vec![0x30u8; 16 * 1024]),
        ("audio/ambient_wind.wav", vec![0x7Fu8; 128 * 1024]),
        ("levels/world_sector_01.dat", vec![0xAAu8; 512 * 1024]),
    ];

    let mut file_hashes = HashMap::new();

    for (rel_path, data) in &files_to_create {
        let p = temp_dir.join(rel_path);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, data).unwrap();
        let hash = twox_hash::XxHash64::oneshot(0, data);
        file_hashes.insert(rel_path.to_string(), hash);
    }

    // 2. Discover and Pack Archive with AES-256-GCM encryption
    let file_map = AssetPacker::build_file_map(&temp_dir).unwrap();
    assert_eq!(file_map.len(), 3);

    let options = PackerOptions {
        method: CompressionMethod::Zstd,
        level: 9,
        chunk_size: 64 * 1024,
        enable_dedup: true,
        key: Some(key_bytes),
        mip_split: false,
        max_tail_dim: 128,
        tags: TAG_BASE_GAME,
        validate_chunks: true,
        max_partition_size: 64 * 1024 * 1024,
        gacl: GaclFormatOverrides::default(),
        atg_profile: true,
        tiled_streaming: false,
        min_tiled_resolution: 0,
        min_tiled_tile_count: 0,
    };

    AssetPacker::compress_files_to_archive(&file_map, &out_gtoc, &options, |_| {}).unwrap();

    // 3. Mount in Virtual File System using decryption key
    let mut vfs = VirtualFileSystem::new();
    vfs.mount_archive_with_key(&out_gtoc, Some(key_bytes))
        .expect("Failed to mount encrypted archive in VFS");

    // 4. Verify all files can be read synchronously and asynchronously with bit-exact hash
    for (rel_path, expected_data) in &files_to_create {
        let sync_data = vfs.read_file(rel_path).expect("Sync read failed");
        assert_eq!(sync_data, *expected_data);

        let async_data = vfs
            .read_file_async(rel_path)
            .await
            .expect("Async read failed");
        assert_eq!(async_data, *expected_data);
    }

    // Cleanup
    let _ = fs::remove_dir_all(&temp_dir);
    let _ = fs::remove_file(&out_gtoc);
    let _ = fs::remove_file(out_gtoc.with_extension("gdat"));
}
