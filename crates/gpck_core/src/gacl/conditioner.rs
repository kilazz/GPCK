// crates/gpck_core/src/gacl/conditioner.rs
//! # Game Asset Conditioning Pipeline & Format Transformer

use super::astc;
use super::bc7;
use super::rdo;
use super::shufflers;
use super::space_curve;
use super::transform::GaclTransform;

use crate::compression::codecs::{Codec, CompressionMethod};
use crate::core::error::{GpckError, GpckResult};
use crate::graphics::dxgi_format::D3D12FormatTable;

pub struct Gacl;

impl Gacl {
    /// Automatically selects and applies the optimal GACL transform for an unconditioned texture payload,
    /// evaluating candidates with the exact target compression level and ATG profile.
    pub fn auto_condition_texture(
        pixels: &[u8],
        dxgi_format: u32,
        width_pixels: usize,
        method: CompressionMethod,
        level: i32,
        atg_profile: bool,
    ) -> GpckResult<(Vec<u8>, u32)> {
        if pixels.is_empty() {
            return Ok((Vec::new(), GaclTransform::None.to_u32()));
        }

        let element_size = match D3D12FormatTable::get_element_size(dxgi_format) {
            Some(size) => size,
            None => return Ok((pixels.to_vec(), GaclTransform::None.to_u32())),
        };

        let candidate_transforms: &[GaclTransform] = if D3D12FormatTable::is_bc1(dxgi_format) {
            &[
                GaclTransform::None,
                GaclTransform::Bc1Linear,
                GaclTransform::Bc1LinearSpaceCurve,
                GaclTransform::Bc1V2BitInterleaved,
                GaclTransform::Bc1V2SpaceCurve,
            ]
        } else if D3D12FormatTable::is_bc2(dxgi_format) {
            &[GaclTransform::None, GaclTransform::Bc2AlphaNibble]
        } else if D3D12FormatTable::is_bc3(dxgi_format) {
            &[
                GaclTransform::None,
                GaclTransform::Bc3Linear,
                GaclTransform::Bc3LinearSpaceCurve,
                GaclTransform::Bc3V2BitInterleaved,
                GaclTransform::Bc3V2SpaceCurve,
            ]
        } else if D3D12FormatTable::is_bc4(dxgi_format) {
            &[
                GaclTransform::None,
                GaclTransform::Bc4Linear,
                GaclTransform::Bc4LinearSpaceCurve,
            ]
        } else if D3D12FormatTable::is_bc5(dxgi_format) {
            &[
                GaclTransform::None,
                GaclTransform::Bc5DualChannel,
                GaclTransform::Bc5SpaceCurve,
            ]
        } else if D3D12FormatTable::is_bc6h(dxgi_format) {
            &[GaclTransform::None, GaclTransform::Bc6hHeaderJoin]
        } else if D3D12FormatTable::is_bc7(dxgi_format) {
            &[
                GaclTransform::None,
                GaclTransform::Bc7ModeSplit,
                GaclTransform::Bc7ModeJoin,
            ]
        } else if element_size == 16 {
            &[GaclTransform::None, GaclTransform::CurveOnly16B]
        } else {
            &[GaclTransform::None]
        };

        let probe_len = pixels.len().min(256 * 1024);
        let block_align = element_size;
        let aligned_probe_len = (probe_len / block_align) * block_align;
        let probe_slice = &pixels[..aligned_probe_len];

        let mut best_transform = GaclTransform::None;
        let mut best_compressed_size = usize::MAX;

        // Establish strict baseline with unconditioned raw probe at the real compression level
        if let Ok(raw_comp) = Codec::compress(probe_slice, method, level, atg_profile) {
            best_compressed_size = raw_comp.len();
        }

        for &transform in candidate_transforms {
            if transform == GaclTransform::None {
                continue;
            }

            if let Ok(transformed_probe) = Self::apply_exact_transform(
                probe_slice,
                transform.to_u32(),
                element_size,
                width_pixels,
            ) && let Ok(compressed_probe) =
                Codec::compress(&transformed_probe, method, level, atg_profile)
                && compressed_probe.len() < best_compressed_size
            {
                best_compressed_size = compressed_probe.len();
                best_transform = transform;
            }
        }

        if best_transform != GaclTransform::None {
            let full_transformed = Self::apply_exact_transform(
                pixels,
                best_transform.to_u32(),
                element_size,
                width_pixels,
            )?;
            Ok((full_transformed, best_transform.to_u32()))
        } else {
            Ok((pixels.to_vec(), GaclTransform::None.to_u32()))
        }
    }

