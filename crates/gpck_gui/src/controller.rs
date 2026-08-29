// crates/gpck_gui/src/controller.rs
//! # Slint UI Event Controller & Settings Synchronization

use crate::converters::*;
use crate::preview::trigger_async_preview;
use crate::{AppWindow, FileItemUI};

use anyhow::Result;
use gpck_core::benchmark;
use gpck_core::compression::codecs::CompressionMethod;
use gpck_core::core::preset::PackerPreset;
use gpck_core::core::settings::{self, AppSettings};
use gpck_core::crypto::aes_gcm::derive_key;
use gpck_core::format::archive::{FLAG_DELETED, FileEntry, GameArchive};
use gpck_core::io::extract::extract_asset_recombined;
use gpck_core::packer::{
    AssetPacker, GaclFormatOverrides, NtcPackerOptions, PackerOptions, PbrSuffixConfig,
};
use slint::{ComponentHandle, Image, ModelRc, SharedPixelBuffer, SharedString, VecModel};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct AppState {
    pub file_store: HashMap<PathBuf, String>,
    pub open_archive_path: Option<PathBuf>,
    pub current_items: Vec<FileItemUI>,
}

pub struct GuiController {
    pub ui: AppWindow,
    pub state: Arc<RwLock<AppState>>,
}

/// Generates a procedural 512x512 checkerboard background image.
pub fn create_checkerboard_image(
    c1: [u8; 4],
    c2: [u8; 4],
    tile_size: usize,
    width: u32,
    height: u32,
) -> Image {
    let mut pixel_buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
    let raw_bytes: &mut [u8] = bytemuck::cast_slice_mut(pixel_buffer.make_mut_slice());

    for y in 0..height as usize {
        for x in 0..width as usize {
            let is_even = ((x / tile_size) + (y / tile_size)).is_multiple_of(2);
            let color = if is_even { c1 } else { c2 };
            let idx = (y * width as usize + x) * 4;
            raw_bytes[idx..idx + 4].copy_from_slice(&color);
        }
    }

    Image::from_rgba8(pixel_buffer)
}

fn parse_suffix_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|item| item.trim().to_lowercase())
        .filter(|item| !item.is_empty())
        .collect()
}

impl GuiController {
    pub fn new(ui: AppWindow) -> Self {
        ui.set_checker_dark_img(create_checkerboard_image(
            [24, 24, 27, 255],
            [39, 39, 42, 255],
            16,
            512,
            512,
        ));
        ui.set_checker_light_img(create_checkerboard_image(
            [228, 228, 231, 255],
            [255, 255, 255, 255],
            16,
            512,
            512,
        ));

        Self {
            ui,
            state: Arc::new(RwLock::new(AppState {
                file_store: HashMap::new(),
                open_archive_path: None,
                current_items: Vec::new(),
            })),
        }
    }

    pub fn attach_all_callbacks(&self) {
        self.attach_clear_list();
        self.attach_add_folder();
        self.attach_add_files();
        self.attach_open_archive();
        self.attach_block_clicked();
        self.attach_sort_column_clicked();
        self.attach_run_bench_clicked();
        self.attach_run_custom_bench_clicked();
        self.attach_compress_clicked();
        self.attach_extract_clicked();
        self.attach_verify_clicked();
        self.attach_file_selected();
        self.attach_preset_selected();
        self.attach_tonemap_changed();
        self.attach_bg_mode_changed();
        self.attach_channel_mask_changed();
        self.attach_preview_option_toggled();
        self.attach_mip_level_changed();
    }

    fn attach_preview_option_toggled(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();

        self.ui.on_preview_option_toggled(move || {
            if let Some(ui) = ui_weak.upgrade() {
                save_ui_settings(&ui);

                let s = state_clone.read().unwrap();
                let selected_idx = ui.get_selected_file_index();
                if selected_idx >= 0
                    && let Some(item) = s.current_items.get(selected_idx as usize)
                {
                    let rel_str = item.rel_path.to_string();
                    let local_path = s
                        .file_store
                        .iter()
                        .find(|(_, v)| *v == &rel_str)
                        .map(|(k, _)| k.clone());

                    trigger_async_preview(&ui, s.open_archive_path.clone(), rel_str, local_path);
                }
            }
        });
    }

    fn attach_channel_mask_changed(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();

        self.ui.on_channel_mask_changed(move || {
            if let Some(ui) = ui_weak.upgrade() {
                save_ui_settings(&ui);

                let s = state_clone.read().unwrap();
                let selected_idx = ui.get_selected_file_index();
                if selected_idx >= 0
                    && let Some(item) = s.current_items.get(selected_idx as usize)
                {
                    let rel_str = item.rel_path.to_string();
                    let local_path = s
                        .file_store
                        .iter()
                        .find(|(_, v)| *v == &rel_str)
                        .map(|(k, _)| k.clone());

                    trigger_async_preview(&ui, s.open_archive_path.clone(), rel_str, local_path);
                }
            }
        });
    }

    fn attach_bg_mode_changed(&self) {
        let ui_weak = self.ui.as_weak();
        self.ui.on_bg_mode_changed(move |_idx| {
            if let Some(ui) = ui_weak.upgrade() {
                save_ui_settings(&ui);
            }
        });
    }

    fn attach_mip_level_changed(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();

        self.ui.on_mip_level_changed(move |_idx| {
            if let Some(ui) = ui_weak.upgrade() {
                let s = state_clone.read().unwrap();
                let selected_idx = ui.get_selected_file_index();
                if selected_idx >= 0
                    && let Some(item) = s.current_items.get(selected_idx as usize)
                {
                    let rel_str = item.rel_path.to_string();
                    let local_path = s
                        .file_store
                        .iter()
                        .find(|(_, v)| *v == &rel_str)
                        .map(|(k, _)| k.clone());

                    trigger_async_preview(&ui, s.open_archive_path.clone(), rel_str, local_path);
                }
            }
        });
    }

