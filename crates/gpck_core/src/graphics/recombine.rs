// crates/gpck_core/src/graphics/recombine.rs
//! # Unified Texture Recombiner & DDS Header Synthesizer
//!
//! Provides centralized texture payload deconditioning (GACL unshuffling),
//! DDS header synthesis with exact Mip-0 pitch/stride alignment, and multi-tier
//! mipmap recombination (Base + `.highmips`).

use crate::core::error::GpckResult;
use crate::format::archive::FileEntry;
use crate::format::dds::{DDSCAPS_COMPLEX, DDSCAPS_MIPMAP, DDSCAPS_TEXTURE};
use crate::gacl::{Gacl, GaclTransform};
use crate::graphics::dxgi_format::D3D12FormatTable;

pub struct TextureRecombiner;

impl TextureRecombiner {
    /// Synthesizes a valid binary DDS DX10 header directly for the given DXGI format.
    ///
    /// Computes the exact byte length of Mip 0 for `dwPitchOrLinearSize` to prevent
    /// stride and row-pitch misalignment when decoding multi-mip streaming slices (such as `.highmips`).
    pub fn wrap_in_dds_header(
        width: u32,
        height: u32,
        dxgi_format: u32,
        raw_data: &[u8],
    ) -> Vec<u8> {
        let mut dds_bytes = Vec::with_capacity(148 + raw_data.len());
        dds_bytes.extend_from_slice(b"DDS ");

        // Compute exact byte length for Mip 0 to ensure correct stride calculation in DDS decoders
        let element_size = D3D12FormatTable::get_element_size(dxgi_format).unwrap_or(16);
        let mip0_bytes = if D3D12FormatTable::is_block_compressed(dxgi_format) {
            (width.div_ceil(4) * height.div_ceil(4)) * element_size as u32
        } else {
            let bpu = D3D12FormatTable::get_bits_per_unit(dxgi_format);
            (width * height * bpu) / 8
        };

        let mut header = [0u8; 124];
        header[0..4].copy_from_slice(&124u32.to_le_bytes());
        header[4..8].copy_from_slice(&(0x1 | 0x2 | 0x4 | 0x1000 | 0x80000u32).to_le_bytes()); // DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT | DDSD_LINEARSIZE
        header[8..12].copy_from_slice(&height.to_le_bytes());
        header[12..16].copy_from_slice(&width.to_le_bytes());
        header[16..20].copy_from_slice(&mip0_bytes.to_le_bytes()); // Exact Mip-0 size prevents pitch skew
        header[20..24].copy_from_slice(&1u32.to_le_bytes());
        header[24..28].copy_from_slice(&1u32.to_le_bytes()); // MipMapCount = 1 for standalone slice
        header[72..76].copy_from_slice(&32u32.to_le_bytes());
        header[76..80].copy_from_slice(&0x4u32.to_le_bytes()); // DDPF_FOURCC
        header[80..84].copy_from_slice(&u32::from_le_bytes(*b"DX10").to_le_bytes());
        header[104..108].copy_from_slice(&(DDSCAPS_TEXTURE | DDSCAPS_COMPLEX).to_le_bytes());

        dds_bytes.extend_from_slice(&header);

        let mut dx10_header = [0u8; 20];
        dx10_header[0..4].copy_from_slice(&dxgi_format.to_le_bytes());
        dx10_header[4..8].copy_from_slice(&3u32.to_le_bytes()); // D3D12_RESOURCE_DIMENSION_TEXTURE2D
        dx10_header[12..16].copy_from_slice(&1u32.to_le_bytes()); // ArraySize = 1

        dds_bytes.extend_from_slice(&dx10_header);
        dds_bytes.extend_from_slice(raw_data);

        dds_bytes
    }

    /// Reverses GACL stream shuffling on a texture buffer while preserving its container header.
    pub fn unshuffle_payload(
        path: &str,
        data: &[u8],
        gacl_transform: u32,
        width_pixels: usize,
        decondition_gacl: bool,
    ) -> Vec<u8> {
        if !decondition_gacl || gacl_transform == GaclTransform::None.to_u32() || data.is_empty() {
            return data.to_vec();
        }

        let is_dds = path.to_lowercase().ends_with(".dds");
        let header_len = if is_dds {
            if data.len() >= 148 && &data[84..88] == b"DX10" {
                148
            } else if data.len() >= 128 && data.starts_with(b"DDS ") {
                128
            } else {
                0
            }
        } else {
            0
        };

        if data.len() <= header_len {
            return data.to_vec();
        }

        let header = &data[..header_len];
        let pixels = &data[header_len..];

        let mut clean_data = Vec::with_capacity(data.len());
        clean_data.extend_from_slice(header);

        if let Ok(unshuffled) = Gacl::unshuffle(gacl_transform, pixels, pixels.len(), width_pixels)
        {
            clean_data.extend_from_slice(&unshuffled);
        } else {
            clean_data.extend_from_slice(pixels);
        }

        clean_data
    }

    /// Recombines a baseline texture and its streaming `.highmips` companion into a single DDS file.
    pub fn recombine_dds(
        rel_path: &str,
        base_raw: &[u8],
        highmips_raw: Option<&[u8]>,
        entry: &FileEntry,
        high_gacl_transform: u32,
        decondition_gacl: bool,
    ) -> GpckResult<Vec<u8>> {
        let width = ((entry.meta1 >> 16) & 0xFFFF) as usize;
        let height = (entry.meta1 & 0xFFFF) as usize;
        let base_gacl = entry.gacl_transform();

        let base_clean =
            Self::unshuffle_payload(rel_path, base_raw, base_gacl, width, decondition_gacl);

        if let Some(high_bytes) = highmips_raw {
            let high_clean = Self::unshuffle_payload(
                &format!("{}.highmips", rel_path),
                high_bytes,
                high_gacl_transform,
                width,
                decondition_gacl,
            );

            let header_len = if base_clean.len() >= 148 && &base_clean[84..88] == b"DX10" {
                148
            } else if base_clean.len() >= 128 && base_clean.starts_with(b"DDS ") {
                128
            } else {
                0
            };

            if base_clean.len() >= header_len && header_len > 0 {
                let mut header = base_clean[..header_len].to_vec();
                if width > 0 && height > 0 {
                    header[12..16].copy_from_slice(&(height as u32).to_le_bytes());
                    header[16..20].copy_from_slice(&(width as u32).to_le_bytes());
                }
                let mip_count = (entry.meta2 >> 24) & 0xFF;
                if mip_count > 0 {
                    header[28..32].copy_from_slice(&mip_count.to_le_bytes());
                }

                // Restore MipMap & Subtree caps flags
                let mut caps = u32::from_le_bytes(header[104..108].try_into().unwrap_or([0; 4]));
                caps |= DDSCAPS_TEXTURE | DDSCAPS_COMPLEX | DDSCAPS_MIPMAP;
                header[104..108].copy_from_slice(&caps.to_le_bytes());

                let mut full_recombined = Vec::with_capacity(
                    header.len() + high_clean.len() + (base_clean.len() - header_len),
                );
                full_recombined.extend_from_slice(&header);
                full_recombined.extend_from_slice(&high_clean);
                full_recombined.extend_from_slice(&base_clean[header_len..]);
                return Ok(full_recombined);
            }
        }

        Ok(base_clean)
    }
}
