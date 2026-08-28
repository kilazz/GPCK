// crates/gpck_core/src/gacl/shufflers.rs
//! # BCn Stream Shufflers & Bit Manipulations
//!
//! Bit-exact Rust implementations matching Microsoft ATG DirectStorage HLSL shaders:
//! - UnshuffleBC1x (Transform 1, 17, 32, 33)
//! - UnshuffleBC3x (Transform 2, 18, 34, 35)
//! - UnshuffleBC4x (Transform 3, 19)
//! - UnshuffleBC5x (Transform 4, 20)
//! - UnshuffleBC2  (Transform 6)
//! - UnshuffleBC6H (Transform 7)

use crate::core::error::GpckResult;

// ============================================================================
// Bit Manipulation Helpers (BMI2 / Scalar Fallback)
// ============================================================================

#[inline(always)]
pub(crate) fn pext_u64(val: u64, mask: u64) -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("bmi2") {
            return unsafe { std::arch::x86_64::_pext_u64(val, mask) };
        }
    }
    let mut res = 0u64;
    let mut out_bit = 1u64;
    let mut m = mask;
    while m != 0 {
        let lsb = m & m.wrapping_neg();
        if (val & lsb) != 0 {
            res |= out_bit;
        }
        out_bit <<= 1;
        m &= m - 1;
    }
    res
}

// ============================================================================
// BC1 Shufflers (Micro-pattern 1 & 2)
// ============================================================================

