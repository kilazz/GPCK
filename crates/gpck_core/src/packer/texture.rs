// crates/gpck_core/src/packer/texture.rs
//! # Pipeline Stage 2: Texture Conditioning, Mip-Splitting & 64KB Tile Alignment

use crate::compression::codecs::CompressionMethod;
use crate::core::asset_id::AssetIdGenerator;
use crate::core::error::GpckResult;
use crate::format::archive::{
    FLAG_BOOT_TAIL, FLAG_ENCRYPTED_META, FLAG_IS_COMPRESSED, FLAG_STREAMING, SHIFT_ALIGNMENT,
    SHIFT_GACL_TRANSFORM, TYPE_TEXTURE, TYPE_TILED_RESOURCE,
};
use crate::format::dds::{DdsBasicInfo, DdsUtils};
use crate::format::ktx2::Ktx2Utils;
use crate::gacl::{Gacl, GaclTransform};
use crate::graphics::dxgi_format::D3D12FormatTable;
use crate::packer::chunker;
use crate::packer::tiler::{TileSliceResult, TiledTexturePacker, get_linear_transform};
use crate::packer::{GaclFormatOverrides, PackerOptions, ProcessedChunk, ProcessedFile};
use std::fs;
use std::path::Path;

const GPU_ALIGNMENT: i64 = 4096;
const TILE_HARDWARE_ALIGNMENT: i64 = 65536;

pub struct ProcessedFileParams<'a> {
    pub rel_path: String,
    pub original_size: u32,
    pub chunks: Vec<ProcessedChunk>,
    pub flags: u32,
    pub tags: u32,
    pub method: CompressionMethod,
    pub alignment: i64,
    pub key: Option<&'a [u8; 32]>,
}

// ============================================================================
// Public Entry Point & Router
// ============================================================================

pub fn process_file(
    input_path: &Path,
    rel_path: &str,
    options: &PackerOptions,
) -> GpckResult<Vec<ProcessedFile>> {
    let raw_data = fs::read(input_path)?;
    let lower_path = rel_path.to_lowercase();
    let method = resolve_packer_method(options.method, &lower_path);

    if lower_path.ends_with(".dds")
        && let Some(processed) = process_dds_texture(&raw_data, rel_path, options, method)?
    {
        return Ok(processed);
    } else if lower_path.ends_with(".ktx2")
        && let Some(processed) = process_ktx2_texture(&raw_data, rel_path, options, method)?
    {
        return Ok(processed);
    }

    process_generic_binary(&raw_data, rel_path, options, method)
}

// ============================================================================
// Format Handlers
// ============================================================================

fn process_dds_texture(
    raw_data: &[u8],
    rel_path: &str,
    options: &PackerOptions,
    method: CompressionMethod,
) -> GpckResult<Option<Vec<ProcessedFile>>> {
    let Some(h_info) = DdsUtils::get_header_info(raw_data) else {
        return Ok(None);
    };

    let (dxgi_fmt, header_len) = DdsUtils::detect_dxgi_format(raw_data);
    let meta1 = ((h_info.width as u32) << 16) | ((h_info.height as u32) & 0xFFFF);
    let meta2 = ((h_info.mip_count as u32) << 24) | ((dxgi_fmt & 0xFF) << 16);

    // Evaluate Configurable Resolution & Non-Square Threshold
    let max_dim = h_info.width.max(h_info.height);
    let passes_res_threshold = if options.min_tiled_resolution == 0 {
        h_info.width >= 128 && h_info.height >= 128
    } else {
        max_dim >= options.min_tiled_resolution
    };

    if options.tiled_streaming && dxgi_fmt > 0 && passes_res_threshold {
        let mut tile_options = options.clone();
        tile_options.method = method;

        let tile_res = TiledTexturePacker::slice_and_compress_texture_tiles(
            raw_data,
            header_len,
            dxgi_fmt,
            h_info.width as u32,
            h_info.height as u32,
            h_info.mip_count as u32,
            &tile_options,
        )?;

        // Verify total tile count constraint
        if tile_res.total_tiles >= options.min_tiled_tile_count {
            return process_tiled_dds_from_result(
                tile_res,
                dxgi_fmt,
                h_info.width as u32,
                h_info.height as u32,
                rel_path,
                &tile_options,
                method,
            )
            .map(Some);
        }
    }

    // Fallback: Mip-Split layout for smaller / sub-threshold textures
    process_mipsplit_dds(
        raw_data, header_len, dxgi_fmt, &h_info, meta1, meta2, rel_path, options, method,
    )
    .map(Some)
}

