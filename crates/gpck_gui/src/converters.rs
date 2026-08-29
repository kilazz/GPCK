// crates/gpck_gui/src/converters.rs
//! # Data Formatters & UI Model Converters

use crate::controller::get_all_entries_filtered;
use crate::{BlockItemUI, FileItemUI};
use gpck_core::compression::codecs::CompressionMethod;
use gpck_core::format::archive::{
    FLAG_BOOT_TAIL, GameArchive, TYPE_NEURAL_TEXTURE, TYPE_TILED_RESOURCE,
};
use gpck_core::gacl::GaclTransform;
use gpck_core::graphics::dxgi_format::dxgi;
use slint::{Color, SharedString};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

pub fn parse_index_to_gacl(idx: i32, mapping: &[u32]) -> Option<u32> {
    match idx {
        0 => None,
        1 => Some(0),
        _ => mapping.get((idx - 2) as usize).copied(),
    }
}

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub fn generate_file_items_from_store(store: &HashMap<PathBuf, String>) -> Vec<FileItemUI> {
    store
        .iter()
        .map(|(abs, rel)| {
            let sz = fs::metadata(abs).map(|m| m.len()).unwrap_or(0);
            let ext = rel.split('.').next_back().unwrap_or("raw").to_uppercase();
            FileItemUI {
                rel_path: SharedString::from(rel),
                res_str: SharedString::from("-"),
                format_str: SharedString::from(ext),
                method_str: SharedString::from("Pending"),
                gacl_str: SharedString::from("-"),
                partition_str: SharedString::from("-"),
                queue_str: SharedString::from("Hard (High)"),
                size_str: SharedString::from(format_size(sz)),
                comp_size_str: SharedString::from(format_size(sz)),
                ratio_str: SharedString::from("100%"),
                is_archive: false,
            }
        })
        .collect()
}