    fn attach_tonemap_changed(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();

        self.ui.on_tonemap_mode_changed(move |_idx| {
            if let Some(ui) = ui_weak.upgrade() {
                save_ui_settings(&ui);

                let s = state_clone.read().unwrap();
                let selected_idx = ui.get_selected_file_index();
                if selected_idx >= 0
                    && let Some(item) = s.current_items.get(selected_idx as usize)
                {
                    let rel_str = item.rel_path.to_string();
                    let local_path = s
                        .file_store
                        .iter()
                        .find(|(_, v)| *v == &rel_str)
                        .map(|(k, _)| k.clone());

                    trigger_async_preview(&ui, s.open_archive_path.clone(), rel_str, local_path);
                }
            }
        });
    }

    fn attach_preset_selected(&self) {
        let ui_weak = self.ui.as_weak();
        self.ui.on_preset_selected(move |idx: i32| {
            if let Some(ui) = ui_weak.upgrade() {
                let preset = match idx {
                    0 => PackerPreset::GpuStreaming,
                    1 => PackerPreset::NeuralPbrNtc,
                    2 => PackerPreset::MobileAndroid,
                    3 => PackerPreset::MaxCompression,
                    4 => PackerPreset::FastDevBuild,
                    5 => PackerPreset::SecureDelivery,
                    _ => PackerPreset::Custom,
                };

                let opts = preset.to_packer_options(None);

                let method_idx = match opts.method {
                    CompressionMethod::GDeflate => 0,
                    CompressionMethod::Zstd => 1,
                    CompressionMethod::Lz4 => 2,
                    CompressionMethod::Rans => 3,
                    CompressionMethod::BrotliG => 4,
                    CompressionMethod::Store => 5,
                    _ => 0,
                };

                ui.set_method_index(method_idx);
                ui.set_compression_level(opts.level);
                ui.set_enable_deduplication(opts.enable_dedup);
                ui.set_validate_chunks(opts.validate_chunks);
                ui.set_atg_profile(opts.atg_profile);
                ui.set_tiled_streaming_enabled(opts.tiled_streaming);
                ui.set_mip_split_enabled(opts.mip_split);
                ui.set_gacl_enabled(opts.gacl.enabled);
                ui.set_gacl_auto_mode(opts.gacl.auto_mode);

                // MiniDXNN Options Bridge
                ui.set_ntc_enabled(opts.ntc.enabled);
                ui.set_ntc_target_bpp(opts.ntc.target_bpp);
                ui.set_ntc_auto_bundle(opts.ntc.auto_bundle_pbr);
                ui.set_ntc_precompute_bc7_modes(opts.ntc.precompute_bc7_modes);
                ui.set_ntc_wave_reduced_accum(opts.ntc.stable_training);

                ui.set_rdo_enabled(opts.gacl.rdo_reduction_pct > 0.0);
                ui.set_rdo_reduction_pct(opts.gacl.rdo_reduction_pct);
                ui.set_rdo_use_ycocg(opts.gacl.rdo_use_ycocg);
                ui.set_rdo_bc1(opts.gacl.rdo_bc1);
                ui.set_rdo_bc2(opts.gacl.rdo_bc2);
                ui.set_rdo_bc3(opts.gacl.rdo_bc3);
                ui.set_rdo_bc4(opts.gacl.rdo_bc4);
                ui.set_rdo_bc5(opts.gacl.rdo_bc5);
                ui.set_rdo_bc6h(opts.gacl.rdo_bc6h);
                ui.set_rdo_bc7(opts.gacl.rdo_bc7);

                let part_idx = match opts.max_partition_size {
                    s if s <= 32 * 1024 * 1024 => 0,
                    s if s <= 64 * 1024 * 1024 => 1,
                    s if s <= 128 * 1024 * 1024 => 2,
                    s if s <= 256 * 1024 * 1024 => 3,
                    _ => 4,
                };
                ui.set_partition_size_index(part_idx);

                let tail_idx = match opts.max_tail_dim {
                    64 => 0,
                    256 => 2,
                    512 => 3,
                    _ => 1,
                };
                ui.set_max_tail_res_index(tail_idx);

                let min_tiled_idx = match opts.min_tiled_resolution {
                    1024 => 1,
                    4096 => 2,
                    0 => 3,
                    _ => 0,
                };
                ui.set_min_tiled_res_index(min_tiled_idx);

                save_ui_settings(&ui);

                append_log(
                    &ui,
                    &format!(
                        "[Preset] Activated profile: {}",
                        PackerPreset::ALL_NAMES[idx.clamp(0, 6) as usize]
                    ),
                );
            }
        });
    }

    fn attach_clear_list(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_clear_list_clicked(move || {
            let mut s = state_clone.write().unwrap();
            s.file_store.clear();
            s.open_archive_path = None;
            s.current_items.clear();

            if let Some(ui) = ui_weak.upgrade() {
                ui.set_file_list(ModelRc::new(VecModel::default()));
                ui.set_visualizer_blocks(ModelRc::new(VecModel::default()));
                ui.set_selected_file_index(-1);
                ui.set_selected_block_index(-1);
                ui.set_show_image(false);
                ui.set_preview_text(SharedString::from(""));
                ui.set_status_text(SharedString::from("Cleared file list."));
                ui.set_total_vram_footprint(SharedString::from("0 B"));
                ui.set_total_disk_footprint(SharedString::from("0 B"));
                ui.set_preview_zoom(1.0);
                ui.set_preview_offset_x(0.0);
                ui.set_preview_offset_y(0.0);
                append_log(&ui, "[Action] Cleared file list and unloaded archives.");
            }
        });
    }