fn process_tiled_dds_from_result(
    tile_res: TileSliceResult,
    dxgi_fmt: u32,
    base_width: u32,
    base_height: u32,
    rel_path: &str,
    options: &PackerOptions,
    method: CompressionMethod,
) -> GpckResult<Vec<ProcessedFile>> {
    let mut out_files = Vec::new();

    // Calculate tail dimensions (the starting resolution of the packed tail mip sequence)
    let tail_w = (base_width >> tile_res.num_standard_mips).max(1);
    let tail_h = (base_height >> tile_res.num_standard_mips).max(1);
    let tail_meta1 = (tail_w << 16) | (tail_h & 0xFFFF);
    let tail_meta2 = ((tile_res.num_packed_mips & 0xFF) << 24)
        | ((dxgi_fmt & 0xFF) << 16)
        | ((tile_res.tail_chunks.len() as u32) & 0xFFFF);

    // 1. Companion Boot-Tail File (Partition 0 Placement for Instant Startup Rendering)
    if !tile_res.tail_chunks.is_empty() {
        let tail_flags = FLAG_STREAMING
            | TYPE_TEXTURE
            | TYPE_TILED_RESOURCE
            | FLAG_BOOT_TAIL
            | ((get_linear_transform(tile_res.gacl_transform).to_u32() & 0x3F)
                << SHIFT_GACL_TRANSFORM);

        let mut tail_file = build_processed_file(ProcessedFileParams {
            rel_path: format!("{}.tail", rel_path),
            original_size: (tile_res.tail_chunks.len() * 65536) as u32,
            chunks: tile_res.tail_chunks,
            flags: tail_flags,
            tags: options.tags,
            method,
            alignment: TILE_HARDWARE_ALIGNMENT,
            key: options.key.as_ref(),
        });
        tail_file.partition_id = 0; // Strictly placed in Partition 0 (Boot Partition)
        tail_file.meta1 = tail_meta1;
        tail_file.meta2 = tail_meta2;
        out_files.push(tail_file);
    }

    // 2. Primary Tiled Resource (Standard Mip Tiles streamed on demand)
    let standard_flags = FLAG_STREAMING
        | TYPE_TEXTURE
        | TYPE_TILED_RESOURCE
        | ((tile_res.gacl_transform.to_u32() & 0x3F) << SHIFT_GACL_TRANSFORM);

    let standard_meta1 = (base_width << 16) | (base_height & 0xFFFF);
    let standard_meta2 = ((tile_res.num_standard_mips & 0xFF) << 24)
        | ((dxgi_fmt & 0xFF) << 16)
        | ((tile_res.standard_chunks.len() as u32) & 0xFFFF);

    let mut standard_file = build_processed_file(ProcessedFileParams {
        rel_path: rel_path.to_string(),
        original_size: (tile_res.standard_chunks.len() * 65536) as u32,
        chunks: tile_res.standard_chunks,
        flags: standard_flags,
        tags: options.tags,
        method,
        alignment: TILE_HARDWARE_ALIGNMENT,
        key: options.key.as_ref(),
    });
    standard_file.meta1 = standard_meta1;
    standard_file.meta2 = standard_meta2;
    out_files.push(standard_file);

    Ok(out_files)
}

