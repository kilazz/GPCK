// src/gacl/space_curve.rs
//! # Morton Space Curve 2D Tile Transformations
//!
//! Transforms linear texture memory into 2D Morton Z-Order spatial curves to maximize
//! L1/L2 cache hit-rates for surrounding 4x4 BCn pixel blocks.
//! Handles full multi-tile reordering with safe boundary tail preservation.

use super::shufflers::pext_u64;

pub(crate) fn apply_space_curve_internal(
    src: &[u8],
    dst: &mut [u8],
    element_size_bytes: usize,
    width_in_pixels: usize,
    forward: bool,
) -> bool {
    let width_in_elements = width_in_pixels.div_ceil(4);
    let pitch_bytes = element_size_bytes * width_in_elements;
    if pitch_bytes == 0 || src.is_empty() || dst.len() < src.len() {
        dst[..src.len()].copy_from_slice(src);
        return false;
    }

    let height_in_elements = src.len().div_ceil(pitch_bytes);
    let min_width_elements = if element_size_bytes == 8 { 64 } else { 32 };

    if (element_size_bytes == 8 || element_size_bytes == 16)
        && src.len() >= 16 * 1024
        && width_in_elements.is_power_of_two()
        && width_in_elements >= min_width_elements
        && height_in_elements.is_power_of_two()
        && height_in_elements >= 32
    {
        let tile_size_bytes = 16 * 1024;
        let tiles = src.len() / tile_size_bytes;
        let tile_width_elements = if element_size_bytes == 16 { 32 } else { 64 };
        let width_in_tiles = width_in_elements / tile_width_elements;

        if width_in_tiles == 0 || tiles < width_in_tiles {
            dst[..src.len()].copy_from_slice(src);
            return false;
        }

        let height_in_tiles = tiles / width_in_tiles;
        if height_in_tiles == 0 {
            dst[..src.len()].copy_from_slice(src);
            return false;
        }

        let mut mask_x = 0xAAAAAAAAu64;
        let mut mask_y = 0x55555555u64;
        if width_in_tiles > height_in_tiles {
            let small_dim_mask = ((height_in_tiles * height_in_tiles) - 1) as u64;
            mask_y &= small_dim_mask;
            mask_x |= !small_dim_mask;
        } else if width_in_tiles < height_in_tiles {
            let small_dim_mask = ((width_in_tiles * width_in_tiles) - 1) as u64;
            mask_y |= !small_dim_mask;
            mask_x &= small_dim_mask;
        }

        for t in 0..tiles {
            let tx = pext_u64(t as u64, mask_x) as usize;
            let ty = pext_u64(t as u64, mask_y) as usize;
            let first_tile_byte = ty * (tile_size_bytes * width_in_tiles)
                + tx * (tile_width_elements * element_size_bytes);

            for r in 0..32 {
                let first_row_byte = first_tile_byte + r * pitch_bytes;
                let tile_row_start = tile_size_bytes * t + 512 * r;
                let tile_row_end = tile_row_start + 512;

                if forward {
                    if first_row_byte + 512 <= src.len() && tile_row_end <= dst.len() {
                        dst[tile_row_start..tile_row_end]
                            .copy_from_slice(&src[first_row_byte..first_row_byte + 512]);
                    }
                } else if tile_row_end <= src.len() && first_row_byte + 512 <= dst.len() {
                    dst[first_row_byte..first_row_byte + 512]
                        .copy_from_slice(&src[tile_row_start..tile_row_end]);
                }
            }
        }

        // Reliably copy unaligned tail bytes to prevent truncation/corruption
        let processed_bytes = tiles * tile_size_bytes;
        if processed_bytes < src.len() && processed_bytes < dst.len() {
            let rem = (src.len() - processed_bytes).min(dst.len() - processed_bytes);
            dst[processed_bytes..processed_bytes + rem]
                .copy_from_slice(&src[processed_bytes..processed_bytes + rem]);
        }

        true
    } else {
        dst[..src.len()].copy_from_slice(src);
        false
    }
}

