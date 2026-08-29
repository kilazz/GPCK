// crates/gpck_gui/src/preview.rs
//! # Asset Preview Generation, Tonemapping, RGBA Channel Masking & Native Mipmap Viewer

use crate::AppWindow;
use anyhow::{Result, anyhow, bail};
use gpck_core::compression::ntc::{GNTC_MAGIC, GntcHeader, NTEX_MAGIC, NtcContext};
use gpck_core::format::archive::{
    FLAG_BOOT_TAIL, FileEntry, GameArchive, TYPE_NEURAL_TEXTURE, TYPE_TILED_RESOURCE,
};
use gpck_core::gacl::GaclTransform;
use gpck_core::graphics::dxgi_format::{D3D12FormatTable, dxgi};
use gpck_core::graphics::recombine::TextureRecombiner;
use gpck_core::graphics::tonemap::{TonemapOperator, apply_tonemap_to_rgba8};
use gpck_core::packer::tiler::get_linear_transform;
use slint::{ComponentHandle, Image, ModelRc, SharedPixelBuffer, SharedString, VecModel};
use std::fs;
use std::path::PathBuf;

pub enum PreviewResult {
    PixelBuffer {
        buffer: SharedPixelBuffer<slint::Rgba8Pixel>,
        mips: Vec<String>,
    },
    Text(String),
}