    /// Applies an exact GACL transformation by its identifier.
    pub fn apply_exact_transform(
        input: &[u8],
        transform_id: u32,
        element_size: usize,
        width_pixels: usize,
    ) -> GpckResult<Vec<u8>> {
        let transform = GaclTransform::from_u32(transform_id);
        if transform == GaclTransform::None || input.is_empty() {
            return Ok(input.to_vec());
        }

        let mut working_buffer = input.to_vec();

        if transform.has_space_curve() {
            let mut curved_buffer = vec![0u8; input.len()];
            if space_curve::apply_space_curve_internal(
                input,
                &mut curved_buffer,
                element_size,
                width_pixels,
                true,
            ) {
                working_buffer = curved_buffer;
            }
        }

        if transform == GaclTransform::CurveOnly16B {
            return Ok(working_buffer);
        }

        let mut output = vec![0u8; input.len()];
        match transform {
            GaclTransform::Bc1Linear | GaclTransform::Bc1LinearSpaceCurve => {
                shufflers::shuffle_bc1(&working_buffer, &mut output)?
            }
            GaclTransform::Bc1V2BitInterleaved | GaclTransform::Bc1V2SpaceCurve => {
                shufflers::shuffle_bc1_v2(&working_buffer, &mut output)?
            }
            GaclTransform::Bc2AlphaNibble => shufflers::shuffle_bc2(&working_buffer, &mut output)?,
            GaclTransform::Bc3Linear | GaclTransform::Bc3LinearSpaceCurve => {
                shufflers::shuffle_bc3(&working_buffer, &mut output)?
            }
            GaclTransform::Bc3V2BitInterleaved | GaclTransform::Bc3V2SpaceCurve => {
                shufflers::shuffle_bc3_v2(&working_buffer, &mut output)?
            }
            GaclTransform::Bc4Linear | GaclTransform::Bc4LinearSpaceCurve => {
                shufflers::shuffle_bc4(&working_buffer, &mut output)?
            }
            GaclTransform::Bc5DualChannel | GaclTransform::Bc5SpaceCurve => {
                shufflers::shuffle_bc5(&working_buffer, &mut output)?
            }
            GaclTransform::Bc6hHeaderJoin => {
                shufflers::shuffle_bc6h_join(&working_buffer, &mut output)?
            }
            GaclTransform::Bc7ModeSplit => {
                bc7::bc7_mode_split_transform(&working_buffer, &mut output)
                    .map_err(|e| GpckError::GaclError(e.to_string()))?
            }
            GaclTransform::Bc7ModeJoin => {
                bc7::bc7_mode_join_transform(&working_buffer, &mut output)
                    .map_err(|e| GpckError::GaclError(e.to_string()))?
            }
            GaclTransform::Astc4x4Linear
            | GaclTransform::Astc4x4SpaceCurve
            | GaclTransform::Astc6x6Linear
            | GaclTransform::Astc6x6SpaceCurve
            | GaclTransform::Astc8x8Linear
            | GaclTransform::Astc8x8SpaceCurve => {
                astc::AstcConditioner::condition_astc_blocks(&working_buffer, &mut output)
                    .map_err(|e| GpckError::GaclError(e.to_string()))?
            }
            _ => return Ok(working_buffer),
        }

        Ok(output)
    }