/// Micro-pattern 1 (Transform 1 / 17): Separates (e0, e1, indices)
pub(crate) fn shuffle_bc1(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 8;
    let total_blocks = src.len() / BLOCK_SIZE;
    let num_pairs = total_blocks / 2;
    let shuffle_size = num_pairs * BLOCK_SIZE * 2;

    if shuffle_size > 0 {
        let (d1, rest) = dst[..shuffle_size].split_at_mut(shuffle_size / 4);
        let (d2, d3) = rest.split_at_mut(shuffle_size / 4);

        for (src_block, (c1, (c2, c3))) in src[..shuffle_size].chunks_exact(8).zip(
            d1.chunks_exact_mut(2)
                .zip(d2.chunks_exact_mut(2).zip(d3.chunks_exact_mut(4))),
        ) {
            c1.copy_from_slice(&src_block[0..2]);
            c2.copy_from_slice(&src_block[2..4]);
            c3.copy_from_slice(&src_block[4..8]);
        }
    }

    if !total_blocks.is_multiple_of(2) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

pub(crate) fn unshuffle_bc1(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 8;
    let total_blocks = dst.len() / BLOCK_SIZE;
    let num_pairs = total_blocks / 2;
    let shuffle_size = num_pairs * BLOCK_SIZE * 2;

    if shuffle_size > 0 {
        let d1 = &src[0..shuffle_size / 4];
        let d2 = &src[shuffle_size / 4..shuffle_size / 2];
        let d3 = &src[shuffle_size / 2..shuffle_size];

        for (dst_block, (c1, (c2, c3))) in dst[..shuffle_size].chunks_exact_mut(8).zip(
            d1.chunks_exact(2)
                .zip(d2.chunks_exact(2).zip(d3.chunks_exact(4))),
        ) {
            dst_block[0..2].copy_from_slice(c1);
            dst_block[2..4].copy_from_slice(c2);
            dst_block[4..8].copy_from_slice(c3);
        }
    }

    if !total_blocks.is_multiple_of(2) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

#[inline(always)]
pub(crate) fn pack_bc1_v2_endpoint_pair(e0: u16, e1: u16) -> u32 {
    let e0_r4_1 = ((e0 >> 12) & 0xF) as u32;
    let e0_g5_2 = ((e0 >> 7) & 0xF) as u32;
    let e0_b4_1 = ((e0 >> 1) & 0xF) as u32;
    let e1_r4_1 = ((e1 >> 12) & 0xF) as u32;
    let e1_g5_2 = ((e1 >> 7) & 0xF) as u32;
    let e1_b4_1 = ((e1 >> 1) & 0xF) as u32;

    let e0_r0 = ((e0 >> 11) & 0x1) as u32;
    let e0_g1_0 = ((e0 >> 5) & 0x3) as u32;
    let e0_b0 = (e0 & 0x1) as u32;
    let e1_r0 = ((e1 >> 11) & 0x1) as u32;
    let e1_g1_0 = ((e1 >> 5) & 0x3) as u32;
    let e1_b0 = (e1 & 0x1) as u32;

    (e0_r4_1 << 28)
        | (e0_g5_2 << 24)
        | (e0_b4_1 << 20)
        | (e1_r4_1 << 16)
        | (e1_g5_2 << 12)
        | (e1_b4_1 << 8)
        | (e0_r0 << 7)
        | (e0_g1_0 << 5)
        | (e0_b0 << 4)
        | (e1_r0 << 3)
        | (e1_g1_0 << 1)
        | e1_b0
}

#[inline(always)]
pub(crate) fn unpack_bc1_v2_endpoint_pair(ep: u32) -> (u16, u16) {
    let e0_r = (((ep >> 28) & 0xF) << 1) | ((ep >> 7) & 0x1);
    let e0_g = (((ep >> 24) & 0xF) << 2) | ((ep >> 5) & 0x3);
    let e0_b = (((ep >> 20) & 0xF) << 1) | ((ep >> 4) & 0x1);

    let e1_r = (((ep >> 16) & 0xF) << 1) | ((ep >> 3) & 0x1);
    let e1_g = (((ep >> 12) & 0xF) << 2) | ((ep >> 1) & 0x3);
    let e1_b = (((ep >> 8) & 0xF) << 1) | (ep & 0x1);

    let e0 = ((e0_r << 11) | (e0_g << 5) | e0_b) as u16;
    let e1 = ((e1_r << 11) | (e1_g << 5) | e1_b) as u16;
    (e0, e1)
}

/// Micro-pattern 2 (Transform 32/33): 5:6:5 High/Low Entropy Split matching UnshuffleBC1x.hlsl
pub(crate) fn shuffle_bc1_v2(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 8;
    let total_blocks = src.len() / BLOCK_SIZE;
    let num_pairs = total_blocks / 2;
    let shuffle_size = num_pairs * BLOCK_SIZE * 2;

    if shuffle_size > 0 {
        let (endpoints_stream, indices_stream) = dst[..shuffle_size].split_at_mut(shuffle_size / 2);

        for (src_pair, (ep_out, idx_out)) in src[..shuffle_size].chunks_exact(16).zip(
            endpoints_stream
                .chunks_exact_mut(8)
                .zip(indices_stream.chunks_exact_mut(8)),
        ) {
            let e0_0 = u16::from_le_bytes([src_pair[0], src_pair[1]]);
            let e1_0 = u16::from_le_bytes([src_pair[2], src_pair[3]]);
            let ep0 = pack_bc1_v2_endpoint_pair(e0_0, e1_0);

            let e0_1 = u16::from_le_bytes([src_pair[8], src_pair[9]]);
            let e1_1 = u16::from_le_bytes([src_pair[10], src_pair[11]]);
            let ep1 = pack_bc1_v2_endpoint_pair(e0_1, e1_1);

            ep_out[0..4].copy_from_slice(&ep0.to_le_bytes());
            ep_out[4..8].copy_from_slice(&ep1.to_le_bytes());

            idx_out[0..4].copy_from_slice(&src_pair[4..8]);
            idx_out[4..8].copy_from_slice(&src_pair[12..16]);
        }
    }

    if !total_blocks.is_multiple_of(2) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

pub(crate) fn unshuffle_bc1_v2(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 8;
    let total_blocks = dst.len() / BLOCK_SIZE;
    let num_pairs = total_blocks / 2;
    let shuffle_size = num_pairs * BLOCK_SIZE * 2;

    if shuffle_size > 0 {
        let (endpoints_stream, indices_stream) = src[..shuffle_size].split_at(shuffle_size / 2);

        for (dst_pair, (ep_in, idx_in)) in dst[..shuffle_size].chunks_exact_mut(16).zip(
            endpoints_stream
                .chunks_exact(8)
                .zip(indices_stream.chunks_exact(8)),
        ) {
            let ep0 = u32::from_le_bytes(ep_in[0..4].try_into().unwrap());
            let ep1 = u32::from_le_bytes(ep_in[4..8].try_into().unwrap());

            let (e0_0, e1_0) = unpack_bc1_v2_endpoint_pair(ep0);
            let (e0_1, e1_1) = unpack_bc1_v2_endpoint_pair(ep1);

            dst_pair[0..2].copy_from_slice(&e0_0.to_le_bytes());
            dst_pair[2..4].copy_from_slice(&e1_0.to_le_bytes());
            dst_pair[4..8].copy_from_slice(&idx_in[0..4]);

            dst_pair[8..10].copy_from_slice(&e0_1.to_le_bytes());
            dst_pair[10..12].copy_from_slice(&e1_1.to_le_bytes());
            dst_pair[12..16].copy_from_slice(&idx_in[4..8]);
        }
    }

    if !total_blocks.is_multiple_of(2) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

// ============================================================================
// BC3 Shufflers (Micro-pattern 1 & 2)
// ============================================================================

pub(crate) fn shuffle_bc3(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 16;
    let total_blocks = src.len() / BLOCK_SIZE;
    let num_quads = total_blocks / 4;
    let shuffle_size = num_quads * BLOCK_SIZE * 4;

    if shuffle_size > 0 {
        let s1 = shuffle_size / 16;
        let s2 = shuffle_size / 8;
        let s3 = shuffle_size / 2;

        let (d1, rest) = dst[..shuffle_size].split_at_mut(s1);
        let (d2, rest) = rest.split_at_mut(s1);
        let (d3, rest) = rest.split_at_mut(s3 - s2);
        let (d4, rest) = rest.split_at_mut(s2);
        let (d5, d6) = rest.split_at_mut(s2);

        for (src_block, (c1, (c2, (c3, (c4, (c5, c6)))))) in
            src[..shuffle_size].chunks_exact(16).zip(
                d1.iter_mut().zip(
                    d2.iter_mut().zip(
                        d3.chunks_exact_mut(6).zip(
                            d4.chunks_exact_mut(2)
                                .zip(d5.chunks_exact_mut(2).zip(d6.chunks_exact_mut(4))),
                        ),
                    ),
                ),
            )
        {
            *c1 = src_block[0];
            *c2 = src_block[1];
            c3.copy_from_slice(&src_block[2..8]);
            c4.copy_from_slice(&src_block[8..10]);
            c5.copy_from_slice(&src_block[10..12]);
            c6.copy_from_slice(&src_block[12..16]);
        }
    }

    if !total_blocks.is_multiple_of(4) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

pub(crate) fn unshuffle_bc3(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 16;
    let total_blocks = dst.len() / BLOCK_SIZE;
    let num_quads = total_blocks / 4;
    let shuffle_size = num_quads * BLOCK_SIZE * 4;

    if shuffle_size > 0 {
        let s1 = shuffle_size / 16;
        let s2 = shuffle_size / 8;
        let s3 = shuffle_size / 2;

        let d1 = &src[0..s1];
        let d2 = &src[s1..s2];
        let d3 = &src[s2..s3];
        let d4 = &src[s3..s3 + s2];
        let d5 = &src[s3 + s2..s3 + shuffle_size / 4];
        let d6 = &src[s3 + shuffle_size / 4..shuffle_size];

        for (dst_block, (c1, (c2, (c3, (c4, (c5, c6)))))) in
            dst[..shuffle_size].chunks_exact_mut(16).zip(
                d1.iter().zip(
                    d2.iter().zip(
                        d3.chunks_exact(6).zip(
                            d4.chunks_exact(2)
                                .zip(d5.chunks_exact(2).zip(d6.chunks_exact(4))),
                        ),
                    ),
                ),
            )
        {
            dst_block[0] = *c1;
            dst_block[1] = *c2;
            dst_block[2..8].copy_from_slice(c3);
            dst_block[8..10].copy_from_slice(c4);
            dst_block[10..12].copy_from_slice(c5);
            dst_block[12..16].copy_from_slice(c6);
        }
    }

    if !total_blocks.is_multiple_of(4) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

/// Micro-pattern 2 (Transform 34/35): 3-Stream 6:6:4 Ratio Split matching UnshuffleBC3x.hlsl
pub(crate) fn shuffle_bc3_v2(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 16;
    let total_blocks = src.len() / BLOCK_SIZE;
    let num_quads = total_blocks / 4;
    let shuffle_size = num_quads * BLOCK_SIZE * 4;

    if shuffle_size > 0 {
        let s_alpha_color = (shuffle_size * 6) / 16;
        let s_alpha_indices = (shuffle_size * 6) / 16;

        let (stream0, rest) = dst[..shuffle_size].split_at_mut(s_alpha_color);
        let (stream1, stream2) = rest.split_at_mut(s_alpha_indices);

        for (block, (s0, (s1, s2))) in src[..shuffle_size].chunks_exact(BLOCK_SIZE).zip(
            stream0
                .chunks_exact_mut(6)
                .zip(stream1.chunks_exact_mut(6).zip(stream2.chunks_exact_mut(4))),
        ) {
            let a0 = block[0];
            let a1 = block[1];
            let e0 = u16::from_le_bytes([block[8], block[9]]);
            let e1 = u16::from_le_bytes([block[10], block[11]]);

            let b0 = ((a0 & 0x0F) << 4) | (a1 & 0x0F);
            let b1 = (a0 & 0xF0) | (a1 >> 4);
            let ep_color = pack_bc1_v2_endpoint_pair(e0, e1);

            s0[0] = b0;
            s0[1] = b1;
            s0[2..6].copy_from_slice(&ep_color.to_le_bytes());
            s1.copy_from_slice(&block[2..8]);
            s2.copy_from_slice(&block[12..16]);
        }
    }

    if !total_blocks.is_multiple_of(4) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

pub(crate) fn unshuffle_bc3_v2(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 16;
    let total_blocks = dst.len() / BLOCK_SIZE;
    let num_quads = total_blocks / 4;
    let shuffle_size = num_quads * BLOCK_SIZE * 4;

    if shuffle_size > 0 {
        let s_alpha_color = (shuffle_size * 6) / 16;
        let s_alpha_indices = (shuffle_size * 6) / 16;

        let (stream0, rest) = src[..shuffle_size].split_at(s_alpha_color);
        let (stream1, stream2) = rest.split_at(s_alpha_indices);

        for (dst_block, (s0, (s1, s2))) in dst[..shuffle_size].chunks_exact_mut(BLOCK_SIZE).zip(
            stream0
                .chunks_exact(6)
                .zip(stream1.chunks_exact(6).zip(stream2.chunks_exact(4))),
        ) {
            let b0 = s0[0];
            let b1 = s0[1];
            let a0 = ((b0 >> 4) & 0x0F) | (b1 & 0xF0);
            let a1 = (b0 & 0x0F) | ((b1 & 0x0F) << 4);

            let ep_color = u32::from_le_bytes(s0[2..6].try_into().unwrap());
            let (e0, e1) = unpack_bc1_v2_endpoint_pair(ep_color);

            dst_block[0] = a0;
            dst_block[1] = a1;
            dst_block[2..8].copy_from_slice(s1);
            dst_block[8..10].copy_from_slice(&e0.to_le_bytes());
            dst_block[10..12].copy_from_slice(&e1.to_le_bytes());
            dst_block[12..16].copy_from_slice(s2);
        }
    }

    if !total_blocks.is_multiple_of(4) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

// ============================================================================
// BC4 & BC5 Shufflers (Micro-pattern 1)
// ============================================================================

pub(crate) fn shuffle_bc4(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 8;
    let total_blocks = src.len() / BLOCK_SIZE;
    let num_quads = total_blocks / 4;
    let shuffle_size = num_quads * BLOCK_SIZE * 4;

    if shuffle_size > 0 {
        let s1 = shuffle_size / 8;
        let (d1, rest) = dst[..shuffle_size].split_at_mut(s1);
        let (d2, d3) = rest.split_at_mut(s1);

        for (src_block, (c1, (c2, c3))) in src[..shuffle_size]
            .chunks_exact(8)
            .zip(d1.iter_mut().zip(d2.iter_mut().zip(d3.chunks_exact_mut(6))))
        {
            *c1 = src_block[0];
            *c2 = src_block[1];
            c3.copy_from_slice(&src_block[2..8]);
        }
    }

    if !total_blocks.is_multiple_of(4) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

pub(crate) fn unshuffle_bc4(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 8;
    let total_blocks = dst.len() / BLOCK_SIZE;
    let num_quads = total_blocks / 4;
    let shuffle_size = num_quads * BLOCK_SIZE * 4;

    if shuffle_size > 0 {
        let s1 = shuffle_size / 8;
        let (d1, rest) = src[..shuffle_size].split_at(s1);
        let (d2, d3) = rest.split_at(s1);

        for (dst_block, (c1, (c2, c3))) in dst[..shuffle_size]
            .chunks_exact_mut(8)
            .zip(d1.iter().zip(d2.iter().zip(d3.chunks_exact(6))))
        {
            dst_block[0] = *c1;
            dst_block[1] = *c2;
            dst_block[2..8].copy_from_slice(c3);
        }
    }

    if !total_blocks.is_multiple_of(4) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

pub(crate) fn shuffle_bc5(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 16;
    let total_blocks = src.len() / BLOCK_SIZE;
    let num_quads = total_blocks / 4;
    let shuffle_size = num_quads * BLOCK_SIZE * 4;

    if shuffle_size > 0 {
        let s1 = shuffle_size / 16;
        let s2 = shuffle_size / 8;
        let s3 = shuffle_size / 2;

        let (d1, rest) = dst[..shuffle_size].split_at_mut(s1);
        let (d2, rest) = rest.split_at_mut(s1);
        let (d3, rest) = rest.split_at_mut(s3 - s2);
        let (d4, rest) = rest.split_at_mut(s1);
        let (d5, d6) = rest.split_at_mut(s1);

        for (src_block, (c1, (c2, (c3, (c4, (c5, c6)))))) in
            src[..shuffle_size].chunks_exact(16).zip(
                d1.iter_mut().zip(
                    d2.iter_mut().zip(
                        d3.chunks_exact_mut(6)
                            .zip(d4.iter_mut().zip(d5.iter_mut().zip(d6.chunks_exact_mut(6)))),
                    ),
                ),
            )
        {
            *c1 = src_block[0];
            *c2 = src_block[1];
            c3.copy_from_slice(&src_block[2..8]);
            *c4 = src_block[8];
            *c5 = src_block[9];
            c6.copy_from_slice(&src_block[10..16]);
        }
    }

    if !total_blocks.is_multiple_of(4) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

pub(crate) fn unshuffle_bc5(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 16;
    let total_blocks = dst.len() / BLOCK_SIZE;
    let num_quads = total_blocks / 4;
    let shuffle_size = num_quads * BLOCK_SIZE * 4;

    if shuffle_size > 0 {
        let s1 = shuffle_size / 16;
        let s2 = shuffle_size / 8;
        let s3 = shuffle_size / 2;

        let d1 = &src[0..s1];
        let d2 = &src[s1..s2];
        let d3 = &src[s2..s3];
        let d4 = &src[s3..s3 + s1];
        let d5 = &src[s3 + s1..s3 + s2];
        let d6 = &src[s3 + s2..shuffle_size];

        for (dst_block, (c1, (c2, (c3, (c4, (c5, c6)))))) in
            dst[..shuffle_size].chunks_exact_mut(16).zip(
                d1.iter().zip(
                    d2.iter().zip(
                        d3.chunks_exact(6)
                            .zip(d4.iter().zip(d5.iter().zip(d6.chunks_exact(6)))),
                    ),
                ),
            )
        {
            dst_block[0] = *c1;
            dst_block[1] = *c2;
            dst_block[2..8].copy_from_slice(c3);
            dst_block[8] = *c4;
            dst_block[9] = *c5;
            dst_block[10..16].copy_from_slice(c6);
        }
    }

    if !total_blocks.is_multiple_of(4) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

// ============================================================================
// BC2 & BC6H Shufflers
// ============================================================================

pub(crate) fn shuffle_bc2(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 16;
    let num_quads = (src.len() / BLOCK_SIZE) / 4;
    let shuffle_size = num_quads * BLOCK_SIZE * 4;

    if shuffle_size > 0 {
        let s1 = shuffle_size / 2;
        let s2 = shuffle_size / 8;
        let (d1, rest) = dst[..shuffle_size].split_at_mut(s1);
        let (d2, rest) = rest.split_at_mut(s2);
        let (d3, d4) = rest.split_at_mut(s2);

        for (src_block, (c1, (c2, (c3, c4)))) in src[..shuffle_size].chunks_exact(16).zip(
            d1.chunks_exact_mut(8).zip(
                d2.chunks_exact_mut(2)
                    .zip(d3.chunks_exact_mut(2).zip(d4.chunks_exact_mut(4))),
            ),
        ) {
            c1.copy_from_slice(&src_block[0..8]);
            c2.copy_from_slice(&src_block[8..10]);
            c3.copy_from_slice(&src_block[10..12]);
            c4.copy_from_slice(&src_block[12..16]);
        }
    }

    if !(src.len() / BLOCK_SIZE).is_multiple_of(4) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

pub(crate) fn unshuffle_bc2(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 16;
    let num_quads = (dst.len() / BLOCK_SIZE) / 4;
    let shuffle_size = num_quads * BLOCK_SIZE * 4;

    if shuffle_size > 0 {
        let s1 = shuffle_size / 2;
        let s2 = shuffle_size / 8;
        let (d1, rest) = src[..shuffle_size].split_at(s1);
        let (d2, rest) = rest.split_at(s2);
        let (d3, d4) = rest.split_at(s2);

        for (dst_block, (c1, (c2, (c3, c4)))) in dst[..shuffle_size].chunks_exact_mut(16).zip(
            d1.chunks_exact(8).zip(
                d2.chunks_exact(2)
                    .zip(d3.chunks_exact(2).zip(d4.chunks_exact(4))),
            ),
        ) {
            dst_block[0..8].copy_from_slice(c1);
            dst_block[8..10].copy_from_slice(c2);
            dst_block[10..12].copy_from_slice(c3);
            dst_block[12..16].copy_from_slice(c4);
        }
    }

    if !(dst.len() / BLOCK_SIZE).is_multiple_of(4) {
        dst[shuffle_size..].copy_from_slice(&src[shuffle_size..]);
    }
    Ok(())
}

pub(crate) fn shuffle_bc6h_join(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 16;
    let total_blocks = src.len() / BLOCK_SIZE;
    if total_blocks == 0 {
        return Ok(());
    }
    let (header_stream, index_stream) = dst.split_at_mut(total_blocks * 10);
    for (src_chunk, (h, idx)) in src.chunks_exact(BLOCK_SIZE).zip(
        header_stream
            .chunks_exact_mut(10)
            .zip(index_stream.chunks_exact_mut(6)),
    ) {
        h.copy_from_slice(&src_chunk[0..10]);
        idx.copy_from_slice(&src_chunk[10..16]);
    }
    Ok(())
}

pub(crate) fn unshuffle_bc6h_join(src: &[u8], dst: &mut [u8]) -> GpckResult<()> {
    const BLOCK_SIZE: usize = 16;
    let total_blocks = dst.len() / BLOCK_SIZE;
    if total_blocks == 0 {
        return Ok(());
    }
    let (header_stream, index_stream) = src.split_at(total_blocks * 10);
    for (dst_chunk, (h, idx)) in dst.chunks_exact_mut(BLOCK_SIZE).zip(
        header_stream
            .chunks_exact(10)
            .zip(index_stream.chunks_exact(6)),
    ) {
        dst_chunk[0..10].copy_from_slice(h);
        dst_chunk[10..16].copy_from_slice(idx);
    }
    Ok(())
}