pub fn trigger_async_preview(
    ui: &AppWindow,
    arch_path: Option<PathBuf>,
    rel_path: String,
    local_path: Option<PathBuf>,
) {
    ui.set_show_image(false);
    ui.set_preview_text(SharedString::from("Loading asset data..."));
    ui.set_is_preview_loading(true);

    let ui_weak = ui.as_weak();
    let decondition = ui.get_decondition_gacl_preview();
    let reconstruct_normal_z = ui.get_reconstruct_normal_z();
    let show_tile_grid = ui.get_show_tile_grid();
    let tonemap_operator = TonemapOperator::from_index(ui.get_tonemap_mode_index());
    let target_mip = ui.get_selected_mip_level().max(0) as u32;

    let channel_mask = [
        ui.get_channel_r(),
        ui.get_channel_g(),
        ui.get_channel_b(),
        ui.get_channel_a(),
    ];

    std::thread::spawn(move || {
        let result = (|| -> Result<PreviewResult> {
            let (data, meta1, meta2, flags) = if let Some(arch_p) = arch_path.as_ref() {
                let archive = GameArchive::open(arch_p)?;
                let id = gpck_core::core::asset_id::AssetIdGenerator::generate(&rel_path);
                let entry = archive
                    .try_get_entry(id)
                    .ok_or_else(|| anyhow!("Asset not found in archive"))?;
                let data = archive.read_asset(&entry)?;
                (data, entry.meta1, entry.meta2, entry.flags)
            } else if let Some(local_p) = local_path {
                let data = fs::read(&local_p)?;
                (data, 0, 0, 0)
            } else {
                bail!("No archive or local file available to read");
            };

            let width = ((meta1 >> 16) & 0xFFFF) as usize;
            let height = (meta1 & 0xFFFF) as usize;
            let total_mips = ((meta2 >> 24) & 0xFF).max(1) as usize;
            let ext = rel_path.split('.').next_back().unwrap_or("").to_lowercase();
            let is_tail = rel_path.ends_with(".tail") || (flags & FLAG_BOOT_TAIL) != 0;
            let is_highmips = rel_path.ends_with(".highmips");
            let is_neural_container = ext == "gntc"
                || ext == "ntex"
                || (flags & TYPE_NEURAL_TEXTURE) != 0
                || (data.len() >= 4
                    && (u32::from_le_bytes(data[0..4].try_into().unwrap_or_default())
                        == GNTC_MAGIC
                        || u32::from_le_bytes(data[0..4].try_into().unwrap_or_default())
                            == NTEX_MAGIC));

            // ================================================================
            // 1. Dedicated Neural PBR Material Preview (.gntc / .ntex) - Native 1:1 Resolution
            // ================================================================
            if is_neural_container {
                let header_size = std::mem::size_of::<GntcHeader>();
                let (native_w, native_h) = if data.len() >= header_size {
                    let header: &GntcHeader = bytemuck::from_bytes(&data[..header_size]);
                    (header.width.max(64), header.height.max(64))
                } else {
                    (width.max(512) as u32, height.max(512) as u32)
                };

                if let Ok(pbr) = NtcContext::decode_gntc_preview(&data, native_w) {
                    let mip_options = vec![
                        format!("Quad-View (2x2 Matrix {}x{})", native_w * 2, native_h * 2),
                        format!("Albedo (Base Color {}x{})", native_w, native_h),
                        format!("Normal Map (Tangent {}x{})", native_w, native_h),
                        format!("ORM Map (Damage/Rough/Metal {}x{})", native_w, native_h),
                        format!("Roughness Channel ({}x{})", native_w, native_h),
                    ];

                    let pixel_buf = match target_mip {
                        // View 1: Standalone Albedo (Native W x H)
                        1 => {
                            let mut buf =
                                SharedPixelBuffer::<slint::Rgba8Pixel>::new(pbr.width, pbr.height);
                            let dst: &mut [u8] = bytemuck::cast_slice_mut(buf.make_mut_slice());
                            dst.copy_from_slice(&pbr.albedo);
                            apply_tonemap_to_rgba8(dst, tonemap_operator);
                            apply_channel_mask(
                                dst,
                                channel_mask[0],
                                channel_mask[1],
                                channel_mask[2],
                                channel_mask[3],
                            );
                            buf
                        }
                        // View 2: Standalone Normal Map (Native W x H)
                        2 => {
                            let mut buf =
                                SharedPixelBuffer::<slint::Rgba8Pixel>::new(pbr.width, pbr.height);
                            let dst: &mut [u8] = bytemuck::cast_slice_mut(buf.make_mut_slice());
                            dst.copy_from_slice(&pbr.normal);
                            apply_channel_mask(
                                dst,
                                channel_mask[0],
                                channel_mask[1],
                                channel_mask[2],
                                channel_mask[3],
                            );
                            buf
                        }
                        // View 3: Standalone ORM Map (Native W x H)
                        3 => {
                            let mut buf =
                                SharedPixelBuffer::<slint::Rgba8Pixel>::new(pbr.width, pbr.height);
                            let dst: &mut [u8] = bytemuck::cast_slice_mut(buf.make_mut_slice());
                            dst.copy_from_slice(&pbr.orm);
                            apply_channel_mask(
                                dst,
                                channel_mask[0],
                                channel_mask[1],
                                channel_mask[2],
                                channel_mask[3],
                            );
                            buf
                        }
                        // View 4: Standalone Roughness Channel (Native W x H Grayscale)
                        4 => {
                            let mut buf =
                                SharedPixelBuffer::<slint::Rgba8Pixel>::new(pbr.width, pbr.height);
                            let dst: &mut [u8] = bytemuck::cast_slice_mut(buf.make_mut_slice());
                            for (src_px, dst_px) in
                                pbr.orm.chunks_exact(4).zip(dst.chunks_exact_mut(4))
                            {
                                let r_val = src_px[1]; // Roughness
                                dst_px[0] = r_val;
                                dst_px[1] = r_val;
                                dst_px[2] = r_val;
                                dst_px[3] = 255;
                            }
                            buf
                        }
                        // View 0 (Default): Quad-View 2x2 Matrix (2*W x 2*H)
                        _ => {
                            let qw = native_w * 2;
                            let qh = native_h * 2;
                            let mut buf = SharedPixelBuffer::<slint::Rgba8Pixel>::new(qw, qh);
                            let dst: &mut [u8] = bytemuck::cast_slice_mut(buf.make_mut_slice());

                            let nw = native_w as usize;
                            let nh = native_h as usize;
                            let q_stride = qw as usize;

                            for y in 0..nh {
                                for x in 0..nw {
                                    let src_idx = (y * nw + x) * 4;

                                    // Top-Left: Albedo (0..nw, 0..nh)
                                    let tl_idx = (y * q_stride + x) * 4;
                                    dst[tl_idx..tl_idx + 4]
                                        .copy_from_slice(&pbr.albedo[src_idx..src_idx + 4]);

                                    // Top-Right: Normal (nw..2nw, 0..nh)
                                    let tr_idx = (y * q_stride + (x + nw)) * 4;
                                    dst[tr_idx..tr_idx + 4]
                                        .copy_from_slice(&pbr.normal[src_idx..src_idx + 4]);

                                    // Bottom-Left: ORM (0..nw, nh..2nh)
                                    let bl_idx = ((y + nh) * q_stride + x) * 4;
                                    dst[bl_idx..bl_idx + 4]
                                        .copy_from_slice(&pbr.orm[src_idx..src_idx + 4]);

                                    // Bottom-Right: Roughness (nw..2nw, nh..2nh)
                                    let br_idx = ((y + nh) * q_stride + (x + nw)) * 4;
                                    let r_val = pbr.orm[src_idx + 1];
                                    dst[br_idx..br_idx + 4]
                                        .copy_from_slice(&[r_val, r_val, r_val, 255]);
                                }
                            }

                            // Dynamic Cyan Grid Dividers
                            for y in 0..qh as usize {
                                let idx = (y * q_stride + (nw - 1)) * 4;
                                dst[idx..idx + 4].copy_from_slice(&[56, 189, 248, 255]);
                                let idx2 = (y * q_stride + nw) * 4;
                                dst[idx2..idx2 + 4].copy_from_slice(&[56, 189, 248, 255]);
                            }
                            for x in 0..qw as usize {
                                let idx = ((nh - 1) * q_stride + x) * 4;
                                dst[idx..idx + 4].copy_from_slice(&[56, 189, 248, 255]);
                                let idx2 = (nh * q_stride + x) * 4;
                                dst[idx2..idx2 + 4].copy_from_slice(&[56, 189, 248, 255]);
                            }

                            apply_channel_mask(
                                dst,
                                channel_mask[0],
                                channel_mask[1],
                                channel_mask[2],
                                channel_mask[3],
                            );
                            buf
                        }
                    };

                    return Ok(PreviewResult::PixelBuffer {
                        buffer: pixel_buf,
                        mips: mip_options,
                    });
                }
            }

            let dummy_entry = FileEntry {
                asset_id: [0; 16],
                data_offset: 0,
                chunk_table_offset: 0,
                name_offset: 0,
                compressed_size: data.len() as u32,
                original_size: data.len() as u32,
                flags,
                meta1,
                meta2,
                tags: 0,
                partition_id: 0,
                chunk_count: 1,
                sub_chunk_offset: 0,
                sub_chunk_size: 0,
            };

            let mut mip_labels = Vec::new();
            if width > 0 && height > 0 {
                for m in 0..total_mips {
                    let mw = (width >> m).max(1);
                    let mh = (height >> m).max(1);
                    if is_tail {
                        mip_labels.push(format!("Tail Mip {} ({}x{})", m, mw, mh));
                    } else if is_highmips {
                        mip_labels.push(format!("High Mip {} ({}x{})", m, mw, mh));
                    } else {
                        mip_labels.push(format!("Mip {} ({}x{})", m, mw, mh));
                    }
                }
            } else {
                mip_labels.push("Mip 0 (Full)".to_string());
            }

            // Automatic Highmips Recombination with paired Base DDS
            if is_highmips
                && let Some(arch_p) = arch_path.as_ref()
                && let Ok(archive) = GameArchive::open(arch_p)
            {
                let base_rel_path = rel_path.trim_end_matches(".highmips");
                let base_id = gpck_core::core::asset_id::AssetIdGenerator::generate(base_rel_path);
                if let Some(base_entry) = archive.try_get_entry(base_id)
                    && let Ok(base_raw) = archive.read_asset(&base_entry)
                    && let Ok(full_dds) = TextureRecombiner::recombine_dds(
                        base_rel_path,
                        &base_raw,
                        Some(&data),
                        &base_entry,
                        dummy_entry.gacl_transform(),
                        decondition,
                    )
                    && let Some(buf) = load_texture_to_buffer(
                        &full_dds,
                        "dds",
                        base_rel_path,
                        &base_entry,
                        target_mip,
                        tonemap_operator,
                        show_tile_grid,
                        decondition,
                        reconstruct_normal_z,
                        channel_mask,
                    )
                {
                    return Ok(PreviewResult::PixelBuffer {
                        buffer: buf,
                        mips: mip_labels,
                    });
                }
            }

            if (matches!(
                ext.as_str(),
                "png" | "jpg" | "jpeg" | "bmp" | "dds" | "ktx2" | "highmips" | "tail"
            ) || is_highmips
                || is_tail
                || meta1 > 0)
                && let Some(buf) = load_texture_to_buffer(
                    &data,
                    &ext,
                    &rel_path,
                    &dummy_entry,
                    target_mip,
                    tonemap_operator,
                    show_tile_grid,
                    decondition,
                    reconstruct_normal_z,
                    channel_mask,
                )
            {
                return Ok(PreviewResult::PixelBuffer {
                    buffer: buf,
                    mips: mip_labels,
                });
            }

            let preview_str = match std::str::from_utf8(&data) {
                Ok(valid_text) => {
                    let char_count = valid_text.chars().count();
                    if char_count > 2000 {
                        let truncated: String = valid_text.chars().take(2000).collect();
                        format!(
                            "{}\n\n... [Truncated: showing 2000 of {} characters ({} bytes total)]",
                            truncated,
                            char_count,
                            data.len()
                        )
                    } else {
                        valid_text.to_string()
                    }
                }
                Err(_) => {
                    let hex_dump = format_hex_dump(&data, 512);
                    format!(
                        "[Binary Asset File: {} bytes]\nDimensions: {}x{}\nAsset Hash: {:016X}\n\nHEX VIEW (First 512 bytes):\n{}",
                        data.len(),
                        width,
                        height,
                        twox_hash::XxHash64::oneshot(0, &data),
                        hex_dump
                    )
                }
            };

            Ok(PreviewResult::Text(preview_str))
        })();

        slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_is_preview_loading(false);
                match result {
                    Ok(PreviewResult::PixelBuffer { buffer, mips }) => {
                        let slint_mips: Vec<SharedString> =
                            mips.into_iter().map(SharedString::from).collect();
                        ui.set_available_mips(ModelRc::new(VecModel::from(slint_mips)));
                        ui.set_preview_image(Image::from_rgba8(buffer));
                        ui.set_show_image(true);
                    }
                    Ok(PreviewResult::Text(txt)) => {
                        ui.set_preview_text(SharedString::from(txt));
                        ui.set_show_image(false);
                    }
                    Err(e) => {
                        ui.set_preview_text(SharedString::from(format!(
                            "Error reading asset: {}",
                            e
                        )));
                        ui.set_show_image(false);
                    }
                }
            }
        })
        .ok();
    });
}