    /// Full texture conditioning pipeline ensuring strict order: RDO -> SpaceCurve -> GACL.
    pub fn condition_texture_pipeline(
        pixels: &[u8],
        dxgi_format: u32,
        width_pixels: usize,
        _height_pixels: usize,
        target_reduction_pct: f32,
    ) -> GpckResult<(Vec<u8>, u32, bool)> {
        if pixels.is_empty() {
            return Ok((Vec::new(), GaclTransform::None.to_u32(), false));
        }

        let element_size = match D3D12FormatTable::get_element_size(dxgi_format) {
            Some(size) => size,
            None => return Ok((pixels.to_vec(), GaclTransform::None.to_u32(), false)),
        };

        let mut working_buffer = pixels.to_vec();

        if target_reduction_pct > 0.0 {
            let reduction_ratio = if target_reduction_pct > 1.0 {
                target_reduction_pct / 100.0
            } else {
                target_reduction_pct
            };

            let _ = rdo::block_level_entropy_reduce(
                &mut working_buffer,
                element_size,
                dxgi_format,
                reduction_ratio,
                true,
            )?;
        }

        let mut curved_buffer = vec![0u8; working_buffer.len()];
        let space_curve_applied = space_curve::apply_space_curve_internal(
            &working_buffer,
            &mut curved_buffer,
            element_size,
            width_pixels,
            true,
        );

        if space_curve_applied {
            working_buffer = curved_buffer;
        }

        let (shuffled_buffer, base_transform) =
            Self::shuffle_compress(dxgi_format, &working_buffer)?;

        if base_transform == GaclTransform::None.to_u32() {
            if space_curve_applied && element_size == 16 {
                return Ok((working_buffer, GaclTransform::CurveOnly16B.to_u32(), true));
            }
            return Ok((
                working_buffer,
                GaclTransform::None.to_u32(),
                space_curve_applied,
            ));
        }

        let base_enum = GaclTransform::from_u32(base_transform);
        let final_transform = if space_curve_applied {
            match base_enum {
                GaclTransform::Bc1Linear => GaclTransform::Bc1LinearSpaceCurve,
                GaclTransform::Bc1V2BitInterleaved => GaclTransform::Bc1V2SpaceCurve,
                GaclTransform::Bc3Linear => GaclTransform::Bc3LinearSpaceCurve,
                GaclTransform::Bc3V2BitInterleaved => GaclTransform::Bc3V2SpaceCurve,
                GaclTransform::Bc4Linear => GaclTransform::Bc4LinearSpaceCurve,
                GaclTransform::Bc5DualChannel => GaclTransform::Bc5SpaceCurve,
                GaclTransform::Astc4x4Linear => GaclTransform::Astc4x4SpaceCurve,
                GaclTransform::Astc6x6Linear => GaclTransform::Astc6x6SpaceCurve,
                GaclTransform::Astc8x8Linear => GaclTransform::Astc8x8SpaceCurve,
                _ => base_enum,
            }
        } else {
            base_enum
        };

        Ok((
            shuffled_buffer,
            final_transform.to_u32(),
            space_curve_applied,
        ))
    }

    /// Shuffles and partitions block bitfields into separate homogeneous data streams.
    pub fn shuffle_compress(dxgi_format: u32, input: &[u8]) -> GpckResult<(Vec<u8>, u32)> {
        if input.is_empty() {
            return Ok((Vec::new(), GaclTransform::None.to_u32()));
        }

        let (transform_type, block_size) = if D3D12FormatTable::is_bc1(dxgi_format) {
            (GaclTransform::Bc1V2BitInterleaved, 8)
        } else if D3D12FormatTable::is_bc2(dxgi_format) {
            (GaclTransform::Bc2AlphaNibble, 16)
        } else if D3D12FormatTable::is_bc3(dxgi_format) {
            (GaclTransform::Bc3V2BitInterleaved, 16)
        } else if D3D12FormatTable::is_bc4(dxgi_format) {
            (GaclTransform::Bc4Linear, 8)
        } else if D3D12FormatTable::is_bc5(dxgi_format) {
            (GaclTransform::Bc5DualChannel, 16)
        } else if D3D12FormatTable::is_bc6h(dxgi_format) {
            (GaclTransform::Bc6hHeaderJoin, 16)
        } else if D3D12FormatTable::is_bc7(dxgi_format) {
            (GaclTransform::Bc7ModeSplit, 16)
        } else {
            (GaclTransform::None, 0)
        };

        if transform_type == GaclTransform::None || !input.len().is_multiple_of(block_size) {
            return Ok((input.to_vec(), GaclTransform::None.to_u32()));
        }

        let mut output = vec![0u8; input.len()];

        match transform_type {
            GaclTransform::Bc1Linear => shufflers::shuffle_bc1(input, &mut output)?,
            GaclTransform::Bc1V2BitInterleaved => shufflers::shuffle_bc1_v2(input, &mut output)?,
            GaclTransform::Bc2AlphaNibble => shufflers::shuffle_bc2(input, &mut output)?,
            GaclTransform::Bc3Linear => shufflers::shuffle_bc3(input, &mut output)?,
            GaclTransform::Bc3V2BitInterleaved => shufflers::shuffle_bc3_v2(input, &mut output)?,
            GaclTransform::Bc4Linear => shufflers::shuffle_bc4(input, &mut output)?,
            GaclTransform::Bc5DualChannel => shufflers::shuffle_bc5(input, &mut output)?,
            GaclTransform::Bc6hHeaderJoin => shufflers::shuffle_bc6h_join(input, &mut output)?,
            GaclTransform::Bc7ModeJoin => bc7::bc7_mode_join_transform(input, &mut output)
                .map_err(|e| GpckError::GaclError(e.to_string()))?,
            GaclTransform::Bc7ModeSplit => bc7::bc7_mode_split_transform(input, &mut output)
                .map_err(|e| GpckError::GaclError(e.to_string()))?,
            _ => unreachable!(),
        }

        Ok((output, transform_type.to_u32()))
    }

