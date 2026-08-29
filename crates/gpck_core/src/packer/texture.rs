// crates/gpck_core/src/packer/texture.rs
//! # Pipeline Stage 2: Texture Conditioning, Mip-Splitting & Hardware Tile Packaging

use crate::compression::codecs::CompressionMethod;
use crate::core::error::GpckResult;
use crate::format::archive::{
    FLAG_BOOT_TAIL, TYPE_NEURAL_TEXTURE, TYPE_TEXTURE, TYPE_TILED_RESOURCE,
};
use crate::format::dds::DdsUtils;
use crate::format::ktx2::Ktx2Utils;
use crate::gacl::{Gacl, GaclTransform};
use crate::graphics::dxgi_format::D3D12FormatTable;
use crate::packer::chunker;
use crate::packer::tiler::{TileSliceResult, TiledTexturePacker, get_linear_transform};
use crate::packer::types::{
    GaclFormatOverrides, PackerOptions, ProcessedFile, ProcessedFileBuilder,
    TextureConditioningResult, TextureMetadata,
};
use std::fs;
use std::path::Path;

const GPU_ALIGNMENT: i64 = 4096;
const TILE_HARDWARE_ALIGNMENT: i64 = 65536;

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

    // Direct Neural Texture Container (.gntc / .ntex)
    if lower_path.ends_with(".gntc") || lower_path.ends_with(".ntex") {
        return process_neural_texture_container(&raw_data, rel_path, options, method);
    }

    // DirectDraw Surface (.dds)
    if lower_path.ends_with(".dds")
        && let Some(processed) = process_dds_texture(&raw_data, rel_path, options, method)?
    {
        return Ok(processed);
    }

    // Khronos Texture 2.0 (.ktx2)
    if lower_path.ends_with(".ktx2")
        && let Some(processed) = process_ktx2_texture(&raw_data, rel_path, options, method)?
    {
        return Ok(processed);
    }

    process_generic_binary(&raw_data, rel_path, options, method)
}

fn process_neural_texture_container(
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

    let meta1 = (2048 << 16) | 2048;
    let meta2 = (1u32 << 24) | (chunks.len() as u32 & 0xFFFF);

    let processed = ProcessedFileBuilder::new(rel_path, raw_data.len() as u32, method)
        .chunks(chunks)
        .flags(TYPE_NEURAL_TEXTURE | TYPE_TILED_RESOURCE)
        .metadata(meta1, meta2)
        .tags(options.tags)
        .alignment(TILE_HARDWARE_ALIGNMENT)
        .encryption_key(options.key.as_ref())
        .build();

    Ok(vec![processed])
}

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
    let meta = TextureMetadata::new(
        h_info.width as u32,
        h_info.height as u32,
        h_info.mip_count as u32,
        dxgi_fmt,
        header_len,
    );

    // Check tiled virtual texturing threshold
    let passes_res_threshold = if options.min_tiled_resolution == 0 {
        meta.width >= 128 && meta.height >= 128
    } else {
        meta.max_dimension() >= options.min_tiled_resolution as u32
    };

    if options.tiled_streaming && meta.dxgi_format > 0 && passes_res_threshold {
        let mut tile_options = options.clone();
        tile_options.method = method;

        let tile_res =
            TiledTexturePacker::slice_and_compress_texture_tiles(raw_data, &meta, &tile_options)?;

        if tile_res.total_tiles >= options.min_tiled_tile_count {
            return process_tiled_dds_from_result(tile_res, &meta, rel_path, &tile_options, method)
                .map(Some);
        }
    }

    // Fallback: Mip-Split layout for sub-threshold textures
    process_mipsplit_dds(raw_data, &meta, rel_path, options, method).map(Some)
}