pub(crate) fn apply_space_curve_decoded_internal(
    src: &[u8],
    dst: &mut [u8],
    encoded_element_size_bytes: usize,
    decoded_pixel_size_bytes: usize,
    width_in_pixels: usize,
    forward: bool,
) -> bool {
    if width_in_pixels == 0
        || decoded_pixel_size_bytes == 0
        || src.is_empty()
        || dst.len() < src.len()
    {
        dst[..src.len()].copy_from_slice(src);
        return false;
    }

    let size_in_pixels = src.len() / decoded_pixel_size_bytes;
    let height_in_pixels = size_in_pixels / width_in_pixels;

    let row_pitch_bytes = width_in_pixels * decoded_pixel_size_bytes;
    let tile_row_pitch_bytes = row_pitch_bytes * 128;
    let width_in_elements = width_in_pixels.div_ceil(4);
    let height_in_elements = height_in_pixels.div_ceil(4);
    let min_width_elements = if encoded_element_size_bytes == 8 {
        64
    } else {
        32
    };

    if (encoded_element_size_bytes == 8 || encoded_element_size_bytes == 16)
        && width_in_elements * height_in_elements * 16 * decoded_pixel_size_bytes == src.len()
        && width_in_elements.is_power_of_two()
        && width_in_elements >= min_width_elements
        && height_in_elements.is_power_of_two()
        && height_in_elements >= 32
    {
        let encoded_tile_size_bytes = 16 * 1024;
        let tile_size_in_elements = encoded_tile_size_bytes / encoded_element_size_bytes;
        if tile_size_in_elements == 0 {
            dst[..src.len()].copy_from_slice(src);
            return false;
        }

        let tiles = (width_in_elements * height_in_elements) / tile_size_in_elements;
        let tile_width_elements = if encoded_element_size_bytes == 16 {
            32
        } else {
            64
        };
        let width_in_tiles = width_in_elements / tile_width_elements;

        if width_in_tiles == 0 || tiles < width_in_tiles {
            dst[..src.len()].copy_from_slice(src);
            return false;
        }

        let height_in_tiles = tiles / width_in_tiles;
        if height_in_tiles == 0 {
            dst[..src.len()].copy_from_slice(src);
            return false;
        }

        let tile_pitch_bytes = tile_width_elements * 4 * decoded_pixel_size_bytes;

        let mut mask_x = 0xAAAAAAAAu64;
        let mut mask_y = 0x55555555u64;
        if width_in_tiles > height_in_tiles {
            let small_dim_mask = ((height_in_tiles * height_in_tiles) - 1) as u64;
            mask_y &= small_dim_mask;
            mask_x |= !small_dim_mask;
        } else if width_in_tiles < height_in_tiles {
            let small_dim_mask = ((width_in_tiles * width_in_tiles) - 1) as u64;
            mask_y |= !small_dim_mask;
            mask_x &= small_dim_mask;
        }

        let decoded_tile_size_bytes = tile_size_in_elements * 16 * decoded_pixel_size_bytes;

        for t in 0..tiles {
            let tx = pext_u64(t as u64, mask_x) as usize;
            let ty = pext_u64(t as u64, mask_y) as usize;
            let first_tile_byte = ty * tile_row_pitch_bytes + tx * tile_pitch_bytes;

            for r in 0..128 {
                let first_row_byte = first_tile_byte + r * row_pitch_bytes;
                let tile_slice_start = decoded_tile_size_bytes * t + tile_pitch_bytes * r;
                let tile_slice_end = tile_slice_start + tile_pitch_bytes;

                if forward {
                    if first_row_byte + tile_pitch_bytes <= src.len() && tile_slice_end <= dst.len()
                    {
                        dst[tile_slice_start..tile_slice_end].copy_from_slice(
                            &src[first_row_byte..first_row_byte + tile_pitch_bytes],
                        );
                    }
                } else if tile_slice_end <= src.len()
                    && first_row_byte + tile_pitch_bytes <= dst.len()
                {
                    dst[first_row_byte..first_row_byte + tile_pitch_bytes]
                        .copy_from_slice(&src[tile_slice_start..tile_slice_end]);
                }
            }
        }

        // Reliably copy unaligned tail bytes
        let processed_bytes = tiles * decoded_tile_size_bytes;
        if processed_bytes < src.len() && processed_bytes < dst.len() {
            let rem = (src.len() - processed_bytes).min(dst.len() - processed_bytes);
            dst[processed_bytes..processed_bytes + rem]
                .copy_from_slice(&src[processed_bytes..processed_bytes + rem]);
        }

        true
    } else {
        dst[..src.len()].copy_from_slice(src);
        false
    }
}