    /// Reconstructs original standard hardware textures from GACL conditioned streams.
    pub fn unshuffle(
        transform_type: u32,
        input: &[u8],
        target_size: usize,
        width_pixels: usize,
    ) -> GpckResult<Vec<u8>> {
        let transform = GaclTransform::from_u32(transform_type);
        if transform == GaclTransform::None || input.is_empty() {
            return Ok(input.to_vec());
        }

        let mut output = vec![0u8; target_size];

        match transform {
            GaclTransform::Bc1Linear | GaclTransform::Bc1LinearSpaceCurve => {
                shufflers::unshuffle_bc1(input, &mut output)?;
                if transform == GaclTransform::Bc1LinearSpaceCurve {
                    output = Self::apply_space_curve(&output, 8, width_pixels, false)?;
                }
            }
            GaclTransform::Bc1V2BitInterleaved | GaclTransform::Bc1V2SpaceCurve => {
                shufflers::unshuffle_bc1_v2(input, &mut output)?;
                if transform == GaclTransform::Bc1V2SpaceCurve {
                    output = Self::apply_space_curve(&output, 8, width_pixels, false)?;
                }
            }
            GaclTransform::Bc3Linear | GaclTransform::Bc3LinearSpaceCurve => {
                shufflers::unshuffle_bc3(input, &mut output)?;
                if transform == GaclTransform::Bc3LinearSpaceCurve {
                    output = Self::apply_space_curve(&output, 16, width_pixels, false)?;
                }
            }
            GaclTransform::Bc3V2BitInterleaved | GaclTransform::Bc3V2SpaceCurve => {
                shufflers::unshuffle_bc3_v2(input, &mut output)?;
                if transform == GaclTransform::Bc3V2SpaceCurve {
                    output = Self::apply_space_curve(&output, 16, width_pixels, false)?;
                }
            }
            GaclTransform::Bc4Linear | GaclTransform::Bc4LinearSpaceCurve => {
                shufflers::unshuffle_bc4(input, &mut output)?;
                if transform == GaclTransform::Bc4LinearSpaceCurve {
                    output = Self::apply_space_curve(&output, 8, width_pixels, false)?;
                }
            }
            GaclTransform::Bc5DualChannel | GaclTransform::Bc5SpaceCurve => {
                shufflers::unshuffle_bc5(input, &mut output)?;
                if transform == GaclTransform::Bc5SpaceCurve {
                    output = Self::apply_space_curve(&output, 16, width_pixels, false)?;
                }
            }
            GaclTransform::CurveOnly16B => {
                output = Self::apply_space_curve(input, 16, width_pixels, false)?;
            }
            GaclTransform::Bc2AlphaNibble => shufflers::unshuffle_bc2(input, &mut output)?,
            GaclTransform::Bc6hHeaderJoin => shufflers::unshuffle_bc6h_join(input, &mut output)?,
            GaclTransform::Bc7ModeJoin => bc7::bc7_mode_join_reverse(input, &mut output)
                .map_err(|e| GpckError::GaclError(e.to_string()))?,
            GaclTransform::Bc7ModeSplit => bc7::bc7_mode_split_reverse(input, &mut output)
                .map_err(|e| GpckError::GaclError(e.to_string()))?,
            GaclTransform::Astc4x4Linear | GaclTransform::Astc4x4SpaceCurve => {
                astc::AstcConditioner::uncondition_astc_blocks(input, &mut output)
                    .map_err(|e| GpckError::GaclError(e.to_string()))?;
                if transform == GaclTransform::Astc4x4SpaceCurve {
                    output = astc::AstcConditioner::apply_astc_space_curve(
                        &output,
                        astc::AstcFootprint::Block4x4,
                        width_pixels,
                        false,
                    )
                    .map_err(|e| GpckError::GaclError(e.to_string()))?;
                }
            }
            GaclTransform::Astc6x6Linear | GaclTransform::Astc6x6SpaceCurve => {
                astc::AstcConditioner::uncondition_astc_blocks(input, &mut output)
                    .map_err(|e| GpckError::GaclError(e.to_string()))?;
                if transform == GaclTransform::Astc6x6SpaceCurve {
                    output = astc::AstcConditioner::apply_astc_space_curve(
                        &output,
                        astc::AstcFootprint::Block6x6,
                        width_pixels,
                        false,
                    )
                    .map_err(|e| GpckError::GaclError(e.to_string()))?;
                }
            }
            GaclTransform::Astc8x8Linear | GaclTransform::Astc8x8SpaceCurve => {
                astc::AstcConditioner::uncondition_astc_blocks(input, &mut output)
                    .map_err(|e| GpckError::GaclError(e.to_string()))?;
                if transform == GaclTransform::Astc8x8SpaceCurve {
                    output = astc::AstcConditioner::apply_astc_space_curve(
                        &output,
                        astc::AstcFootprint::Block8x8,
                        width_pixels,
                        false,
                    )
                    .map_err(|e| GpckError::GaclError(e.to_string()))?;
                }
            }
            _ => {
                return Err(GpckError::GaclError(format!(
                    "Unsupported GACL transform type: {}",
                    transform_type
                )));
            }
        }

        Ok(output)
    }

