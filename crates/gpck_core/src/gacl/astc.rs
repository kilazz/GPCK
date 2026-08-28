// crates/gpck_core/src/gacl/astc.rs
//! # ASTC & ETC2 Mobile Texture Conditioning Scaffold
//!
//! Implements block-level conditioning, Morton 2D spatial curve transposition,
//! and stream partitioning for mobile GPU texture formats (ASTC & ETC2) targeting
//! Vulkan Android Profile 2025 (ARM Mali, Qualcomm Adreno, PowerVR, Apple Silicon).

use super::space_curve::apply_space_curve_internal;
use crate::core::error::{GpckError, GpckResult};

/// ASTC block size in bytes (always 128 bits / 16 bytes for all footprints).
pub const ASTC_BLOCK_SIZE: usize = 16;

/// ETC2 block size in bytes (8 bytes for RGB, 16 bytes for RGBA).
pub const ETC2_RGB_BLOCK_SIZE: usize = 8;
pub const ETC2_RGBA_BLOCK_SIZE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum AstcFootprint {
    #[default]
    Block4x4 = 0,
    Block5x4 = 1,
    Block5x5 = 2,
    Block6x5 = 3,
    Block6x6 = 4,
    Block8x5 = 5,
    Block8x6 = 6,
    Block8x8 = 7,
    Block10x5 = 8,
    Block10x6 = 9,
    Block10x8 = 10,
    Block10x10 = 11,
    Block12x10 = 12,
    Block12x12 = 13,
}

impl AstcFootprint {
    #[inline(always)]
    pub fn dimensions(self) -> (usize, usize) {
        match self {
            Self::Block4x4 => (4, 4),
            Self::Block5x4 => (5, 4),
            Self::Block5x5 => (5, 5),
            Self::Block6x5 => (6, 5),
            Self::Block6x6 => (6, 6),
            Self::Block8x5 => (8, 5),
            Self::Block8x6 => (8, 6),
            Self::Block8x8 => (8, 8),
            Self::Block10x5 => (10, 5),
            Self::Block10x6 => (10, 6),
            Self::Block10x8 => (10, 8),
            Self::Block10x10 => (10, 10),
            Self::Block12x10 => (12, 10),
            Self::Block12x12 => (12, 12),
        }
    }
}

pub struct AstcConditioner;

impl AstcConditioner {
    /// Applies 2D Morton Z-Order spatial curve on ASTC 16-byte blocks.
    pub fn apply_astc_space_curve(
        input: &[u8],
        footprint: AstcFootprint,
        width_pixels: usize,
        forward: bool,
    ) -> GpckResult<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        if !input.len().is_multiple_of(ASTC_BLOCK_SIZE) {
            return Err(GpckError::AstcError(
                "ASTC payload size is not aligned to 16 bytes".to_string(),
            ));
        }

        let (block_w, _) = footprint.dimensions();
        let width_in_blocks = width_pixels.div_ceil(block_w);
        let mut output = vec![0u8; input.len()];

        if apply_space_curve_internal(
            input,
            &mut output,
            ASTC_BLOCK_SIZE,
            width_in_blocks * 4,
            forward,
        ) {
            Ok(output)
        } else {
            Ok(input.to_vec())
        }
    }

    /// Splits ASTC mode header bits from weight and color index partitions.
    pub fn condition_astc_blocks(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
        let total_blocks = src.len() / ASTC_BLOCK_SIZE;
        if total_blocks == 0 {
            return Ok(());
        }

        let (header_stream, payload_stream) = dst.split_at_mut(total_blocks * 2);

        // ASTC Block Mode is stored in lowest 11 bits of word 0
        for (i, block) in src.chunks_exact(ASTC_BLOCK_SIZE).enumerate() {
            header_stream[i * 2..i * 2 + 2].copy_from_slice(&block[0..2]);
            payload_stream[i * 14..(i + 1) * 14].copy_from_slice(&block[2..16]);
        }

        Ok(())
    }

    /// Reconstructs original ASTC blocks from split streams.
    pub fn uncondition_astc_blocks(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
        let total_blocks = dst.len() / ASTC_BLOCK_SIZE;
        if total_blocks == 0 {
            return Ok(());
        }

        let (header_stream, payload_stream) = src.split_at(total_blocks * 2);

        for (i, block) in dst.chunks_exact_mut(ASTC_BLOCK_SIZE).enumerate() {
            block[0..2].copy_from_slice(&header_stream[i * 2..i * 2 + 2]);
            block[2..16].copy_from_slice(&payload_stream[i * 14..(i + 1) * 14]);
        }

        Ok(())
    }
}