    fn attach_add_folder(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_add_folder_clicked(move || {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                let state_clone = state_clone.clone();
                let ui_weak = ui_weak.clone();

                std::thread::spawn(move || {
                    if let Ok(map) = AssetPacker::build_file_map(&folder) {
                        let mut s = state_clone.write().unwrap();
                        s.file_store.extend(map);
                        let ui_items = generate_file_items_from_store(&s.file_store);
                        let count = ui_items.len();
                        s.current_items = ui_items.clone();

                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                ui.set_file_list(ModelRc::new(VecModel::from(ui_items)));
                                ui.set_selected_file_index(-1);
                                ui.set_status_text(SharedString::from("Added folder."));
                                append_log(
                                    &ui,
                                    &format!("[Action] Added folder ({} items).", count),
                                );
                            }
                        })
                        .ok();
                    }
                });
            }
        });
    }

    fn attach_add_files(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_add_files_clicked(move || {
            if let Some(files) = rfd::FileDialog::new().pick_files() {
                let state_clone = state_clone.clone();
                let ui_weak = ui_weak.clone();

                std::thread::spawn(move || {
                    let mut s = state_clone.write().unwrap();
                    for file_path in files {
                        if let Some(name) = file_path.file_name() {
                            s.file_store
                                .insert(file_path.clone(), name.to_string_lossy().to_string());
                        }
                    }
                    let ui_items = generate_file_items_from_store(&s.file_store);
                    let count = ui_items.len();
                    s.current_items = ui_items.clone();

                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_file_list(ModelRc::new(VecModel::from(ui_items)));
                            ui.set_status_text(SharedString::from(format!(
                                "Added {} files.",
                                count
                            )));
                        }
                    })
                    .ok();
                });
            }
        });
    }

    fn attach_open_archive(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_open_archive_clicked(move || {
            if let Some(file) = rfd::FileDialog::new()
                .add_filter("GPCK Archive", &["gtoc"])
                .pick_file()
            {
                state_clone.write().unwrap().open_archive_path = Some(file.clone());
                let state_clone = state_clone.clone();
                let ui_weak = ui_weak.clone();

                std::thread::spawn(move || {
                    if let Ok(arch) = GameArchive::open(&file) {
                        let (ui_items, blocks, total_orig, total_comp) =
                            generate_ui_elements_from_archive(&arch);
                        let count = ui_items.len();
                        state_clone.write().unwrap().current_items = ui_items.clone();

                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                ui.set_file_list(ModelRc::new(VecModel::from(ui_items)));
                                ui.set_visualizer_blocks(ModelRc::new(VecModel::from(blocks)));
                                ui.set_total_vram_footprint(SharedString::from(format_size(
                                    total_orig as u64,
                                )));
                                ui.set_total_disk_footprint(SharedString::from(format_size(
                                    total_comp as u64,
                                )));
                                ui.set_status_text(SharedString::from(format!(
                                    "Loaded {:?}",
                                    file.file_name().unwrap_or_default()
                                )));
                                append_log(
                                    &ui,
                                    &format!("[Success] Loaded archive with {} entries.", count),
                                );
                            }
                        })
                        .ok();
                    }
                });
            }
        });
    }

    fn attach_block_clicked(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_block_clicked(move |idx| {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_selected_block_index(idx);

                let s = state_clone.read().unwrap();
                if let Some(arch_p) = s.open_archive_path.as_ref()
                    && let Ok(arch) = GameArchive::open(arch_p)
                    && let Ok(mut entries) = get_all_entries_filtered(&arch)
                {
                    entries.sort_by_key(|e| e.data_offset);
                    if let Some(entry) = entries.get(idx as usize) {
                        let rel_path = arch
                            .get_path_for_asset(entry)
                            .unwrap_or_else(|| Uuid::from_bytes(entry.asset_id).to_string());

                        if let Some(pos) = s
                            .current_items
                            .iter()
                            .position(|it| it.rel_path == rel_path.as_str())
                        {
                            ui.set_selected_file_index(pos as i32);
                        }

                        ui.set_status_text(SharedString::from(format!(
                            "Selected Block #{}: {} (Offset: 0x{:08X})",
                            idx + 1,
                            rel_path,
                            entry.data_offset
                        )));

                        trigger_async_preview(&ui, s.open_archive_path.clone(), rel_path, None);
                    }
                }
            }
        });
    }

    fn attach_sort_column_clicked(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_sort_column_clicked(move |column_idx| {
            if let Some(ui) = ui_weak.upgrade() {
                let mut s = state_clone.write().unwrap();
                if s.current_items.is_empty() {
                    return;
                }

                let current_col = ui.get_sort_column();
                let mut ascending = ui.get_sort_ascending();

                if current_col == column_idx {
                    ascending = !ascending;
                } else {
                    ascending = true;
                }

                ui.set_sort_column(column_idx);
                ui.set_sort_ascending(ascending);

                s.current_items.sort_by(|a, b| {
                    let cmp = match column_idx {
                        0 => a.rel_path.cmp(&b.rel_path),
                        1 => a.res_str.cmp(&b.res_str),
                        2 => a.format_str.cmp(&b.format_str),
                        3 => a.method_str.cmp(&b.method_str),
                        4 => a.gacl_str.cmp(&b.gacl_str),
                        5 => a.partition_str.cmp(&b.partition_str),
                        6 => a.queue_str.cmp(&b.queue_str),
                        7 => a.size_str.cmp(&b.size_str),
                        8 => a.comp_size_str.cmp(&b.comp_size_str),
                        _ => a.ratio_str.cmp(&b.ratio_str),
                    };
                    if ascending { cmp } else { cmp.reverse() }
                });

                ui.set_file_list(ModelRc::new(VecModel::from(s.current_items.clone())));
                ui.set_selected_file_index(-1);
            }
        });
    }

    fn attach_file_selected(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();

        self.ui.on_file_selected(move |rel_path: SharedString| {
            let s = state_clone.read().unwrap();
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_preview_zoom(1.0);
                ui.set_preview_offset_x(0.0);
                ui.set_preview_offset_y(0.0);

                let rel_str = rel_path.to_string();
                let local_path = s
                    .file_store
                    .iter()
                    .find(|(_, v)| *v == &rel_str)
                    .map(|(k, _)| k.clone());

                trigger_async_preview(&ui, s.open_archive_path.clone(), rel_str, local_path);
            }
        });
    }

    fn attach_compress_clicked(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_compress_clicked(move || {
            let ui = ui_weak.unwrap();
            save_ui_settings(&ui);

            if let Some(save_file) = rfd::FileDialog::new()
                .add_filter("GPCK Archive", &["gtoc"])
                .save_file()
            {
                ui.set_is_processing(true);
                ui.set_status_text(SharedString::from(
                    "Packing archive with active settings...",
                ));
                append_log(
                    &ui,
                    &format!("[Action] Packing archive to {:?}...", save_file),
                );
                ui.set_progress_value(0.2);

                let method_idx = ui.get_method_index();
                let level = ui.get_compression_level();
                let partition_idx = ui.get_partition_size_index();
                let enable_dedup = ui.get_enable_deduplication();
                let validate_chunks = ui.get_validate_chunks();
                let atg_profile = ui.get_atg_profile();
                let tiled_streaming = ui.get_tiled_streaming_enabled();
                let min_tiled_res_idx = ui.get_min_tiled_res_index();
                let mip_split = ui.get_mip_split_enabled();
                let max_tail_idx = ui.get_max_tail_res_index();
                let key_str = ui.get_encryption_key().to_string();

                // MiniDXNN Options Bridge with exact grid resolution mapping
                let ntc_enabled = ui.get_ntc_enabled();
                let ntc_grid_res_idx = ui.get_ntc_grid_res_index();
                let ntc_bpp = match ntc_grid_res_idx {
                    0 => 5.0,  // Grid 64x64
                    1 => 8.0,  // Grid 128x128
                    2 => 12.0, // Grid 256x256 (High Quality)
                    3 => 16.0, // Grid 512x512 (Ultra 4K Crisp)
                    4 => 20.0, // Grid 1024x1024 (Extreme 4K Native)
                    _ => ui.get_ntc_target_bpp().max(1.5),
                };

                let ntc_quality_idx = ui.get_ntc_quality_index();
                let ntc_auto_bundle = ui.get_ntc_auto_bundle();
                let ntc_precompute_bc7 = ui.get_ntc_precompute_bc7_modes();
                let ntc_wave_reduced = ui.get_ntc_wave_reduced_accum();

                let sfx_albedo = parse_suffix_list(&ui.get_pbr_suffix_albedo());
                let sfx_normal = parse_suffix_list(&ui.get_pbr_suffix_normal());
                let sfx_metallic = parse_suffix_list(&ui.get_pbr_suffix_metal());
                let sfx_roughness = parse_suffix_list(&ui.get_pbr_suffix_rough());
                let sfx_ao = parse_suffix_list(&ui.get_pbr_suffix_ao());
                let sfx_displacement = parse_suffix_list(&ui.get_pbr_suffix_displ());

                let ntc_training_steps = match ntc_quality_idx {
                    0 => 10,
                    1 => 30,
                    2 => 100,
                    _ => 30,
                };

                let gacl_enabled = ui.get_gacl_enabled();
                let gacl_auto_mode = ui.get_gacl_auto_mode();

                let bc1_override = parse_index_to_gacl(ui.get_gacl_bc1_index(), &[1, 17, 32, 33]);
                let bc2_override = parse_index_to_gacl(ui.get_gacl_bc2_index(), &[6]);
                let bc3_override = parse_index_to_gacl(ui.get_gacl_bc3_index(), &[2, 18, 34, 35]);
                let bc4_override = parse_index_to_gacl(ui.get_gacl_bc4_index(), &[3, 19]);
                let bc5_override = parse_index_to_gacl(ui.get_gacl_bc5_index(), &[4, 20]);
                let bc6h_override = parse_index_to_gacl(ui.get_gacl_bc6h_index(), &[7]);
                let bc7_override = parse_index_to_gacl(ui.get_gacl_bc7_index(), &[10, 11]);

                let rdo_enabled = ui.get_rdo_enabled();
                let rdo_pct = if rdo_enabled {
                    ui.get_rdo_reduction_pct()
                } else {
                    0.0
                };
                let rdo_use_ycocg = ui.get_rdo_use_ycocg();
                let rdo_bc1 = ui.get_rdo_bc1();
                let rdo_bc2 = ui.get_rdo_bc2();
                let rdo_bc3 = ui.get_rdo_bc3();
                let rdo_bc4 = ui.get_rdo_bc4();
                let rdo_bc5 = ui.get_rdo_bc5();
                let rdo_bc6h = ui.get_rdo_bc6h();
                let rdo_bc7 = ui.get_rdo_bc7();

                let store = state_clone.read().unwrap().file_store.clone();
                let ui_weak_log = ui_weak.clone();

                std::thread::spawn(move || {
                    let method = match method_idx {
                        0 => CompressionMethod::GDeflate,
                        1 => CompressionMethod::Zstd,
                        2 => CompressionMethod::Lz4,
                        3 => CompressionMethod::Rans,
                        4 => CompressionMethod::BrotliG,
                        _ => CompressionMethod::Store,
                    };

                    let key_bytes = if key_str.is_empty() {
                        None
                    } else {
                        Some(derive_key(&key_str))
                    };

                    let max_tail_dim = match max_tail_idx {
                        0 => 64,
                        2 => 256,
                        3 => 512,
                        _ => 128,
                    };

                    let max_partition_size = match partition_idx {
                        0 => 32 * 1024 * 1024,
                        2 => 128 * 1024 * 1024,
                        3 => 256 * 1024 * 1024,
                        4 => 512 * 1024 * 1024,
                        _ => 64 * 1024 * 1024,
                    };

                    let (min_tiled_resolution, min_tiled_tile_count) = match min_tiled_res_idx {
                        1 => (1024, 4),
                        2 => (4096, 8),
                        3 => (0, 0),
                        _ => (2048, 8),
                    };

                    let options = PackerOptions {
                        method,
                        level,
                        chunk_size: gpck_core::packer::DEFAULT_CHUNK_SIZE,
                        enable_dedup,
                        key: key_bytes,
                        mip_split,
                        max_tail_dim,
                        tags: gpck_core::format::archive::TAG_BASE_GAME,
                        validate_chunks,
                        max_partition_size,
                        atg_profile,
                        tiled_streaming,
                        min_tiled_resolution,
                        min_tiled_tile_count,
                        gacl: GaclFormatOverrides {
                            enabled: gacl_enabled,
                            auto_mode: gacl_auto_mode,
                            bc1_transform: bc1_override,
                            bc2_transform: bc2_override,
                            bc3_transform: bc3_override,
                            bc4_transform: bc4_override,
                            bc5_transform: bc5_override,
                            bc6h_transform: bc6h_override,
                            bc7_transform: bc7_override,
                            rdo_reduction_pct: rdo_pct,
                            rdo_use_ycocg,
                            rdo_bc1,
                            rdo_bc2,
                            rdo_bc3,
                            rdo_bc4,
                            rdo_bc5,
                            rdo_bc6h,
                            rdo_bc7,
                        },
                        ntc: NtcPackerOptions {
                            enabled: ntc_enabled,
                            target_bpp: ntc_bpp,
                            training_steps: ntc_training_steps,
                            auto_bundle_pbr: ntc_auto_bundle,
                            precompute_bc7_modes: ntc_precompute_bc7,
                            stable_training: ntc_wave_reduced,
                            pbr_suffixes: PbrSuffixConfig {
                                albedo: sfx_albedo,
                                normal: sfx_normal,
                                metallic: sfx_metallic,
                                roughness: sfx_roughness,
                                ao: sfx_ao,
                                displacement: sfx_displacement,
                            },
                        },
                    };

                    let log_ui_weak = ui_weak_log.clone();
                    let log_callback = move |msg: &str| {
                        let msg_owned = msg.to_string();
                        let log_ui_weak = log_ui_weak.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = log_ui_weak.upgrade() {
                                append_log(&ui, &msg_owned);
                            }
                        })
                        .ok();
                    };

                    let res = AssetPacker::compress_files_to_archive(
                        &store,
                        &save_file,
                        &options,
                        log_callback,
                    );

                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak_log.upgrade() {
                            ui.set_is_processing(false);
                            ui.set_progress_value(1.0);
                            match res {
                                Ok(_) => {
                                    append_log(&ui, "[Success] Packed archive successfully! ✅")
                                }
                                Err(e) => {
                                    append_log(&ui, &format!("[Error] Packing failed: {}", e))
                                }
                            }
                        }
                    })
                    .ok();
                });
            }
        });
    }

    fn attach_extract_clicked(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_extract_clicked(move || {
            let arch_path = state_clone.read().unwrap().open_archive_path.clone();
            let ui = ui_weak.unwrap();

            if arch_path.is_none() {
                ui.set_status_text(SharedString::from("No archive loaded to extract."));
                append_log(&ui, "[Warning] Extraction aborted: No archive loaded.");
                return;
            }

            if let Some(dest_dir) = rfd::FileDialog::new().pick_folder() {
                let key_str = ui.get_encryption_key().to_string();
                let raw_mode = ui.get_raw_extraction_enabled();

                ui.set_is_processing(true);
                let status_msg = if raw_mode {
                    "Extracting raw assets (1-to-1 disk copy)..."
                } else {
                    "Extracting and recombining assets..."
                };
                ui.set_status_text(SharedString::from(status_msg));

                let mode_str = if raw_mode {
                    "RAW Mode"
                } else {
                    "Smart Recombine DDS"
                };
                append_log(
                    &ui,
                    &format!(
                        "[Action] Extracting archive ({}) to {:?}...",
                        mode_str, dest_dir
                    ),
                );

                let ui_weak_thread = ui_weak.clone();

                std::thread::spawn(move || {
                    let key_bytes = if key_str.is_empty() {
                        None
                    } else {
                        Some(derive_key(&key_str))
                    };

                    let res = (|| -> Result<(usize, usize)> {
                        let arch_p = arch_path.unwrap();
                        let mut archive = GameArchive::open(&arch_p)?;
                        archive.decryption_key = key_bytes;

                        let entries = get_all_entries_filtered(&archive)?;
                        let total = entries.len();
                        let mut count = 0;
                        let mut errors = 0;

                        for (i, entry) in entries.iter().enumerate() {
                            match extract_asset_recombined(&archive, entry, &dest_dir, raw_mode) {
                                Ok(true) => count += 1,
                                Ok(false) => {}
                                Err(e) => {
                                    errors += 1;
                                    let rel_path = archive.get_path_for_asset(entry).unwrap_or_else(
                                        || Uuid::from_bytes(entry.asset_id).to_string(),
                                    );
                                    let err_msg =
                                        format!("[Error] Failed to extract {}: {}", rel_path, e);
                                    let ui_weak2 = ui_weak_thread.clone();
                                    slint::invoke_from_event_loop(move || {
                                        if let Some(ui2) = ui_weak2.upgrade() {
                                            append_log(&ui2, &err_msg);
                                        }
                                    })
                                    .ok();
                                }
                            }

                            let progress = (i + 1) as f32 / total as f32;
                            let ui_weak2 = ui_weak_thread.clone();
                            slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_weak2.upgrade() {
                                    ui.set_progress_value(progress);
                                }
                            })
                            .ok();
                        }

                        Ok((count, errors))
                    })();

                    slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_weak_thread.upgrade() {
                            ui.set_is_processing(false);
                            ui.set_progress_value(1.0);
                            match res {
                                Ok((c, e)) => {
                                    ui.set_status_text(SharedString::from(format!(
                                        "Extracted {} assets. ({} errors)",
                                        c, e
                                    )));
                                    append_log(
                                        &ui,
                                        &format!(
                                            "[Success] Extraction finished. Processed: {}, Errors: {}",
                                            c, e
                                        ),
                                    );
                                }
                                Err(e) => {
                                    ui.set_status_text(SharedString::from(format!(
                                        "Extraction error: {}",
                                        e
                                    )));
                                    append_log(
                                        &ui,
                                        &format!("[Error] Fatal extraction error: {}", e),
                                    );
                                }
                            }
                        }
                    })
                    .ok();
                });
            }
        });
    }

    fn attach_verify_clicked(&self) {
        let state_clone = self.state.clone();
        let ui_weak = self.ui.as_weak();
        self.ui.on_verify_clicked(move || {
            let arch_path = state_clone.read().unwrap().open_archive_path.clone();
            let ui = ui_weak.unwrap();

            if arch_path.is_none() {
                ui.set_status_text(SharedString::from("No archive loaded to verify."));
                append_log(&ui, "[Warning] Verification aborted: No archive loaded.");
                return;
            }

            let key_str = ui.get_encryption_key().to_string();

            ui.set_is_processing(true);
            ui.set_status_text(SharedString::from("Verifying archive integrity..."));
            append_log(&ui, "[Action] Starting full archive integrity check...");

            let ui_weak = ui_weak.clone();

            std::thread::spawn(move || {
                let key_bytes = if key_str.is_empty() {
                    None
                } else {
                    Some(derive_key(&key_str))
                };

                let res = (|| -> Result<(usize, usize)> {
                    let arch_p = arch_path.unwrap();
                    let mut archive = GameArchive::open(&arch_p)?;
                    archive.decryption_key = key_bytes;

                    let entries = get_all_entries_filtered(&archive)?;
                    let total = entries.len();
                    let mut errors = 0;

                    for (i, entry) in entries.iter().enumerate() {
                        if let Err(e) = archive.read_asset(entry) {
                            errors += 1;
                            let name = archive
                                .get_path_for_asset(entry)
                                .unwrap_or_else(|| Uuid::from_bytes(entry.asset_id).to_string());
                            let err_msg = format!("[Corrupt] Asset {}: {}", name, e);
                            let ui_weak2 = ui_weak.clone();
                            slint::invoke_from_event_loop(move || {
                                if let Some(ui2) = ui_weak2.upgrade() {
                                    append_log(&ui2, &err_msg);
                                }
                            })
                            .ok();
                        }

                        let progress = (i + 1) as f32 / total as f32;
                        let ui_weak = ui_weak.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_weak.upgrade() {
                                ui.set_progress_value(progress);
                            }
                        })
                        .ok();
                    }

                    Ok((total, errors))
                })();

                slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_is_processing(false);
                        ui.set_progress_value(1.0);
                        match res {
                            Ok((total, errors)) => {
                                if errors == 0 {
                                    ui.set_status_text(SharedString::from(format!(
                                        "PASSED! All {} files intact. ✅",
                                        total
                                    )));
                                    append_log(
                                        &ui,
                                        "[Success] Verification PASSED. All files intact.",
                                    );
                                } else {
                                    ui.set_status_text(SharedString::from(format!(
                                        "FAILED! {}/{} files corrupted. ❌",
                                        errors, total
                                    )));
                                    append_log(
                                        &ui,
                                        &format!(
                                            "[Failed] Verification FAILED. {}/{} files corrupted.",
                                            errors, total
                                        ),
                                    );
                                }
                            }
                            Err(e) => {
                                ui.set_status_text(SharedString::from(format!(
                                    "Verification error: {}",
                                    e
                                )));
                                append_log(
                                    &ui,
                                    &format!("[Error] Verification fatal error: {}", e),
                                );
                            }
                        }
                    }
                })
                .ok();
            });
        });
    }

    fn attach_run_bench_clicked(&self) {
        let ui_weak = self.ui.as_weak();
        self.ui.on_run_bench_clicked(move || {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_is_processing(true);
                ui.set_active_tab(3);
                ui.set_status_text(SharedString::from("Benchmarking hardware..."));
                append_log(
                    &ui,
                    "[Benchmark] Started Comprehensive Benchmark Suite (CPU, GPU, RAM, I/O)...",
                );

                let ui_weak_ok = ui_weak.clone();
                let ui_weak_err = ui_weak.clone();
                let ui_weak_done = ui_weak.clone();

                std::thread::spawn(move || {
                    let res_str = match benchmark::run_benchmark_suite_string(None) {
                        Ok(s) => {
                            slint::invoke_from_event_loop(move || {
                                if let Some(ui2) = ui_weak_ok.upgrade() {
                                    append_log(&ui2, "[Benchmark] Suite finished successfully.");
                                }
                            })
                            .ok();
                            s
                        }
                        Err(e) => {
                            let err_msg = format!("Benchmark failed: {}", e);
                            let err_msg_clone = err_msg.clone();
                            slint::invoke_from_event_loop(move || {
                                if let Some(ui2) = ui_weak_err.upgrade() {
                                    append_log(&ui2, &format!("[Error] {}", err_msg_clone));
                                }
                            })
                            .ok();
                            err_msg
                        }
                    };

                    slint::invoke_from_event_loop(move || {
                        if let Some(ui2) = ui_weak_done.upgrade() {
                            ui2.set_is_processing(false);
                            append_log(&ui2, &res_str);
                            ui2.set_status_text(SharedString::from("Benchmark complete."));
                        }
                    })
                    .ok();
                });
            }
        });
    }

    fn attach_run_custom_bench_clicked(&self) {
        let ui_weak = self.ui.as_weak();
        self.ui.on_run_custom_bench_clicked(move || {
            if let Some(file) = rfd::FileDialog::new().pick_file()
                && let Some(ui) = ui_weak.upgrade()
            {
                ui.set_is_processing(true);
                ui.set_active_tab(3);
                ui.set_status_text(SharedString::from("Running custom benchmark..."));
                append_log(
                    &ui,
                    &format!("[Benchmark] Started custom benchmark for {:?}", file),
                );

                let ui_weak_ok = ui_weak.clone();
                let ui_weak_err = ui_weak.clone();
                let ui_weak_done = ui_weak.clone();

                std::thread::spawn(move || {
                    let res_str = match benchmark::run_benchmark_suite_string(Some(&file)) {
                        Ok(s) => {
                            slint::invoke_from_event_loop(move || {
                                if let Some(ui2) = ui_weak_ok.upgrade() {
                                    append_log(
                                        &ui2,
                                        "[Benchmark] Custom benchmark finished successfully.",
                                    );
                                }
                            })
                            .ok();
                            s
                        }
                        Err(e) => {
                            let err_msg = format!("Custom benchmark failed: {}", e);
                            let err_msg_clone = err_msg.clone();
                            slint::invoke_from_event_loop(move || {
                                if let Some(ui2) = ui_weak_err.upgrade() {
                                    append_log(&ui2, &format!("[Error] {}", err_msg_clone));
                                }
                            })
                            .ok();
                            err_msg
                        }
                    };

                    slint::invoke_from_event_loop(move || {
                        if let Some(ui2) = ui_weak_done.upgrade() {
                            ui2.set_is_processing(false);
                            append_log(&ui2, &res_str);
                            ui2.set_status_text(SharedString::from("Custom benchmark complete."));
                        }
                    })
                    .ok();
                });
            }
        });
    }
}