    /// Applies 2D Morton Z-order curve transposition on linear block buffers.
    pub fn apply_space_curve(
        input: &[u8],
        element_size: usize,
        width_pixels: usize,
        forward: bool,
    ) -> GpckResult<Vec<u8>> {
        let mut output = vec![0u8; input.len()];
        if space_curve::apply_space_curve_internal(
            input,
            &mut output,
            element_size,
            width_pixels,
            forward,
        ) {
            Ok(output)
        } else {
            Ok(input.to_vec())
        }
    }

    /// Applies Morton Space Curve transformation on pre-decoded RGBA pixel buffers.
    pub fn apply_space_curve_decoded(
        input: &[u8],
        encoded_element_size_bytes: usize,
        decoded_pixel_size_bytes: usize,
        width_pixels: usize,
        forward: bool,
    ) -> GpckResult<Vec<u8>> {
        let mut output = vec![0u8; input.len()];
        if space_curve::apply_space_curve_decoded_internal(
            input,
            &mut output,
            encoded_element_size_bytes,
            decoded_pixel_size_bytes,
            width_pixels,
            forward,
        ) {
            Ok(output)
        } else {
            Ok(input.to_vec())
        }
    }

    /// Applies Block-Level Entropy Reduction (BLER) via Lagrangian Rate-Distortion Optimization.
    pub fn apply_bler(
        encoded_data: &mut [u8],
        dxgi_format: u32,
        target_reduction_pct: f32,
        use_ycocg: bool,
    ) -> GpckResult<usize> {
        let element_size = match D3D12FormatTable::get_element_size(dxgi_format) {
            Some(size) => size,
            None => return Ok(0),
        };

        if encoded_data.is_empty() || !encoded_data.len().is_multiple_of(element_size) {
            return Ok(0);
        }

        let reduction_ratio = if target_reduction_pct > 1.0 {
            target_reduction_pct / 100.0
        } else {
            target_reduction_pct
        };

        rdo::block_level_entropy_reduce(
            encoded_data,
            element_size,
            dxgi_format,
            reduction_ratio,
            use_ycocg,
        )
    }
}