fn process_tiled_dds_from_result(
    tile_res: TileSliceResult,
    meta: &TextureMetadata,
    rel_path: &str,
    options: &PackerOptions,
    method: CompressionMethod,
) -> GpckResult<Vec<ProcessedFile>> {
    let mut out_files = Vec::new();

    let tail_w = (meta.width >> tile_res.num_standard_mips).max(1);
    let tail_h = (meta.height >> tile_res.num_standard_mips).max(1);
    let tail_meta1 = (tail_w << 16) | (tail_h & 0xFFFF);
    let tail_meta2 = ((tile_res.num_packed_mips & 0xFF) << 24)
        | ((meta.dxgi_format & 0xFF) << 16)
        | (tile_res.tail_chunks.len() as u32 & 0xFFFF);

    // CASE 1: Texture has ONLY Tail tiles (num_standard_mips == 0, e.g. 512x128, 128x512, 256x256)
    // Emit directly under real name into Partition 0 without creating 0-byte ghost files!
    if tile_res.standard_chunks.is_empty() {
        if !tile_res.tail_chunks.is_empty() {
            let tail_file = ProcessedFileBuilder::new(
                rel_path,
                (tile_res.tail_chunks.len() * 65536) as u32,
                method,
            )
            .chunks(tile_res.tail_chunks)
            .flags(TYPE_TEXTURE | TYPE_TILED_RESOURCE | FLAG_BOOT_TAIL)
            .gacl_transform(get_linear_transform(tile_res.gacl_transform))
            .metadata(meta.meta1(), tail_meta2)
            .partition_id(0)
            .tags(options.tags)
            .alignment(TILE_HARDWARE_ALIGNMENT)
            .encryption_key(options.key.as_ref())
            .build();

            out_files.push(tail_file);
        }
        return Ok(out_files);
    }

    // CASE 2: Texture has BOTH Standard tiles and Tail tiles
    if !tile_res.tail_chunks.is_empty() {
        let tail_file = ProcessedFileBuilder::new(
            format!("{}.tail", rel_path),
            (tile_res.tail_chunks.len() * 65536) as u32,
            method,
        )
        .chunks(tile_res.tail_chunks)
        .flags(TYPE_TEXTURE | TYPE_TILED_RESOURCE | FLAG_BOOT_TAIL)
        .gacl_transform(get_linear_transform(tile_res.gacl_transform))
        .metadata(tail_meta1, tail_meta2)
        .partition_id(0)
        .tags(options.tags)
        .alignment(TILE_HARDWARE_ALIGNMENT)
        .encryption_key(options.key.as_ref())
        .build();

        out_files.push(tail_file);
    }

    let standard_meta1 = meta.meta1();
    let standard_meta2 = ((tile_res.num_standard_mips & 0xFF) << 24)
        | ((meta.dxgi_format & 0xFF) << 16)
        | (tile_res.standard_chunks.len() as u32 & 0xFFFF);

    let standard_file = ProcessedFileBuilder::new(
        rel_path,
        (tile_res.standard_chunks.len() * 65536) as u32,
        method,
    )
    .chunks(tile_res.standard_chunks)
    .flags(TYPE_TEXTURE | TYPE_TILED_RESOURCE)
    .gacl_transform(tile_res.gacl_transform)
    .metadata(standard_meta1, standard_meta2)
    .tags(options.tags)
    .alignment(TILE_HARDWARE_ALIGNMENT)
    .encryption_key(options.key.as_ref())
    .build();

    out_files.push(standard_file);
    Ok(out_files)
}

