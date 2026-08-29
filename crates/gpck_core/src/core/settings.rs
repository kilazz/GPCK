// crates/gpck_core/src/core/settings.rs
//! # Application Configuration & JSON Preset Persistence

use super::paths::GpckPaths;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    pub preset_index: i32,
    pub method_index: i32,
    pub compression_level: i32,
    pub partition_size_index: i32,
    pub enable_deduplication: bool,
    pub validate_chunks: bool,
    pub atg_profile: bool,
    pub tiled_streaming: bool,
    pub min_tiled_res_index: i32,
    pub min_tiled_tile_count: u32,

    // MiniDXNN Native Neural Settings
    pub ntc_enabled: bool,
    pub ntc_target_bpp: f32,
    pub ntc_encoding_index: i32, // 0 = Bilinear Grid, 1 = Positional, 2 = Raw UV
    pub ntc_grid_res_index: i32, // 0 = 32x32, 1 = 64x64, 2 = 128x128
    pub ntc_optimizer_index: i32, // 0 = Lion, 1 = Adam, 2 = SGD
    pub ntc_quality_index: i32,  // 0 = 5 Epochs, 1 = 30 Epochs, 2 = 100 Epochs
    pub ntc_auto_bundle: bool,
    pub ntc_precompute_bc7_modes: bool,
    pub ntc_wave_reduced_accum: bool,
    pub ntc_inference_mode_index: i32, // 0 = DP4a Universal, 1 = LinAlg SM 6.10, 2 = CPU SIMD

    // Customizable PBR Suffixes (Comma-separated)
    pub pbr_suffix_albedo: String,
    pub pbr_suffix_normal: String,
    pub pbr_suffix_metallic: String,
    pub pbr_suffix_roughness: String,
    pub pbr_suffix_ao: String,
    pub pbr_suffix_displacement: String,

    // GACL Settings
    pub gacl_auto_mode: bool,
    pub gacl_bc1_index: i32,
    pub gacl_bc2_index: i32,
    pub gacl_bc3_index: i32,
    pub gacl_bc4_index: i32,
    pub gacl_bc5_index: i32,
    pub gacl_bc6h_index: i32,
    pub gacl_bc7_index: i32,

    // RDO Settings & Format Filters
    pub rdo_enabled: bool,
    pub rdo_reduction_pct: f32,
    pub rdo_use_ycocg: bool,
    pub rdo_bc1: bool,
    pub rdo_bc2: bool,
    pub rdo_bc3: bool,
    pub rdo_bc4: bool,
    pub rdo_bc5: bool,
    pub rdo_bc6h: bool,
    pub rdo_bc7: bool,

    // Texture Streaming Settings
    pub mip_split_enabled: bool,
    pub max_tail_res_index: i32,

    // GPU Acceleration Settings
    pub prefer_gpu_decompression: bool,
    pub staging_buffer_size_index: i32,
    pub default_queue_priority_index: i32,

    // Remote CDN & Memory Cache
    pub cdn_cache_size_mb: usize,

    // UI & Inspector Preferences
    pub decondition_gacl_preview: bool,
    pub reconstruct_normal_z: bool,
    pub show_tile_grid: bool,
    pub tonemap_mode_index: i32,
    pub bg_mode_index: i32,
    pub channel_r: bool,
    pub channel_g: bool,
    pub channel_b: bool,
    pub channel_a: bool,
    pub raw_extraction_enabled: bool,
    pub map_color_by_partition: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            preset_index: 0,
            method_index: 0,
            compression_level: 9,
            partition_size_index: 1,
            enable_deduplication: true,
            validate_chunks: true,
            atg_profile: true,
            tiled_streaming: true,
            min_tiled_res_index: 0,
            min_tiled_tile_count: 8,

            ntc_enabled: false,
            ntc_target_bpp: 5.0,
            ntc_encoding_index: 0,
            ntc_grid_res_index: 1,
            ntc_optimizer_index: 0,
            ntc_quality_index: 1,
            ntc_auto_bundle: true,
            ntc_precompute_bc7_modes: true,
            ntc_wave_reduced_accum: true,
            ntc_inference_mode_index: 0,

            // Smart PBR Suffix Defaults
            pbr_suffix_albedo: "_diff, _albedo, _basecolor, _color, _col, _d, _alb".to_string(),
            pbr_suffix_normal: "_ddn, _ddna, _normal, _norm, _nrm, _n, _nor".to_string(),
            pbr_suffix_metallic: "_spec, _specular, _metal, _metallic, _metalness, _m, _met"
                .to_string(),
            pbr_suffix_roughness: "_gloss, _rough, _roughness, _r, _rgh".to_string(),
            pbr_suffix_ao: "_ao, _ambient, _occlusion, _ambientocclusion".to_string(),
            pbr_suffix_displacement: "_displ, _disp, _height, _h, _bump".to_string(),

            gacl_auto_mode: true,
            gacl_bc1_index: 0,
            gacl_bc2_index: 0,
            gacl_bc3_index: 0,
            gacl_bc4_index: 0,
            gacl_bc5_index: 0,
            gacl_bc6h_index: 0,
            gacl_bc7_index: 0,

            rdo_enabled: false,
            rdo_reduction_pct: 5.0,
            rdo_use_ycocg: true,
            rdo_bc1: true,
            rdo_bc2: true,
            rdo_bc3: true,
            rdo_bc4: false,
            rdo_bc5: false,
            rdo_bc6h: false,
            rdo_bc7: true,

            mip_split_enabled: true,
            max_tail_res_index: 1,

            prefer_gpu_decompression: true,
            staging_buffer_size_index: 2,
            default_queue_priority_index: 0,

            cdn_cache_size_mb: 256,

            decondition_gacl_preview: true,
            reconstruct_normal_z: false,
            show_tile_grid: false,
            tonemap_mode_index: 0,
            bg_mode_index: 0,
            channel_r: true,
            channel_g: true,
            channel_b: true,
            channel_a: true,
            raw_extraction_enabled: false,
            map_color_by_partition: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CustomPresetConfig {
    pub name: String,
    pub method: String,
    pub level: i32,
    pub atg_profile: bool,
    pub partition_size_mb: usize,
    pub mip_split: bool,
    pub max_tail_dim: usize,
    pub enable_dedup: bool,
    pub validate_chunks: bool,
    pub tiled_streaming: bool,
    pub min_tiled_resolution: usize,
    pub min_tiled_tile_count: u32,
    pub rdo_reduction_pct: f32,
    pub rdo_use_ycocg: bool,
}