pub fn get_all_entries_filtered(archive: &GameArchive) -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    for entry in archive.get_all_entries()? {
        let id = Uuid::from_bytes(entry.asset_id);
        if !id.is_nil() && (entry.flags & FLAG_DELETED) == 0 && seen.insert(id) {
            entries.push(entry);
        }
    }

    Ok(entries)
}

pub fn apply_settings_to_ui(ui: &AppWindow, settings: &AppSettings) {
    ui.set_preset_index(settings.preset_index);
    ui.set_method_index(settings.method_index);
    ui.set_compression_level(settings.compression_level);
    ui.set_partition_size_index(settings.partition_size_index);
    ui.set_enable_deduplication(settings.enable_deduplication);
    ui.set_validate_chunks(settings.validate_chunks);
    ui.set_atg_profile(settings.atg_profile);
    ui.set_tiled_streaming_enabled(settings.tiled_streaming);
    ui.set_min_tiled_res_index(settings.min_tiled_res_index);

    // MiniDXNN Options Bridge
    ui.set_ntc_enabled(settings.ntc_enabled);
    ui.set_ntc_target_bpp(settings.ntc_target_bpp);
    ui.set_ntc_encoding_index(settings.ntc_encoding_index);
    ui.set_ntc_grid_res_index(settings.ntc_grid_res_index);
    ui.set_ntc_optimizer_index(settings.ntc_optimizer_index);
    ui.set_ntc_quality_index(settings.ntc_quality_index);
    ui.set_ntc_auto_bundle(settings.ntc_auto_bundle);
    ui.set_ntc_precompute_bc7_modes(settings.ntc_precompute_bc7_modes);
    ui.set_ntc_wave_reduced_accum(settings.ntc_wave_reduced_accum);
    ui.set_ntc_inference_mode_index(settings.ntc_inference_mode_index);

    // Suffix Rules
    ui.set_pbr_suffix_albedo(SharedString::from(&settings.pbr_suffix_albedo));
    ui.set_pbr_suffix_normal(SharedString::from(&settings.pbr_suffix_normal));
    ui.set_pbr_suffix_metal(SharedString::from(&settings.pbr_suffix_metallic));
    ui.set_pbr_suffix_rough(SharedString::from(&settings.pbr_suffix_roughness));
    ui.set_pbr_suffix_ao(SharedString::from(&settings.pbr_suffix_ao));
    ui.set_pbr_suffix_displ(SharedString::from(&settings.pbr_suffix_displacement));

    ui.set_gacl_enabled(true);
    ui.set_gacl_auto_mode(settings.gacl_auto_mode);
    ui.set_gacl_bc1_index(settings.gacl_bc1_index);
    ui.set_gacl_bc2_index(settings.gacl_bc2_index);
    ui.set_gacl_bc3_index(settings.gacl_bc3_index);
    ui.set_gacl_bc4_index(settings.gacl_bc4_index);
    ui.set_gacl_bc5_index(settings.gacl_bc5_index);
    ui.set_gacl_bc6h_index(settings.gacl_bc6h_index);
    ui.set_gacl_bc7_index(settings.gacl_bc7_index);

    ui.set_rdo_enabled(settings.rdo_enabled);
    ui.set_rdo_reduction_pct(settings.rdo_reduction_pct);
    ui.set_rdo_use_ycocg(settings.rdo_use_ycocg);
    ui.set_rdo_bc1(settings.rdo_bc1);
    ui.set_rdo_bc2(settings.rdo_bc2);
    ui.set_rdo_bc3(settings.rdo_bc3);
    ui.set_rdo_bc4(settings.rdo_bc4);
    ui.set_rdo_bc5(settings.rdo_bc5);
    ui.set_rdo_bc6h(settings.rdo_bc6h);
    ui.set_rdo_bc7(settings.rdo_bc7);

    ui.set_mip_split_enabled(settings.mip_split_enabled);
    ui.set_max_tail_res_index(settings.max_tail_res_index);

    ui.set_prefer_gpu_decompression(settings.prefer_gpu_decompression);
    ui.set_staging_buffer_size_index(settings.staging_buffer_size_index);
    ui.set_default_queue_priority_index(settings.default_queue_priority_index);

    ui.set_decondition_gacl_preview(settings.decondition_gacl_preview);
    ui.set_reconstruct_normal_z(settings.reconstruct_normal_z);
    ui.set_show_tile_grid(settings.show_tile_grid);
    ui.set_tonemap_mode_index(settings.tonemap_mode_index);
    ui.set_bg_mode_index(settings.bg_mode_index);
    ui.set_channel_r(settings.channel_r);
    ui.set_channel_g(settings.channel_g);
    ui.set_channel_b(settings.channel_b);
    ui.set_channel_a(settings.channel_a);
    ui.set_raw_extraction_enabled(settings.raw_extraction_enabled);
    ui.set_map_color_by_partition(settings.map_color_by_partition);
}

