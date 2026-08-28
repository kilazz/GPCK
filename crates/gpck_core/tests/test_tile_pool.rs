// crates/gpck_core/tests/test_tile_pool.rs
//! # Tile Pool Manager & LRU Residency Cache Integration Tests

use gpck_core::gpu::tile_pool::{TileKey, TilePoolManager};
use uuid::Uuid;

#[test]
fn test_tile_pool_allocation_and_lru_eviction() {
    let asset_id = Uuid::new_v4();
    let capacity_bytes = 4 * 65536; // Pool capacity: exactly 4 physical tiles (256 KB)
    let mut pool = TilePoolManager::new(capacity_bytes, None);

    assert_eq!(pool.stats(), (0, 4, 4));

    // Allocate 4 tiles (fill pool to 100%)
    let key0 = TileKey::new(asset_id, 0, 0, 0);
    let key1 = TileKey::new(asset_id, 0, 1, 0);
    let key2 = TileKey::new(asset_id, 0, 0, 1);
    let key3 = TileKey::new(asset_id, 0, 1, 1);

    let plan1 = pool.allocate_tiles(&[key0, key1, key2, key3]);
    assert_eq!(plan1.newly_mapped.len(), 4);
    assert_eq!(plan1.evicted.len(), 0);
    assert_eq!(pool.stats(), (4, 0, 4));

    assert!(pool.is_tile_resident(&key0));
    assert!(pool.is_tile_resident(&key1));
    assert!(pool.is_tile_resident(&key2));
    assert!(pool.is_tile_resident(&key3));

    // Touch key0 to make it recently used: LRU order becomes [key1, key2, key3, key0]
    pool.touch_tile(&key0);

    // Allocate a 5th tile -> must evict the oldest tile (key1)
    let key4 = TileKey::new(asset_id, 1, 0, 0);
    let plan2 = pool.allocate_tiles(&[key4]);

    assert_eq!(plan2.newly_mapped.len(), 1);
    assert_eq!(plan2.evicted.len(), 1);
    assert_eq!(plan2.evicted[0].0, key1); // key1 was evicted!

    assert!(!pool.is_tile_resident(&key1)); // key1 is no longer resident
    assert!(pool.is_tile_resident(&key0)); // key0 was saved by touch_tile!
    assert!(pool.is_tile_resident(&key4)); // key4 is now resident

    // Test Pool Reset / Clear
    pool.clear();
    assert_eq!(pool.stats(), (0, 4, 4));
    assert!(!pool.is_tile_resident(&key0));
    assert!(!pool.is_tile_resident(&key4));
}