pub fn get_portable_config_dir() -> PathBuf {
    GpckPaths::get_config_dir()
}

pub fn load_settings() -> AppSettings {
    let settings_file = get_portable_config_dir().join("Settings.json");
    if settings_file.exists()
        && let Ok(content) = fs::read_to_string(&settings_file)
        && let Ok(settings) = serde_json::from_str::<AppSettings>(&content)
    {
        return settings;
    }
    AppSettings::default()
}

pub fn save_settings(settings: &AppSettings) {
    let settings_file = get_portable_config_dir().join("Settings.json");
    if let Ok(json_str) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(settings_file, json_str);
    }
}

pub fn load_custom_presets() -> Vec<CustomPresetConfig> {
    let presets_file = get_portable_config_dir().join("presets.json");
    if presets_file.exists()
        && let Ok(content) = fs::read_to_string(&presets_file)
        && let Ok(presets) = serde_json::from_str::<Vec<CustomPresetConfig>>(&content)
    {
        return presets;
    }
    Vec::new()
}

pub fn save_custom_presets(presets: &[CustomPresetConfig]) {
    let presets_file = get_portable_config_dir().join("presets.json");
    if let Ok(json_str) = serde_json::to_string_pretty(presets) {
        let _ = fs::write(presets_file, json_str);
    }
}
