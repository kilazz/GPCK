// crates/gpck_core/src/gacl/bc7.rs
//! # Bit-Exact BC7 Mode-Split, Mode-Join & Color Conditioning
//!
//! Implements Microsoft ATG GACL byte-aligned stream partitioning and recombining
//! for all BC7 modes (Transform 10 and 11) matching DirectStorage UnshuffleBC7.hlsl.

use crate::core::error::GpckResult;

const BLOCK_BYTES: usize = 16;

/// Transform 10: BC7 Mode Split (1-byte mode, 8-byte color endpoints, 7-byte indices)
pub fn bc7_mode_split_transform(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    let total_blocks = src.len() / BLOCK_BYTES;
    if total_blocks == 0 {
        return Ok(());
    }

    let (mode_stream, rest) = dst.split_at_mut(total_blocks);
    let (color_stream, index_stream) = rest.split_at_mut(total_blocks * 8);

    for (i, src_chunk) in src.chunks_exact(BLOCK_BYTES).enumerate() {
        mode_stream[i] = src_chunk[0];
        color_stream[i * 8..(i + 1) * 8].copy_from_slice(&src_chunk[1..9]);
        index_stream[i * 7..(i + 1) * 7].copy_from_slice(&src_chunk[9..16]);
    }

    Ok(())
}

/// Reverse of Transform 10: Reconstructs original 16-byte BC7 blocks from 3 streams
pub fn bc7_mode_split_reverse(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    let total_blocks = dst.len() / BLOCK_BYTES;
    if total_blocks == 0 {
        return Ok(());
    }

    let (mode_stream, rest) = src.split_at(total_blocks);
    let (color_stream, index_stream) = rest.split_at(total_blocks * 8);

    for (i, dst_chunk) in dst.chunks_exact_mut(BLOCK_BYTES).enumerate() {
        dst_chunk[0] = mode_stream[i];
        dst_chunk[1..9].copy_from_slice(&color_stream[i * 8..(i + 1) * 8]);
        dst_chunk[9..16].copy_from_slice(&index_stream[i * 7..(i + 1) * 7]);
    }

    Ok(())
}

/// Transform 11: BC7 Mode Join (12-byte endpoints + 4-byte indices)
pub fn bc7_mode_join_transform(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    let total_blocks = src.len() / BLOCK_BYTES;
    if total_blocks == 0 {
        return Ok(());
    }

    let (endpoints_stream, index_stream) = dst.split_at_mut(total_blocks * 12);

    for (i, src_chunk) in src.chunks_exact(BLOCK_BYTES).enumerate() {
        endpoints_stream[i * 12..(i + 1) * 12].copy_from_slice(&src_chunk[0..12]);
        index_stream[i * 4..(i + 1) * 4].copy_from_slice(&src_chunk[12..16]);
    }

    Ok(())
}

/// Reverse of Transform 11: Reconstructs original 16-byte BC7 blocks from 2 streams
pub fn bc7_mode_join_reverse(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    let total_blocks = dst.len() / BLOCK_BYTES;
    if total_blocks == 0 {
        return Ok(());
    }

    let (endpoints_stream, index_stream) = src.split_at(total_blocks * 12);

    for (i, dst_chunk) in dst.chunks_exact_mut(BLOCK_BYTES).enumerate() {
        dst_chunk[0..12].copy_from_slice(&endpoints_stream[i * 12..(i + 1) * 12]);
        dst_chunk[12..16].copy_from_slice(&index_stream[i * 4..(i + 1) * 4]);
    }

    Ok(())
}