pub fn generate_ui_elements_from_archive(
    arch: &GameArchive,
) -> (Vec<FileItemUI>, Vec<BlockItemUI>, usize, usize) {
    let mut ui_items = Vec::new();
    let mut blocks = Vec::new();
    let mut total_orig_size = 0usize;
    let mut total_comp_size = 0usize;

    if let Ok(entries) = get_all_entries_filtered(arch) {
        let total_size = arch.total_uncompressed_size().max(1) as f64;

        let part_colors = [
            Color::from_rgb_u8(244, 67, 54),
            Color::from_rgb_u8(233, 30, 99),
            Color::from_rgb_u8(156, 39, 176),
            Color::from_rgb_u8(103, 58, 183),
            Color::from_rgb_u8(63, 81, 181),
            Color::from_rgb_u8(33, 150, 243),
            Color::from_rgb_u8(3, 169, 244),
            Color::from_rgb_u8(0, 188, 212),
            Color::from_rgb_u8(0, 150, 136),
            Color::from_rgb_u8(76, 175, 80),
            Color::from_rgb_u8(139, 195, 74),
            Color::from_rgb_u8(205, 220, 57),
            Color::from_rgb_u8(255, 235, 59),
            Color::from_rgb_u8(255, 193, 7),
            Color::from_rgb_u8(255, 152, 0),
            Color::from_rgb_u8(255, 87, 34),
        ];

        // Generate standard file table rows
        for e in &entries {
            let name = arch
                .get_path_for_asset(e)
                .unwrap_or_else(|| Uuid::from_bytes(e.asset_id).to_string());
            let method = CompressionMethod::from_flags(e.flags);
            let ext = name.split('.').next_back().unwrap_or("raw").to_uppercase();

            total_orig_size += e.original_size as usize;
            total_comp_size += e.compressed_size as usize;

            let gacl_transform = GaclTransform::from_u32(e.gacl_transform());
            let gacl_str = gacl_transform.display_name();

            let width = (e.meta1 >> 16) & 0xFFFF;
            let height = e.meta1 & 0xFFFF;
            let is_tiled = (e.flags & TYPE_TILED_RESOURCE) != 0;
            let is_neural = (e.flags & TYPE_NEURAL_TEXTURE) != 0 || ext == "GNTC" || ext == "NTEX";
            let dxgi_fmt = (e.meta2 >> 16) & 0xFF;

            let format_str = if is_neural {
                "GNTC".to_string()
            } else if dxgi_fmt > 0 {
                match dxgi_fmt {
                    dxgi::BC1_UNORM => "BC1u".to_string(),
                    dxgi::BC1_UNORM_SRGB => "BC1_sRGB".to_string(),
                    dxgi::BC2_UNORM => "BC2u".to_string(),
                    dxgi::BC2_UNORM_SRGB => "BC2_sRGB".to_string(),
                    dxgi::BC3_UNORM => "BC3u".to_string(),
                    dxgi::BC3_UNORM_SRGB => "BC3_sRGB".to_string(),
                    dxgi::BC4_UNORM => "BC4u".to_string(),
                    dxgi::BC4_SNORM => "BC4s".to_string(),
                    dxgi::BC5_UNORM => "BC5u".to_string(),
                    dxgi::BC5_SNORM => "BC5s".to_string(),
                    dxgi::BC6H_UF16 => "BC6H_uf16".to_string(),
                    dxgi::BC6H_SF16 => "BC6H_sf16".to_string(),
                    dxgi::BC7_UNORM => "BC7".to_string(),
                    dxgi::BC7_UNORM_SRGB => "BC7_sRGB".to_string(),
                    dxgi::B8G8R8A8_UNORM | dxgi::B8G8R8A8_UNORM_SRGB => "BGRA8".to_string(),
                    dxgi::R8G8B8A8_UNORM | dxgi::R8G8B8A8_UNORM_SRGB => "RGBA8".to_string(),
                    dxgi::B8G8R8X8_UNORM | dxgi::B8G8R8X8_UNORM_SRGB => "BGRX8".to_string(),
                    dxgi::B5G6R5_UNORM => "RGB565".to_string(),
                    dxgi::B5G5R5A1_UNORM => "RGB5A1".to_string(),
                    dxgi::B4G4R4A4_UNORM => "RGBA4".to_string(),
                    dxgi::R8_UNORM => "R8".to_string(),
                    dxgi::A8_UNORM => "A8".to_string(),
                    dxgi::R16_UNORM => "R16".to_string(),
                    _ => ext,
                }
            } else if gacl_transform != GaclTransform::None {
                match gacl_transform.to_dxgi_format() {
                    dxgi::BC1_UNORM => "BC1u".to_string(),
                    dxgi::BC2_UNORM => "BC2u".to_string(),
                    dxgi::BC3_UNORM => "BC3u".to_string(),
                    dxgi::BC4_UNORM => "BC4u".to_string(),
                    dxgi::BC5_UNORM => "BC5u".to_string(),
                    dxgi::BC6H_UF16 => "BC6H_uf16".to_string(),
                    dxgi::BC7_UNORM => "BC7".to_string(),
                    _ => ext,
                }
            } else {
                ext
            };

            let res_str = if is_neural {
                "Neural PBR".to_string()
            } else if width > 0 && height > 0 {
                if is_tiled {
                    let tile_count = e.meta2 & 0x0000FFFF;
                    if tile_count > 0 {
                        format!("{}x{} ({}T)", width, height, tile_count)
                    } else {
                        format!("{}x{} (Tile)", width, height)
                    }
                } else {
                    format!("{}x{}", width, height)
                }
            } else {
                "-".to_string()
            };

            let queue_str = if name.to_lowercase().ends_with(".highmips") {
                "Soft (Low)"
            } else {
                "Hard (High)"
            };

            let partition_str = format!("P{}", e.partition_id);

            let ratio = if e.original_size > 0 {
                (e.compressed_size as f64 / e.original_size as f64) * 100.0
            } else {
                100.0
            };

            ui_items.push(FileItemUI {
                rel_path: SharedString::from(&name),
                res_str: SharedString::from(res_str),
                format_str: SharedString::from(format_str),
                method_str: SharedString::from(format!("{:?}", method)),
                gacl_str: SharedString::from(gacl_str),
                partition_str: SharedString::from(partition_str),
                queue_str: SharedString::from(queue_str),
                size_str: SharedString::from(format_size(e.original_size as u64)),
                comp_size_str: SharedString::from(format_size(e.compressed_size as u64)),
                ratio_str: SharedString::from(format!("{:.0}%", ratio)),
                is_archive: true,
            });
        }

        // ====================================================================
        // Physical Disk Layout Sorting for VFS Block Visualizer
        // ====================================================================
        // Sorts strictly by physical `data_offset` on NVMe storage.
        // Guarantees Block 0..K represents Partition 0 Boot-Tails at the start of .gdat.
        let mut physical_entries = entries.clone();
        physical_entries.sort_by_key(|e| e.data_offset);

        for e in &physical_entries {
            let name = arch
                .get_path_for_asset(e)
                .unwrap_or_else(|| Uuid::from_bytes(e.asset_id).to_string());
            let method = CompressionMethod::from_flags(e.flags);

            let is_boot_tail = (e.flags & FLAG_BOOT_TAIL) != 0 || name.ends_with(".tail");
            let offset_hex = format!("0x{:08X}", e.data_offset);

            let block_width =
                ((e.compressed_size as f64 / total_size) * 1400.0).clamp(6.0, 160.0) as f32;

            let color_algo = match method {
                CompressionMethod::GDeflate => Color::from_rgb_u8(129, 199, 132),
                CompressionMethod::Zstd => Color::from_rgb_u8(144, 202, 249),
                CompressionMethod::Lz4 => Color::from_rgb_u8(255, 183, 77),
                CompressionMethod::Rans => Color::from_rgb_u8(171, 71, 188),
                CompressionMethod::BrotliG => Color::from_rgb_u8(232, 121, 249),
                _ => Color::from_rgb_u8(207, 216, 220),
            };

            let color_part = part_colors[(e.partition_id as usize) % part_colors.len()];

            blocks.push(BlockItemUI {
                width: block_width,
                color_algo,
                color_part,
                tooltip: SharedString::from(format!(
                    "{} ({})",
                    name,
                    format_size(e.original_size as u64)
                )),
                file_name: SharedString::from(&name),
                offset_str: SharedString::from(offset_hex),
                size_str: SharedString::from(format_size(e.original_size as u64)),
                comp_size_str: SharedString::from(format_size(e.compressed_size as u64)),
                partition_str: SharedString::from(format!("Partition {}", e.partition_id)),
                codec_str: SharedString::from(format!("{:?}", method)),
                is_boot_tail,
            });
        }
    }

    (ui_items, blocks, total_orig_size, total_comp_size)
}