fn process_mipsplit_dds(
    raw_data: &[u8],
    meta: &TextureMetadata,
    rel_path: &str,
    options: &PackerOptions,
    method: CompressionMethod,
) -> GpckResult<Vec<ProcessedFile>> {
    let mut base_data;
    let mut highres_data = Vec::new();
    let mut sub_meta2 = meta.meta2(0);

    let base_width = if options.mip_split {
        let (processed_data, t_size) =
            DdsUtils::process_texture_for_streaming(raw_data, options.max_tail_dim);
        base_data = processed_data[0..t_size].to_vec();
        highres_data = processed_data[t_size..].to_vec();
        sub_meta2 = meta.meta2(t_size as u32);
        (options.max_tail_dim as u32).min(meta.width)
    } else {
        base_data = raw_data.to_vec();
        meta.width
    };

    let mut gacl_transform = GaclTransform::None;
    if options.gacl.enabled && meta.dxgi_format > 0 && base_data.len() > meta.header_length {
        let header = base_data[..meta.header_length].to_vec();
        let payload = base_data[meta.header_length..].to_vec();
        let result = apply_conditioning_and_rdo(
            payload,
            meta.dxgi_format,
            base_width as usize,
            options,
            method,
        )?;
        gacl_transform = result.transform;

        let mut combined = Vec::with_capacity(header.len() + result.payload.len());
        combined.extend_from_slice(&header);
        combined.extend_from_slice(&result.payload);
        base_data = combined;
    }

    let mut high_gacl_transform = gacl_transform;
    if options.gacl.enabled && meta.dxgi_format > 0 && !highres_data.is_empty() {
        let result = apply_conditioning_and_rdo(
            highres_data,
            meta.dxgi_format,
            meta.width as usize,
            options,
            method,
        )?;
        high_gacl_transform = result.transform;
        highres_data = result.payload;
    }

    let base_chunks = chunker::compress_to_chunks(
        &base_data,
        options.chunk_size,
        options.level,
        method,
        options.validate_chunks,
        options.atg_profile,
    )?;

    let mut out_files = Vec::new();
    let base_file = ProcessedFileBuilder::new(rel_path, base_data.len() as u32, method)
        .chunks(base_chunks)
        .flags(TYPE_TEXTURE)
        .gacl_transform(gacl_transform)
        .metadata(meta.meta1(), sub_meta2)
        .tags(options.tags)
        .alignment(TILE_HARDWARE_ALIGNMENT)
        .encryption_key(options.key.as_ref())
        .build();

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

        let highres_file = ProcessedFileBuilder::new(
            format!("{}.highmips", rel_path),
            highres_data.len() as u32,
            method,
        )
        .chunks(highres_chunks)
        .flags(TYPE_TEXTURE)
        .gacl_transform(high_gacl_transform)
        .metadata(meta.meta1(), sub_meta2)
        .tags(options.tags)
        .alignment(TILE_HARDWARE_ALIGNMENT)
        .encryption_key(options.key.as_ref())
        .build();

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

    let meta = TextureMetadata::new(width as u32, height as u32, mips as u32, dxgi_fmt, 80);

    let (conditioned_pixels, gacl_transform) = if options.gacl.enabled {
        let res = apply_conditioning_and_rdo(pixels, dxgi_fmt, width, options, method)?;
        (res.payload, res.transform)
    } else {
        (pixels, GaclTransform::None)
    };

    let chunks = chunker::compress_to_chunks(
        &conditioned_pixels,
        options.chunk_size,
        options.level,
        method,
        options.validate_chunks,
        options.atg_profile,
    )?;

    let processed = ProcessedFileBuilder::new(rel_path, conditioned_pixels.len() as u32, method)
        .chunks(chunks)
        .flags(TYPE_TEXTURE)
        .gacl_transform(gacl_transform)
        .metadata(meta.meta1(), meta.meta2(0))
        .tags(options.tags)
        .alignment(TILE_HARDWARE_ALIGNMENT)
        .encryption_key(options.key.as_ref())
        .build();

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

    let processed = ProcessedFileBuilder::new(rel_path, raw_data.len() as u32, method)
        .chunks(chunks)
        .tags(options.tags)
        .alignment(GPU_ALIGNMENT)
        .encryption_key(options.key.as_ref())
        .build();

    Ok(vec![processed])
}

pub fn apply_conditioning_and_rdo(
    data: Vec<u8>,
    dxgi_fmt: u32,
    width_pixels: usize,
    options: &PackerOptions,
    method: CompressionMethod,
) -> GpckResult<TextureConditioningResult> {
    let element_size = D3D12FormatTable::get_element_size(dxgi_fmt).unwrap_or(16);
    let aligned_len = (data.len() / element_size) * element_size;
    if aligned_len == 0 {
        return Ok(TextureConditioningResult {
            payload: data,
            transform: GaclTransform::None,
            space_curve_applied: false,
        });
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

    Ok(TextureConditioningResult {
        payload: output,
        transform: selected_transform,
        space_curve_applied: selected_transform.has_space_curve(),
    })
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