pub fn apply_channel_mask(pixels: &mut [u8], r: bool, g: bool, b: bool, a: bool) {
    let active_color_count = (r as u8) + (g as u8) + (b as u8);

    if active_color_count == 1 && !a {
        for px in pixels.chunks_exact_mut(4) {
            let val = if r {
                px[0]
            } else if g {
                px[1]
            } else {
                px[2]
            };
            px[0] = val;
            px[1] = val;
            px[2] = val;
            px[3] = 255;
        }
        return;
    }

    if active_color_count == 0 && a {
        for px in pixels.chunks_exact_mut(4) {
            let val = px[3];
            px[0] = val;
            px[1] = val;
            px[2] = val;
            px[3] = 255;
        }
        return;
    }

    for px in pixels.chunks_exact_mut(4) {
        if !r {
            px[0] = 0;
        }
        if !g {
            px[1] = 0;
        }
        if !b {
            px[2] = 0;
        }
        if !a {
            px[3] = 255;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn load_texture_to_buffer(
    data: &[u8],
    ext: &str,
    rel_path: &str,
    entry: &FileEntry,
    target_mip: u32,
    tonemap_operator: TonemapOperator,
    show_tile_grid: bool,
    decondition_gacl: bool,
    reconstruct_normal_z: bool,
    channel_mask: [bool; 4],
) -> Option<SharedPixelBuffer<slint::Rgba8Pixel>> {
    let mut width = (entry.meta1 >> 16) & 0xFFFF;
    let mut height = entry.meta1 & 0xFFFF;
    let gacl_fmt = entry.gacl_transform();
    let mut dxgi_fmt = (entry.meta2 >> 16) & 0xFF;

    if (width == 0 || height == 0 || dxgi_fmt == 0)
        && data.len() >= 128
        && data.starts_with(b"DDS ")
    {
        if let Some(h) = gpck_core::format::dds::DdsUtils::get_header_info(data) {
            if width == 0 {
                width = h.width as u32;
            }
            if height == 0 {
                height = h.height as u32;
            }
        }
        if dxgi_fmt == 0 {
            let (detected, _) = gpck_core::format::dds::DdsUtils::detect_dxgi_format(data);
            dxgi_fmt = detected;
        }
    }

    if dxgi_fmt == 0 {
        if gacl_fmt != 0 {
            dxgi_fmt = GaclTransform::from_u32(gacl_fmt).to_dxgi_format();
        } else {
            let name_lower = rel_path.to_lowercase();
            dxgi_fmt = if name_lower.contains("_ddna")
                || name_lower.contains("_ddn")
                || name_lower.contains("_norm")
            {
                dxgi::BC5_UNORM
            } else if name_lower.contains("_gloss")
                || name_lower.contains("_rough")
                || name_lower.contains("_height")
                || name_lower.contains("_disp")
            {
                dxgi::BC4_UNORM
            } else {
                dxgi::BC7_UNORM
            };
        }
    }

    // Direct Uncompressed RGB / RGBA / BGRA Decoder
    if data.len() >= 128 && data.starts_with(b"DDS ") && width > 0 && height > 0 {
        let total_pixels = (width as usize) * (height as usize);

        if (dxgi_fmt == dxgi::B8G8R8A8_UNORM || dxgi_fmt == dxgi::B8G8R8A8_UNORM_SRGB)
            && data.len() >= 128 + total_pixels * 4
        {
            let raw_pixels = &data[128..128 + total_pixels * 4];
            let mut pixel_buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
            let dst_bytes: &mut [u8] = bytemuck::cast_slice_mut(pixel_buffer.make_mut_slice());

            for (src_px, dst_px) in raw_pixels
                .chunks_exact(4)
                .zip(dst_bytes.chunks_exact_mut(4))
            {
                dst_px[0] = src_px[2];
                dst_px[1] = src_px[1];
                dst_px[2] = src_px[0];
                dst_px[3] = src_px[3];
            }

            apply_tonemap_to_rgba8(dst_bytes, tonemap_operator);
            apply_channel_mask(
                dst_bytes,
                channel_mask[0],
                channel_mask[1],
                channel_mask[2],
                channel_mask[3],
            );
            return Some(pixel_buffer);
        }

        if (dxgi_fmt == dxgi::R8G8B8A8_UNORM || dxgi_fmt == dxgi::R8G8B8A8_UNORM_SRGB)
            && data.len() >= 128 + total_pixels * 4
        {
            let raw_pixels = &data[128..128 + total_pixels * 4];
            let mut pixel_buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
            let dst_bytes: &mut [u8] = bytemuck::cast_slice_mut(pixel_buffer.make_mut_slice());
            dst_bytes.copy_from_slice(raw_pixels);

            apply_tonemap_to_rgba8(dst_bytes, tonemap_operator);
            apply_channel_mask(
                dst_bytes,
                channel_mask[0],
                channel_mask[1],
                channel_mask[2],
                channel_mask[3],
            );
            return Some(pixel_buffer);
        }

        if (dxgi_fmt == dxgi::B8G8R8X8_UNORM || dxgi_fmt == dxgi::B8G8R8X8_UNORM_SRGB)
            && data.len() >= 128 + total_pixels * 3
        {
            let raw_pixels = &data[128..128 + total_pixels * 3];
            let mut pixel_buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
            let dst_bytes: &mut [u8] = bytemuck::cast_slice_mut(pixel_buffer.make_mut_slice());

            for (src_px, dst_px) in raw_pixels
                .chunks_exact(3)
                .zip(dst_bytes.chunks_exact_mut(4))
            {
                dst_px[0] = src_px[2];
                dst_px[1] = src_px[1];
                dst_px[2] = src_px[0];
                dst_px[3] = 255;
            }

            apply_tonemap_to_rgba8(dst_bytes, tonemap_operator);
            apply_channel_mask(
                dst_bytes,
                channel_mask[0],
                channel_mask[1],
                channel_mask[2],
                channel_mask[3],
            );
            return Some(pixel_buffer);
        }
    }

    let is_tail = rel_path.ends_with(".tail") || (entry.flags & FLAG_BOOT_TAIL) != 0;
    let is_tiled = (entry.flags & TYPE_TILED_RESOURCE) != 0;

    let processed_dds_bytes = if is_tail && width > 0 && height > 0 {
        let (tail_mip_bytes, tw, th) = extract_tail_mip(
            data,
            width,
            height,
            target_mip,
            gacl_fmt,
            dxgi_fmt,
            decondition_gacl,
        )?;
        TextureRecombiner::wrap_in_dds_header(tw, th, dxgi_fmt, &tail_mip_bytes)
    } else if is_tiled && width > 0 && height > 0 {
        let (linear_bytes, mw, mh) = detile_specific_mip(
            data,
            width,
            height,
            target_mip,
            gacl_fmt,
            dxgi_fmt,
            decondition_gacl,
        )?;
        TextureRecombiner::wrap_in_dds_header(mw, mh, dxgi_fmt, &linear_bytes)
    } else if !data.starts_with(b"DDS ") && width > 0 && height > 0 {
        let clean_payload = TextureRecombiner::unshuffle_payload(
            "texture.dds",
            data,
            gacl_fmt,
            width as usize,
            decondition_gacl,
        );
        TextureRecombiner::wrap_in_dds_header(width, height, dxgi_fmt, &clean_payload)
    } else if data.starts_with(b"DDS ") {
        TextureRecombiner::unshuffle_payload(
            "texture.dds",
            data,
            gacl_fmt,
            width as usize,
            decondition_gacl,
        )
    } else {
        data.to_vec()
    };

    let rgba_img = if ext == "dds" || ext == "tail" || processed_dds_bytes.starts_with(b"DDS ") {
        if let Ok(dds) = image_dds::ddsfile::Dds::read(std::io::Cursor::new(&processed_dds_bytes)) {
            image_dds::image_from_dds(&dds, 0).ok()
        } else {
            image::load_from_memory_with_format(&processed_dds_bytes, image::ImageFormat::Dds)
                .ok()?
                .to_rgba8()
                .into()
        }
    } else if ext == "ktx2" {
        if let Some((payload, detected_dxgi, w, h, _)) =
            gpck_core::format::ktx2::Ktx2Utils::extract_texture_payload(&processed_dds_bytes)
        {
            let full_dds_bytes =
                TextureRecombiner::wrap_in_dds_header(w as u32, h as u32, detected_dxgi, &payload);
            if let Ok(dds) = image_dds::ddsfile::Dds::read(std::io::Cursor::new(&full_dds_bytes)) {
                image_dds::image_from_dds(&dds, 0).ok()
            } else {
                None
            }
        } else {
            None
        }
    } else {
        image::load_from_memory(&processed_dds_bytes)
            .ok()?
            .to_rgba8()
            .into()
    }?;

    let (img_w, img_h) = rgba_img.dimensions();
    let mut pixel_buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::new(img_w, img_h);

    let raw_bytes: &mut [u8] = bytemuck::cast_slice_mut(pixel_buffer.make_mut_slice());
    raw_bytes.copy_from_slice(rgba_img.as_raw());

    let is_two_channel = dxgi_fmt == dxgi::BC5_UNORM
        || dxgi_fmt == dxgi::BC5_SNORM
        || dxgi_fmt == dxgi::R8G8_UNORM
        || dxgi_fmt == dxgi::R8G8_SNORM;

    if is_two_channel {
        for px in raw_bytes.chunks_exact_mut(4) {
            if reconstruct_normal_z {
                let nx = (px[0] as f32 / 255.0) * 2.0 - 1.0;
                let ny = (px[1] as f32 / 255.0) * 2.0 - 1.0;
                let nz = (1.0f32 - nx * nx - ny * ny).max(0.0).sqrt();
                px[2] = (nz * 255.0).clamp(0.0, 255.0) as u8;
            } else {
                px[2] = 0;
            }
            px[3] = 255;
        }
    }

    apply_tonemap_to_rgba8(raw_bytes, tonemap_operator);
    apply_channel_mask(
        raw_bytes,
        channel_mask[0],
        channel_mask[1],
        channel_mask[2],
        channel_mask[3],
    );

    if show_tile_grid && img_w >= 128 && img_h >= 128 && !is_tail {
        let block_size = D3D12FormatTable::get_element_size(dxgi_fmt).unwrap_or(16);
        let (tile_w, tile_h) = if block_size == 8 {
            (512usize, 256usize)
        } else {
            (256usize, 256usize)
        };

        for y in 0..img_h as usize {
            for x in 0..img_w as usize {
                let is_border_x = (x % tile_w == 0) || ((x + 1) % tile_w == 0);
                let is_border_y = (y % tile_h == 0) || ((y + 1) % tile_h == 0);

                if is_border_x || is_border_y {
                    let pixel_idx = (y * img_w as usize + x) * 4;
                    raw_bytes[pixel_idx] = ((raw_bytes[pixel_idx] as u32 * 3) / 4) as u8;
                    raw_bytes[pixel_idx + 1] =
                        ((raw_bytes[pixel_idx + 1] as u32 * 2 + 229 * 2) / 4) as u8;
                    raw_bytes[pixel_idx + 2] =
                        ((raw_bytes[pixel_idx + 2] as u32 + 255 * 3) / 4) as u8;
                }
            }
        }
    }

    Some(pixel_buffer)
}

fn extract_tail_mip(
    tail_data: &[u8],
    tail_base_width: u32,
    tail_base_height: u32,
    target_tail_mip: u32,
    gacl_fmt: u32,
    dxgi_fmt: u32,
    decondition_gacl: bool,
) -> Option<(Vec<u8>, u32, u32)> {
    let element_size = D3D12FormatTable::get_element_size(dxgi_fmt).unwrap_or(16);
    let tile_size = 65536usize;
    let mut working_tail = tail_data.to_vec();

    if working_tail.len() < tile_size {
        working_tail.resize(tile_size, 0);
    }

    let transform = GaclTransform::from_u32(gacl_fmt);
    let linear_tail_transform = get_linear_transform(transform);

    if decondition_gacl
        && linear_tail_transform != GaclTransform::None
        && let Ok(unshuffled) = gpck_core::gacl::Gacl::unshuffle(
            linear_tail_transform.to_u32(),
            &working_tail,
            tile_size,
            tail_base_width as usize,
        )
    {
        working_tail = unshuffled;
    }

    let mut offset = 0usize;
    let mut cur_w = tail_base_width;
    let mut cur_h = tail_base_height;

    for _ in 0..target_tail_mip {
        let mip_bytes = if D3D12FormatTable::is_block_compressed(dxgi_fmt) {
            (cur_w as usize).div_ceil(4) * (cur_h as usize).div_ceil(4) * element_size
        } else {
            let bpu = D3D12FormatTable::get_bits_per_unit(dxgi_fmt);
            (cur_w as usize * cur_h as usize * bpu as usize) / 8
        };
        offset += mip_bytes;
        cur_w = (cur_w / 2).max(1);
        cur_h = (cur_h / 2).max(1);
    }

    let target_bytes = if D3D12FormatTable::is_block_compressed(dxgi_fmt) {
        (cur_w as usize).div_ceil(4) * (cur_h as usize).div_ceil(4) * element_size
    } else {
        let bpu = D3D12FormatTable::get_bits_per_unit(dxgi_fmt);
        (cur_w as usize * cur_h as usize * bpu as usize) / 8
    };

    if offset + target_bytes > working_tail.len() {
        return None;
    }

    Some((
        working_tail[offset..offset + target_bytes].to_vec(),
        cur_w,
        cur_h,
    ))
}

fn detile_specific_mip(
    tiled_data: &[u8],
    base_width: u32,
    base_height: u32,
    target_mip: u32,
    gacl_fmt: u32,
    dxgi_fmt: u32,
    decondition_gacl: bool,
) -> Option<(Vec<u8>, u32, u32)> {
    let transform = GaclTransform::from_u32(gacl_fmt);
    let element_size = D3D12FormatTable::get_element_size(dxgi_fmt).unwrap_or(16);

    let tile_shape = D3D12FormatTable::get_tile_shape_64k(dxgi_fmt, false);
    let tile_w_texels = tile_shape.width_in_texels as usize;
    let tile_h_texels = tile_shape.height_in_texels as usize;
    let tile_size = 65536usize;

    let (tilings, packed_info, _total_tiles) = D3D12FormatTable::calculate_subresource_tilings(
        dxgi_fmt,
        base_width,
        base_height,
        1,
        13,
        1,
    );

    let num_standard_mips = packed_info.num_standard_mips as u32;
    let mip_w = (base_width >> target_mip).max(1);
    let mip_h = (base_height >> target_mip).max(1);
    let mip_wb = (mip_w as usize).div_ceil(4);
    let mip_hb = (mip_h as usize).div_ceil(4);
    let target_mip_bytes = mip_wb * mip_hb * element_size;

    if target_mip < num_standard_mips {
        let tiling = tilings.get(target_mip as usize)?;
        let mut current_tile_idx = tiling.start_tile_index_in_overall_resource as usize;

        let tile_wb = tile_w_texels / 4;
        let tile_hb = tile_h_texels / 4;
        let tiles_x = (mip_w as usize).div_ceil(tile_w_texels);
        let tiles_y = (mip_h as usize).div_ceil(tile_h_texels);

        let mut linear_buffer = vec![0u8; target_mip_bytes];

        for ty in 0..tiles_y {
            for tx in 0..tiles_x {
                let tile_start = current_tile_idx * tile_size;
                if tile_start + tile_size > tiled_data.len() {
                    break;
                }

                let mut tile_slice = tiled_data[tile_start..tile_start + tile_size].to_vec();

                if decondition_gacl
                    && gacl_fmt != 0
                    && let Ok(unshuffled) = gpck_core::gacl::Gacl::unshuffle(
                        gacl_fmt,
                        &tile_slice,
                        tile_size,
                        tile_w_texels,
                    )
                {
                    tile_slice = unshuffled;
                }

                let y_start_b = ty * tile_hb;
                let y_end_b = (y_start_b + tile_hb).min(mip_hb);
                let x_start_b = tx * tile_wb;
                let x_end_b = (x_start_b + tile_wb).min(mip_wb);

                for yb in y_start_b..y_end_b {
                    let row_in_tile = yb - y_start_b;
                    let src_off = row_in_tile * tile_wb * element_size;
                    let dst_off = (yb * mip_wb + x_start_b) * element_size;
                    let copy_len = (x_end_b - x_start_b) * element_size;

                    if src_off + copy_len <= tile_slice.len()
                        && dst_off + copy_len <= linear_buffer.len()
                    {
                        linear_buffer[dst_off..dst_off + copy_len]
                            .copy_from_slice(&tile_slice[src_off..src_off + copy_len]);
                    }
                }

                current_tile_idx += 1;
            }
        }

        return Some((linear_buffer, mip_w, mip_h));
    }

    let tail_start_tile = packed_info.start_tile_index_in_overall_resource as usize;
    let tail_byte_offset_in_archive = tail_start_tile * tile_size;

    if tail_byte_offset_in_archive >= tiled_data.len() {
        return None;
    }

    let tail_end = (tail_byte_offset_in_archive + tile_size).min(tiled_data.len());
    let mut tail_tile_slice = tiled_data[tail_byte_offset_in_archive..tail_end].to_vec();
    if tail_tile_slice.len() < tile_size {
        tail_tile_slice.resize(tile_size, 0);
    }

    let linear_tail_transform = get_linear_transform(transform);
    if decondition_gacl
        && linear_tail_transform != GaclTransform::None
        && let Ok(unshuffled) = gpck_core::gacl::Gacl::unshuffle(
            linear_tail_transform.to_u32(),
            &tail_tile_slice,
            tile_size,
            tile_w_texels,
        )
    {
        tail_tile_slice = unshuffled;
    }

    let mut offset_in_tail = 0usize;
    for m in num_standard_mips..target_mip {
        let mw = (base_width >> m).max(1);
        let mh = (base_height >> m).max(1);
        let m_bytes = (mw as usize).div_ceil(4) * (mh as usize).div_ceil(4) * element_size;
        offset_in_tail += m_bytes;
    }

    if offset_in_tail + target_mip_bytes > tail_tile_slice.len() {
        return None;
    }

    let mip_bytes = tail_tile_slice[offset_in_tail..offset_in_tail + target_mip_bytes].to_vec();

    Some((mip_bytes, mip_w, mip_h))
}

pub fn format_hex_dump(bytes: &[u8], max_bytes: usize) -> String {
    let len = bytes.len().min(max_bytes);
    let slice = &bytes[..len];
    let mut out = String::new();

    for (i, chunk) in slice.chunks(16).enumerate() {
        let offset = i * 16;
        out.push_str(&format!("{:08X}  ", offset));

        for (j, byte) in chunk.iter().enumerate() {
            out.push_str(&format!("{:02X} ", byte));
            if j == 7 {
                out.push(' ');
            }
        }

        if chunk.len() < 16 {
            let missing = 16 - chunk.len();
            for j in 0..missing {
                out.push_str("   ");
                if chunk.len() + j == 7 {
                    out.push(' ');
                }
            }
        }

        out.push_str(" |");
        for byte in chunk {
            if byte.is_ascii_graphic() || *byte == b' ' {
                out.push(*byte as char);
            } else {
                out.push('.');
            }
        }
        out.push_str("|\n");
    }

    if bytes.len() > max_bytes {
        out.push_str(&format!(
            "\n... [Truncated: showing {} of {} bytes]",
            max_bytes,
            bytes.len()
        ));
    }

    out
}
