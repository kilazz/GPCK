// crates/gpck_core/tests/test_sampler_feedback.rs
//! # Sampler Feedback Map & Visible Tile Resolver Integration Tests

use gpck_core::gpu::directstorage::QueuePriority;
use gpck_core::gpu::sampler_feedback::{
    FeedbackMapConfig, FeedbackRegionDimensions, SamplerFeedbackAnalyzer,
};
use gpck_core::gpu::tile_pool::TilePoolManager;
use gpck_core::graphics::dxgi_format::dxgi;
use uuid::Uuid;

#[test]
fn test_sampler_feedback_parsing_and_request_generation() {
    let asset_id = Uuid::new_v4();
    let width = 2048u32;
    let height = 2048u32;
    let mip_levels = 12u32;
    let dxgi_format = dxgi::BC7_UNORM;

    let region = FeedbackRegionDimensions {
        width: 16,
        height: 16,
    };
    let config = FeedbackMapConfig::new(width, height, mip_levels, dxgi_format, region);

    assert_eq!(config.feedback_width, 128);
    assert_eq!(config.feedback_height, 128);
    assert_eq!(config.feedback_byte_size(), 16384);

    // Simulate Feedback Map: mostly unsampled (0xFF), with a few actively sampled regions
    let mut feedback_bytes = vec![0xFFu8; config.feedback_byte_size()];

    // Region (0, 0) requested Mip 0
    feedback_bytes[0] = 0;

    // Region (16, 16) requested Mip 1
    feedback_bytes[16 * 128 + 16] = 1;

    let mut tile_pool = TilePoolManager::new(64 * 65536, None); // 64-tile pool budget
    let dummy_resource_ptr = std::ptr::NonNull::dangling().as_ptr();

    // First Pass: generate requests for sampled regions
    let requests = SamplerFeedbackAnalyzer::extract_missing_tiles(
        &feedback_bytes,
        &config,
        asset_id,
        dummy_resource_ptr,
        &mut tile_pool,
        QueuePriority::High,
    );

    assert!(!requests.is_empty());
    assert!(
        requests
            .iter()
            .any(|r| r.subresource == 0 && r.tile_x == 0 && r.tile_y == 0)
    );
    assert!(requests.iter().any(|r| r.subresource == 1));

    // Second Pass with same feedback: since tiles are now resident, 0 new requests should be emitted
    let requests_second_pass = SamplerFeedbackAnalyzer::extract_missing_tiles(
        &feedback_bytes,
        &config,
        asset_id,
        dummy_resource_ptr,
        &mut tile_pool,
        QueuePriority::High,
    );

    assert_eq!(requests_second_pass.len(), 0);
}