pub fn save_ui_settings(ui: &AppWindow) {
    let settings = AppSettings {
        preset_index: ui.get_preset_index(),
        method_index: ui.get_method_index(),
        compression_level: ui.get_compression_level(),
        partition_size_index: ui.get_partition_size_index(),
        enable_deduplication: ui.get_enable_deduplication(),
        validate_chunks: ui.get_validate_chunks(),
        atg_profile: ui.get_atg_profile(),
        tiled_streaming: ui.get_tiled_streaming_enabled(),
        min_tiled_res_index: ui.get_min_tiled_res_index(),
        min_tiled_tile_count: 8,

        ntc_enabled: ui.get_ntc_enabled(),
        ntc_target_bpp: ui.get_ntc_target_bpp(),
        ntc_encoding_index: ui.get_ntc_encoding_index(),
        ntc_grid_res_index: ui.get_ntc_grid_res_index(),
        ntc_optimizer_index: ui.get_ntc_optimizer_index(),
        ntc_quality_index: ui.get_ntc_quality_index(),
        ntc_auto_bundle: ui.get_ntc_auto_bundle(),
        ntc_precompute_bc7_modes: ui.get_ntc_precompute_bc7_modes(),
        ntc_wave_reduced_accum: ui.get_ntc_wave_reduced_accum(),
        ntc_inference_mode_index: ui.get_ntc_inference_mode_index(),

        pbr_suffix_albedo: ui.get_pbr_suffix_albedo().to_string(),
        pbr_suffix_normal: ui.get_pbr_suffix_normal().to_string(),
        pbr_suffix_metallic: ui.get_pbr_suffix_metal().to_string(),
        pbr_suffix_roughness: ui.get_pbr_suffix_rough().to_string(),
        pbr_suffix_ao: ui.get_pbr_suffix_ao().to_string(),
        pbr_suffix_displacement: ui.get_pbr_suffix_displ().to_string(),

        gacl_auto_mode: ui.get_gacl_auto_mode(),
        gacl_bc1_index: ui.get_gacl_bc1_index(),
        gacl_bc2_index: ui.get_gacl_bc2_index(),
        gacl_bc3_index: ui.get_gacl_bc3_index(),
        gacl_bc4_index: ui.get_gacl_bc4_index(),
        gacl_bc5_index: ui.get_gacl_bc5_index(),
        gacl_bc6h_index: ui.get_gacl_bc6h_index(),
        gacl_bc7_index: ui.get_gacl_bc7_index(),

        rdo_enabled: ui.get_rdo_enabled(),
        rdo_reduction_pct: ui.get_rdo_reduction_pct(),
        rdo_use_ycocg: ui.get_rdo_use_ycocg(),
        rdo_bc1: ui.get_rdo_bc1(),
        rdo_bc2: ui.get_rdo_bc2(),
        rdo_bc3: ui.get_rdo_bc3(),
        rdo_bc4: ui.get_rdo_bc4(),
        rdo_bc5: ui.get_rdo_bc5(),
        rdo_bc6h: ui.get_rdo_bc6h(),
        rdo_bc7: ui.get_rdo_bc7(),

        mip_split_enabled: ui.get_mip_split_enabled(),
        max_tail_res_index: ui.get_max_tail_res_index(),

        prefer_gpu_decompression: ui.get_prefer_gpu_decompression(),
        staging_buffer_size_index: ui.get_staging_buffer_size_index(),
        default_queue_priority_index: ui.get_default_queue_priority_index(),

        cdn_cache_size_mb: 256,

        decondition_gacl_preview: ui.get_decondition_gacl_preview(),
        reconstruct_normal_z: ui.get_reconstruct_normal_z(),
        show_tile_grid: ui.get_show_tile_grid(),
        tonemap_mode_index: ui.get_tonemap_mode_index(),
        bg_mode_index: ui.get_bg_mode_index(),
        channel_r: ui.get_channel_r(),
        channel_g: ui.get_channel_g(),
        channel_b: ui.get_channel_b(),
        channel_a: ui.get_channel_a(),
        raw_extraction_enabled: ui.get_raw_extraction_enabled(),
        map_color_by_partition: ui.get_map_color_by_partition(),
    };
    settings::save_settings(&settings);
}

pub fn append_log(ui: &AppWindow, msg: &str) {
    let mut current = ui.get_log_text().to_string();
    current.push_str("\n> ");
    current.push_str(msg);
    ui.set_log_text(SharedString::from(current));
    gpck_core::core::logger::log_info(msg);
}