#[allow(clippy::too_many_arguments)]
fn process_mipsplit_dds(
    raw_data: &[u8],
    header_len: usize,
    dxgi_fmt: u32,
    h_info: &DdsBasicInfo,
    meta1: u32,
    meta2: u32,
    rel_path: &str,
    options: &PackerOptions,
    method: CompressionMethod,
) -> GpckResult<Vec<ProcessedFile>> {
    let type_flags = TYPE_TEXTURE;
    let mut base_data;
    let mut highres_data = Vec::new();
    let mut sub_meta2 = meta2;

    let base_width = if options.mip_split {
        let (processed_data, t_size) =
            DdsUtils::process_texture_for_streaming(raw_data, options.max_tail_dim);
        base_data = processed_data[0..t_size].to_vec();
        highres_data = processed_data[t_size..].to_vec();
        sub_meta2 = meta2 | ((t_size as u32) & 0x0000FFFF);
        options.max_tail_dim.min(h_info.width)
    } else {
        base_data = raw_data.to_vec();
        h_info.width
    };

    let mut gacl_transform = GaclTransform::None;
    if options.gacl.enabled && dxgi_fmt > 0 && base_data.len() > header_len {
        let header = base_data[..header_len].to_vec();
        let payload = base_data[header_len..].to_vec();
        let (transformed_payload, transform) =
            apply_conditioning_and_rdo(payload, dxgi_fmt, base_width, options, method)?;
        gacl_transform = transform;

        let mut combined = Vec::with_capacity(header.len() + transformed_payload.len());
        combined.extend_from_slice(&header);
        combined.extend_from_slice(&transformed_payload);
        base_data = combined;
    }

    let mut high_gacl_transform = gacl_transform;
    if options.gacl.enabled && dxgi_fmt > 0 && !highres_data.is_empty() {
        let (transformed_high, transform) =
            apply_conditioning_and_rdo(highres_data, dxgi_fmt, h_info.width, options, method)?;
        high_gacl_transform = transform;
        highres_data = transformed_high;
    }

    let flags_base =
        FLAG_STREAMING | type_flags | ((gacl_transform.to_u32() & 0x3F) << SHIFT_GACL_TRANSFORM);
    let flags_high = FLAG_STREAMING
        | type_flags
        | ((high_gacl_transform.to_u32() & 0x3F) << SHIFT_GACL_TRANSFORM);

    let base_chunks = chunker::compress_to_chunks(
        &base_data,
        options.chunk_size,
        options.level,
        method,
        options.validate_chunks,
        options.atg_profile,
    )?;

    let mut out_files = Vec::new();
    let mut base_file = build_processed_file(ProcessedFileParams {
        rel_path: rel_path.to_string(),
        original_size: base_data.len() as u32,
        chunks: base_chunks,
        flags: flags_base,
        tags: options.tags,
        method,
        alignment: TILE_HARDWARE_ALIGNMENT,
        key: options.key.as_ref(),
    });
    base_file.meta1 = meta1;
    base_file.meta2 = sub_meta2;
    out_files.push(base_file);

    if !highres_data.is_empty() {
        let highres_chunks = chunker::compress_to_chunks(
            &highres_data,
            options.chunk_size,
            options.level,
            method,
            options.validate_chunks,
            options.atg_profile,
        )?;
        let mut highres_file = build_processed_file(ProcessedFileParams {
            rel_path: format!("{}.highmips", rel_path),
            original_size: highres_data.len() as u32,
            chunks: highres_chunks,
            flags: flags_high,
            tags: options.tags,
            method,
            alignment: TILE_HARDWARE_ALIGNMENT,
            key: options.key.as_ref(),
        });
        highres_file.meta1 = meta1;
        highres_file.meta2 = sub_meta2;
        out_files.push(highres_file);
    }

    Ok(out_files)
}

fn process_ktx2_texture(
    raw_data: &[u8],
    rel_path: &str,
    options: &PackerOptions,
    method: CompressionMethod,
) -> GpckResult<Option<Vec<ProcessedFile>>> {
    let Some((pixels, dxgi_fmt, width, height, mips)) =
        Ktx2Utils::extract_texture_payload(raw_data)
    else {
        return Ok(None);
    };

    let meta1 = ((width as u32) << 16) | ((height as u32) & 0xFFFF);
    let meta2 = ((mips as u32) << 24) | ((dxgi_fmt & 0xFF) << 16);

    let (conditioned_pixels, gacl_transform) = if options.gacl.enabled {
        apply_conditioning_and_rdo(pixels, dxgi_fmt, width, options, method)?
    } else {
        (pixels, GaclTransform::None)
    };

    let flags =
        FLAG_STREAMING | TYPE_TEXTURE | ((gacl_transform.to_u32() & 0x3F) << SHIFT_GACL_TRANSFORM);

    let chunks = chunker::compress_to_chunks(
        &conditioned_pixels,
        options.chunk_size,
        options.level,
        method,
        options.validate_chunks,
        options.atg_profile,
    )?;

    let mut processed = build_processed_file(ProcessedFileParams {
        rel_path: rel_path.to_string(),
        original_size: conditioned_pixels.len() as u32,
        chunks,
        flags,
        tags: options.tags,
        method,
        alignment: TILE_HARDWARE_ALIGNMENT,
        key: options.key.as_ref(),
    });
    processed.meta1 = meta1;
    processed.meta2 = meta2;

    Ok(Some(vec![processed]))
}

fn process_generic_binary(
    raw_data: &[u8],
    rel_path: &str,
    options: &PackerOptions,
    method: CompressionMethod,
) -> GpckResult<Vec<ProcessedFile>> {
    let chunks = chunker::compress_to_chunks(
        raw_data,
        options.chunk_size,
        options.level,
        method,
        options.validate_chunks,
        options.atg_profile,
    )?;

    let processed = build_processed_file(ProcessedFileParams {
        rel_path: rel_path.to_string(),
        original_size: raw_data.len() as u32,
        chunks,
        flags: FLAG_STREAMING,
        tags: options.tags,
        method,
        alignment: GPU_ALIGNMENT,
        key: options.key.as_ref(),
    });

    Ok(vec![processed])
}

fn apply_conditioning_and_rdo(
    data: Vec<u8>,
    dxgi_fmt: u32,
    width_pixels: usize,
    options: &PackerOptions,
    method: CompressionMethod,
) -> GpckResult<(Vec<u8>, GaclTransform)> {
    let element_size = D3D12FormatTable::get_element_size(dxgi_fmt).unwrap_or(16);
    let aligned_len = (data.len() / element_size) * element_size;
    if aligned_len == 0 {
        return Ok((data, GaclTransform::None));
    }

    let mut pixels = data[..aligned_len].to_vec();
    let trailer = data[aligned_len..].to_vec();
    let mut selected_transform = GaclTransform::None;

    if options.gacl.rdo_reduction_pct > 0.0 && is_rdo_allowed_for_format(dxgi_fmt, &options.gacl) {
        let _ = Gacl::apply_bler(
            &mut pixels,
            dxgi_fmt,
            options.gacl.rdo_reduction_pct / 100.0,
            options.gacl.rdo_use_ycocg,
        );
    }

    if let Some(forced_transform) = select_format_override(dxgi_fmt, &options.gacl) {
        if let Ok(transformed) = Gacl::apply_exact_transform(
            &pixels,
            forced_transform.to_u32(),
            element_size,
            width_pixels,
        ) {
            selected_transform = forced_transform;
            pixels = transformed;
        }
    } else if let Ok((best_conditioned, best_transform)) = Gacl::auto_condition_texture(
        &pixels,
        dxgi_fmt,
        width_pixels,
        method,
        options.level,
        options.atg_profile,
    ) && best_transform != GaclTransform::None.to_u32()
    {
        selected_transform = GaclTransform::from_u32(best_transform);
        pixels = best_conditioned;
    }

    let mut output = Vec::with_capacity(pixels.len() + trailer.len());
    output.extend_from_slice(&pixels);
    output.extend_from_slice(&trailer);

    Ok((output, selected_transform))
}

#[inline(always)]
fn resolve_packer_method(option_method: CompressionMethod, lower_path: &str) -> CompressionMethod {
    match option_method {
        CompressionMethod::Auto => {
            let is_texture = lower_path.ends_with(".dds") || lower_path.ends_with(".ktx2");
            if is_texture && crate::compression::gdeflate::is_gdeflate_available() {
                CompressionMethod::GDeflate
            } else {
                CompressionMethod::Zstd
            }
        }
        m => m,
    }
}

pub fn select_format_override(
    dxgi_fmt: u32,
    overrides: &GaclFormatOverrides,
) -> Option<GaclTransform> {
    if !overrides.enabled || overrides.auto_mode {
        return None;
    }

    let raw_opt = if D3D12FormatTable::is_bc1(dxgi_fmt) {
        overrides.bc1_transform
    } else if D3D12FormatTable::is_bc2(dxgi_fmt) {
        overrides.bc2_transform
    } else if D3D12FormatTable::is_bc3(dxgi_fmt) {
        overrides.bc3_transform
    } else if D3D12FormatTable::is_bc4(dxgi_fmt) {
        overrides.bc4_transform
    } else if D3D12FormatTable::is_bc5(dxgi_fmt) {
        overrides.bc5_transform
    } else if D3D12FormatTable::is_bc6h(dxgi_fmt) {
        overrides.bc6h_transform
    } else if D3D12FormatTable::is_bc7(dxgi_fmt) {
        overrides.bc7_transform
    } else {
        None
    };

    raw_opt.map(GaclTransform::from_u32)
}

#[inline(always)]
fn is_rdo_allowed_for_format(dxgi_fmt: u32, gacl_opts: &GaclFormatOverrides) -> bool {
    match dxgi_fmt {
        crate::graphics::dxgi_format::dxgi::BC1_UNORM
        | crate::graphics::dxgi_format::dxgi::BC1_UNORM_SRGB => gacl_opts.rdo_bc1,
        crate::graphics::dxgi_format::dxgi::BC2_UNORM
        | crate::graphics::dxgi_format::dxgi::BC2_UNORM_SRGB => gacl_opts.rdo_bc2,
        crate::graphics::dxgi_format::dxgi::BC3_UNORM
        | crate::graphics::dxgi_format::dxgi::BC3_UNORM_SRGB => gacl_opts.rdo_bc3,
        crate::graphics::dxgi_format::dxgi::BC4_UNORM
        | crate::graphics::dxgi_format::dxgi::BC4_SNORM => gacl_opts.rdo_bc4,
        crate::graphics::dxgi_format::dxgi::BC5_UNORM
        | crate::graphics::dxgi_format::dxgi::BC5_SNORM => gacl_opts.rdo_bc5,
        crate::graphics::dxgi_format::dxgi::BC6H_UF16
        | crate::graphics::dxgi_format::dxgi::BC6H_SF16 => gacl_opts.rdo_bc6h,
        crate::graphics::dxgi_format::dxgi::BC7_UNORM
        | crate::graphics::dxgi_format::dxgi::BC7_UNORM_SRGB => gacl_opts.rdo_bc7,
        _ => true,
    }
}

pub fn build_processed_file(params: ProcessedFileParams<'_>) -> ProcessedFile {
    let compressed_size = params.chunks.iter().map(|c| c.compressed_size).sum();
    let mut flags = params.flags;

    if params.method != CompressionMethod::Store && compressed_size < params.original_size {
        flags |= FLAG_IS_COMPRESSED;
    }
    flags |= params.method.to_flag_bits();

    if params.key.is_some() {
        flags |= FLAG_ENCRYPTED_META;
    }

    let align_power = (params.alignment as f64).log2() as u32;
    flags |= align_power << SHIFT_ALIGNMENT;

    ProcessedFile {
        asset_id: AssetIdGenerator::generate(&params.rel_path),
        original_path: params.rel_path,
        original_size: params.original_size,
        compressed_size,
        flags,
        tags: params.tags,
        partition_id: 0,
        alignment: params.alignment,
        meta1: 0,
        meta2: 0,
        chunks: params.chunks,
        sub_chunk_offset: 0,
        sub_chunk_size: 0,
    }
}
