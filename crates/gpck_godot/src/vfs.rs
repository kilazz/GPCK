// crates/gpck_godot/src/vfs.rs
//! # GPCK Godot Virtual File System (VFS) Controller
//!
//! Provides runtime archive mounting with priority mod stacks, memory-mapped zero-copy queries,
//! 64KB sparse tile DirectStorage GPU streaming, Sampler Feedback processing,
//! and in-engine package compilation with configurable auto-tiling thresholds.

use godot::global::Error;
use godot::prelude::*;

use gpck_core::compression::codecs::CompressionMethod;
use gpck_core::core::asset_id::AssetIdGenerator;
use gpck_core::crypto::aes_gcm::derive_key;
use gpck_core::format::archive::TAG_BASE_GAME;
use gpck_core::gacl::GaclTransform;
use gpck_core::graphics::dxgi_format::D3D12FormatTable;
use gpck_core::io::vfs::VirtualFileSystem;
use gpck_core::packer::{AssetPacker, GaclFormatOverrides, PackerOptions};

#[cfg(windows)]
use gpck_core::gpu::directstorage::{GpuDirectStorage, QueuePriority};
#[cfg(windows)]
use gpck_core::gpu::directstorage_sys::*;

use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};
use uuid::Uuid;

/// Global VFS instance shared between `GpckVfs` and `GpckResourceFormatLoader`.
pub fn get_global_vfs() -> &'static Arc<RwLock<VirtualFileSystem>> {
    static GLOBAL_VFS: OnceLock<Arc<RwLock<VirtualFileSystem>>> = OnceLock::new();
    GLOBAL_VFS.get_or_init(|| Arc::new(RwLock::new(VirtualFileSystem::new())))
}

/// Normalizes Godot `res://` paths to standard filesystem paths.
fn normalize_godot_path(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("res://") {
        stripped.to_string()
    } else {
        path.to_string()
    }
}

/// Parses a GACL transform from either string label or integer index.
fn parse_gacl_override(val: Option<Variant>) -> Option<u32> {
    let v = val?;
    if let Ok(s) = v.try_to::<GString>() {
        let s_str = s.to_string();
        let s_lower = s_str.to_lowercase();
        match s_lower.as_str() {
            "disabled" | "raw" | "none" => Some(0),
            "bc1 linear (v1)" | "bc1_linear" => Some(1),
            "bc1 linear + z-curve" | "bc1_linear_zcurve" => Some(17),
            "bc1 5:6:5 split (v2)" | "bc1_565" => Some(32),
            "bc1 5:6:5 + z-curve" | "bc1_565_zcurve" => Some(33),
            "bc2 alpha nibble split" | "bc2_alpha_nibble" => Some(6),
            "bc3 linear (v1)" | "bc3_linear" => Some(2),
            "bc3 linear + z-curve" | "bc3_linear_zcurve" => Some(18),
            "bc3 6:6:4 split (v2)" | "bc3_664" => Some(34),
            "bc3 6:6:4 + z-curve" | "bc3_664_zcurve" => Some(35),
            "bc4 linear" | "bc4_linear" => Some(3),
            "bc4 linear + z-curve" | "bc4_linear_zcurve" => Some(19),
            "bc5 dual channel" | "bc5_dual_channel" => Some(4),
            "bc5 dual channel + z-curve" | "bc5_dual_channel_zcurve" => Some(20),
            "bc6h header/index join" | "bc6h_header_join" => Some(7),
            "bc7 mode-split (3-stream)" | "bc7_mode_split" => Some(10),
            "bc7 mode-join (24-bit)" | "bc7_mode_join" => Some(11),
            _ => None,
        }
    } else if let Ok(i) = i32::try_from_variant(&v) {
        match i {
            0 => None,
            1 => Some(0),
            _ => None,
        }
    } else {
        None
    }
}

#[derive(GodotClass)]
#[class(init, base=RefCounted)]
pub struct GpckVfs {
    base: Base<RefCounted>,
}

#[godot_api]
impl GpckVfs {
    #[func]
    pub fn mount_archive(&mut self, path: GString, passphrase: GString) -> Error {
        let path_str = path.to_string();
        let actual_path = normalize_godot_path(&path_str);
        let pass_str = passphrase.to_string();

        let key_bytes = if pass_str.is_empty() {
            None
        } else {
            Some(derive_key(&pass_str))
        };

        let vfs = get_global_vfs();
        if let Ok(mut guard) = vfs.write() {
            match guard.mount_archive_with_key(&actual_path, key_bytes) {
                Ok(_) => {
                    godot_print!("[GPCK] Successfully mounted archive: {}", actual_path);
                    Error::OK
                }
                Err(e) => {
                    godot_error!("[GPCK] Failed to mount archive '{}': {}", actual_path, e);
                    Error::ERR_CANT_OPEN
                }
            }
        } else {
            Error::ERR_LOCKED
        }
    }

    /// Mounts an archive with an explicit layer priority and label (e.g. Base Game: 0, Mod priority: 100).
    #[func]
    pub fn mount_archive_layer(
        &mut self,
        path: GString,
        passphrase: GString,
        priority: i32,
        label: GString,
    ) -> GString {
        let path_str = normalize_godot_path(&path.to_string());
        let pass_str = passphrase.to_string();
        let key_bytes = if pass_str.is_empty() {
            None
        } else {
            Some(derive_key(&pass_str))
        };

        let vfs = get_global_vfs();
        if let Ok(mut guard) = vfs.write() {
            match guard.mount_archive_layered(&path_str, key_bytes, priority, &label.to_string()) {
                Ok(mount_id) => GString::from(mount_id.to_string()),
                Err(e) => {
                    godot_error!("[GPCK VFS] Failed to mount layer '{}': {}", path_str, e);
                    GString::new()
                }
            }
        } else {
            GString::new()
        }
    }

    /// Dynamically alters the load order priority of an active mod/archive layer.
    #[func]
    pub fn set_layer_priority(&mut self, mount_id_str: GString, new_priority: i32) -> bool {
        if let Ok(id) = Uuid::parse_str(&mount_id_str.to_string()) {
            let vfs = get_global_vfs();
            if let Ok(mut guard) = vfs.write() {
                return guard.set_archive_layer_priority(id, new_priority);
            }
        }
        false
    }

    #[func]
    pub fn mount_directory(&mut self, path: GString) -> Error {
        let path_str = path.to_string();
        let actual_path = normalize_godot_path(&path_str);

        let vfs = get_global_vfs();
        if let Ok(mut guard) = vfs.write() {
            guard.mount_directory(&actual_path);
            godot_print!("[GPCK] Mounted directory: {}", actual_path);
            Error::OK
        } else {
            Error::ERR_LOCKED
        }
    }

    #[func]
    pub fn has_file(&self, virtual_path: GString) -> bool {
        let v_path = normalize_godot_path(&virtual_path.to_string());
        let id = AssetIdGenerator::generate(&v_path);
        let vfs = get_global_vfs();
        if let Ok(guard) = vfs.read() {
            guard.try_get_entry_by_id(id).is_some()
        } else {
            false
        }
    }

    #[func]
    pub fn read_file(&self, virtual_path: GString) -> PackedByteArray {
        let v_path = normalize_godot_path(&virtual_path.to_string());
        let vfs = get_global_vfs();
        if let Ok(guard) = vfs.read() {
            match guard.read_file(&v_path) {
                Ok(data) => PackedByteArray::from(data.as_slice()),
                Err(e) => {
                    godot_error!("[GPCK] Error reading asset '{}': {}", v_path, e);
                    PackedByteArray::new()
                }
            }
        } else {
            PackedByteArray::new()
        }
    }

    #[func]
    pub fn read_text(&self, virtual_path: GString) -> GString {
        let bytes = self.read_file(virtual_path);
        if bytes.is_empty() {
            return GString::new();
        }
        match std::str::from_utf8(bytes.as_slice()) {
            Ok(s) => GString::from(s),
            Err(_) => {
                godot_error!("[GPCK] Failed to decode asset as valid UTF-8 string");
                GString::new()
            }
        }
    }

    /// Returns true if DirectStorage 1.4 BypassIO GPU streaming is active on the platform.
    #[func]
    pub fn is_directstorage_supported(&self) -> bool {
        #[cfg(windows)]
        {
            if let Ok(ds) = GpuDirectStorage::new() {
                ds.is_supported()
            } else {
                false
            }
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// Queries D3D12/Vulkan 64KB Sparse Tile metadata for Godot Sparse Texture creation.
    #[func]
    pub fn get_tiled_texture_info(&self, virtual_path: GString) -> Dictionary {
        let mut dict = Dictionary::new();
        let path_str = normalize_godot_path(&virtual_path.to_string());
        let asset_id = AssetIdGenerator::generate(&path_str);

        let vfs = get_global_vfs();
        if let Ok(guard) = vfs.read()
            && let Some((entry, _)) = guard.find_entry_and_archive(asset_id)
        {
            let width = (entry.meta1 >> 16) & 0xFFFF;
            let height = entry.meta1 & 0xFFFF;
            let mip_count = (entry.meta2 >> 24) & 0xFF;
            let dxgi_fmt = GaclTransform::from_u32(entry.gacl_transform()).to_dxgi_format();

            let (tilings, packed_info, total_tiles) =
                D3D12FormatTable::calculate_subresource_tilings(
                    dxgi_fmt,
                    width.max(1),
                    height.max(1),
                    1,
                    mip_count.max(1),
                    1,
                );

            dict.set("width", width as i64);
            dict.set("height", height as i64);
            dict.set("mip_levels", mip_count as i64);
            dict.set("dxgi_format", dxgi_fmt as i64);
            dict.set("standard_mips", packed_info.num_standard_mips as i64);
            dict.set("packed_mips", packed_info.num_packed_mips as i64);
            dict.set("total_64k_tiles", total_tiles as i64);
            dict.set("standard_tile_count", tilings.len() as i64);
        }
        dict
    }

    /// Analyzes a Sampler Feedback map captured from Godot's RenderingServer
    /// and dispatches streaming requests for missing 64KB sparse tiles directly to VRAM.
    #[func]
    pub fn process_sampler_feedback(
        &self,
        virtual_path: GString,
        feedback_bytes: PackedByteArray,
        d3d12_texture_ptr: i64,
        priority: i32,
    ) -> VariantArray {
        let mut dispatched_tiles = VariantArray::new();
        let path_str = normalize_godot_path(&virtual_path.to_string());
        let asset_id = AssetIdGenerator::generate(&path_str);

        let vfs = get_global_vfs();
        if let Ok(guard) = vfs.read()
            && let Some((entry, _)) = guard.find_entry_and_archive(asset_id)
        {
            let width = (entry.meta1 >> 16) & 0xFFFF;
            let height = entry.meta1 & 0xFFFF;
            let mip_count = (entry.meta2 >> 24) & 0xFF;
            let dxgi_fmt = GaclTransform::from_u32(entry.gacl_transform()).to_dxgi_format();

            let config = gpck_core::gpu::sampler_feedback::FeedbackMapConfig::new(
                width.max(1),
                height.max(1),
                mip_count.max(1),
                dxgi_fmt,
                gpck_core::gpu::sampler_feedback::FeedbackRegionDimensions::default(),
            );

            let mut tile_pool = gpck_core::gpu::tile_pool::TilePoolManager::new(256 * 65536, None);
            let q_prio = match priority {
                2 => QueuePriority::High,
                0 => QueuePriority::Low,
                _ => QueuePriority::Normal,
            };

            let requests =
                gpck_core::gpu::sampler_feedback::SamplerFeedbackAnalyzer::extract_missing_tiles(
                    feedback_bytes.as_slice(),
                    &config,
                    asset_id,
                    d3d12_texture_ptr as *mut std::ffi::c_void,
                    &mut tile_pool,
                    q_prio,
                );

            for req in requests {
                let mut tile_dict = Dictionary::new();
                tile_dict.set("subresource", req.subresource as i64);
                tile_dict.set("tile_x", req.tile_x as i64);
                tile_dict.set("tile_y", req.tile_y as i64);
                tile_dict.set("tile_z", req.tile_z as i64);
                dispatched_tiles.push(&tile_dict.to_variant());
            }
        }

        dispatched_tiles
    }

    /// Streams a specific 64KB sparse tile directly to a D3D12 Tiled Resource from GDScript.
    #[func]
    #[allow(clippy::too_many_arguments)]
    pub fn stream_tile_to_d3d12(
        &self,
        virtual_path: GString,
        d3d12_texture_ptr: i64,
        subresource: i32,
        tile_x: i32,
        tile_y: i32,
        tile_z: i32,
        priority: i32,
    ) -> Dictionary {
        let mut result = Dictionary::new();
        #[cfg(windows)]
        {
            if d3d12_texture_ptr == 0 {
                result.set("ok", false);
                result.set("fence", 0);
                return result;
            }

            let path_str = normalize_godot_path(&virtual_path.to_string());
            let asset_id = AssetIdGenerator::generate(&path_str);

            let vfs = get_global_vfs();
            let vfs_guard = match vfs.read() {
                Ok(g) => g,
                Err(_) => {
                    result.set("ok", false);
                    result.set("fence", 0);
                    return result;
                }
            };

            let Some((entry, archive)) = vfs_guard.find_entry_and_archive(asset_id) else {
                result.set("ok", false);
                result.set("fence", 0);
                return result;
            };

            let Ok(ds) = GpuDirectStorage::new() else {
                result.set("ok", false);
                result.set("fence", 0);
                return result;
            };

            if !ds.is_supported() {
                result.set("ok", false);
                result.set("fence", 0);
                return result;
            }

            let method = CompressionMethod::from_flags(entry.flags);
            let ds_format = match method {
                CompressionMethod::GDeflate => DSTORAGE_COMPRESSION_FORMAT_GDEFLATE,
                CompressionMethod::Zstd => DSTORAGE_COMPRESSION_FORMAT_ZSTD,
                CompressionMethod::BrotliG => DSTORAGE_CUSTOM_COMPRESSION_0,
                _ => DSTORAGE_COMPRESSION_FORMAT_NONE,
            };
            let gacl_transform = entry.gacl_transform() as u8;

            let width = (entry.meta1 >> 16) & 0xFFFF;
            let height = entry.meta1 & 0xFFFF;
            let mip_count = (entry.meta2 >> 24) & 0xFF;

            let gacl_enum = GaclTransform::from_u32(entry.gacl_transform());
            let dxgi_fmt = gacl_enum.to_dxgi_format();

            let (tilings, packed_info, _total_tiles) =
                D3D12FormatTable::calculate_subresource_tilings(
                    dxgi_fmt,
                    width.max(1),
                    height.max(1),
                    1,
                    mip_count.max(1),
                    1,
                );

            let subres_u32 = subresource as u32;
            let tile_index = if subres_u32 < packed_info.num_standard_mips as u32 {
                let tiling = &tilings[subres_u32 as usize];
                tiling.start_tile_index_in_overall_resource as usize
                    + (tile_y as u32 * tiling.width_in_tiles + tile_x as u32) as usize
            } else {
                packed_info.start_tile_index_in_overall_resource as usize
            };

            let Ok(chunks) = archive.get_chunk_table(&entry) else {
                result.set("ok", false);
                result.set("fence", 0);
                return result;
            };

            let Some(chunk) = chunks.get(tile_index) else {
                result.set("ok", false);
                result.set("fence", 0);
                return result;
            };

            if chunk.offset < 0 {
                result.set("ok", false);
                result.set("fence", 0);
                return result;
            }

            let gdat_path = std::path::Path::new(archive.file_path()).with_extension("gdat");
            let Ok(dstorage_file) = ds.open_file(&gdat_path) else {
                result.set("ok", false);
                result.set("fence", 0);
                return result;
            };

            let q_prio = match priority {
                2 => QueuePriority::High,
                0 => QueuePriority::Low,
                _ => QueuePriority::Normal,
            };

            let coord = D3D12_TILED_RESOURCE_COORDINATE {
                X: tile_x as u32,
                Y: tile_y as u32,
                Z: tile_z as u32,
                Subresource: subres_u32,
            };

            let tile_region = D3D12_TILE_REGION_SIZE {
                NumTiles: 1,
                UseBox: Default::default(),
                Width: 1,
                Height: 1,
                Depth: 1,
            };

            let mut ds_req: DSTORAGE_REQUEST = unsafe { std::mem::zeroed() };
            ds_req.set_file_to_tiles(
                dstorage_file.ptr(),
                chunk.offset as u64,
                chunk.compressed_size,
                d3d12_texture_ptr as *mut std::ffi::c_void,
                coord,
                tile_region,
                chunk.original_size,
                ds_format,
                gacl_transform,
            );

            ds.enqueue_tile_request(q_prio, &ds_req);

            match ds.flush_and_signal(q_prio) {
                Ok(fence) => {
                    result.set("ok", true);
                    result.set("fence", fence as i64);
                }
                Err(_) => {
                    result.set("ok", false);
                    result.set("fence", 0);
                }
            }
            result
        }
        #[cfg(not(windows))]
        {
            let _ = (
                virtual_path,
                d3d12_texture_ptr,
                subresource,
                tile_x,
                tile_y,
                tile_z,
                priority,
            );
            result.set("ok", false);
            result.set("fence", 0);
            result
        }
    }

    /// Waits for a DirectStorage GPU fence on the CPU with timeout protection.
    #[func]
    pub fn wait_for_d3d12_fence(&self, priority: i32, fence_value: i64) -> bool {
        #[cfg(windows)]
        {
            let q_prio = match priority {
                2 => QueuePriority::High,
                0 => QueuePriority::Low,
                _ => QueuePriority::Normal,
            };

            if let Ok(ds) = GpuDirectStorage::new() {
                ds.wait_for_fence(q_prio, fence_value as u64).is_ok()
            } else {
                false
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (priority, fence_value);
            false
        }
    }

    #[func]
    pub fn pack_directory(
        &self,
        input_directory: GString,
        output_archive_path: GString,
        method_str: GString,
        compression_level: i32,
        passphrase: GString,
    ) -> bool {
        let mut dict = Dictionary::new();
        dict.set("method", method_str.to_variant());
        dict.set("level", compression_level.to_variant());
        dict.set("passphrase", passphrase.to_variant());
        self.pack_directory_with_options(input_directory, output_archive_path, dict)
    }

    #[func]
    #[allow(clippy::too_many_arguments)]
    pub fn pack_directory_advanced(
        &self,
        input_directory: GString,
        output_archive_path: GString,
        method_str: GString,
        compression_level: i32,
        passphrase: GString,
        enable_dedup: bool,
        mip_split: bool,
        max_tail_dim: i32,
        atg_profile: bool,
        rdo_enabled: bool,
        rdo_reduction_pct: f32,
        rdo_use_ycocg: bool,
    ) -> bool {
        let mut dict = Dictionary::new();
        dict.set("method", method_str.to_variant());
        dict.set("level", compression_level.to_variant());
        dict.set("passphrase", passphrase.to_variant());
        dict.set("enable_deduplication", enable_dedup.to_variant());
        dict.set("mip_split", mip_split.to_variant());
        dict.set("max_tail_dimension", max_tail_dim.to_variant());
        dict.set("atg_profile", atg_profile.to_variant());
        dict.set("rdo_enabled", rdo_enabled.to_variant());
        dict.set("rdo_reduction_pct", rdo_reduction_pct.to_variant());
        dict.set("rdo_use_ycocg", rdo_use_ycocg.to_variant());
        self.pack_directory_with_options(input_directory, output_archive_path, dict)
    }

    /// Complete Dictionary-driven packaging supporting Brotli-G, 64KB Sparse Tile Packaging,
    /// and configurable auto-thresholding.
    #[func]
    pub fn pack_directory_with_options(
        &self,
        input_directory: GString,
        output_archive_path: GString,
        options_dict: Dictionary,
    ) -> bool {
        let in_dir = normalize_godot_path(&input_directory.to_string());
        let out_arch = normalize_godot_path(&output_archive_path.to_string());

        let method_str = options_dict
            .get("method")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "zstd".to_string());
        let level = options_dict
            .get("level")
            .and_then(|v| i64::try_from_variant(&v).ok())
            .map(|v| v as i32)
            .unwrap_or(9);
        let passphrase = options_dict
            .get("passphrase")
            .map(|v| v.to_string())
            .unwrap_or_default();

        let partition_size_mb = options_dict
            .get("partition_size_mb")
            .and_then(|v| i64::try_from_variant(&v).ok())
            .map(|v| v as usize)
            .unwrap_or(64);
        let chunk_size_kb = options_dict
            .get("chunk_size_kb")
            .and_then(|v| i64::try_from_variant(&v).ok())
            .map(|v| v as usize)
            .unwrap_or(64);

        let enable_dedup = options_dict
            .get("enable_deduplication")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(true);
        let validate_chunks = options_dict
            .get("validate_chunks")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(true);
        let atg_profile = options_dict
            .get("atg_profile")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(true);
        let tiled_streaming = options_dict
            .get("tiled_streaming")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(true);

        let min_tiled_resolution = options_dict
            .get("min_tiled_resolution")
            .and_then(|v| i64::try_from_variant(&v).ok())
            .map(|v| v as usize)
            .unwrap_or(2048);

        let min_tiled_tile_count = options_dict
            .get("min_tiled_tile_count")
            .and_then(|v| i64::try_from_variant(&v).ok())
            .map(|v| v as u32)
            .unwrap_or(8);

        let mip_split = options_dict
            .get("mip_split")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(true);
        let max_tail_dim = options_dict
            .get("max_tail_dimension")
            .and_then(|v| i64::try_from_variant(&v).ok())
            .map(|v| v as usize)
            .unwrap_or(128);

        // GACL Master Switch & Overrides
        let gacl_enabled = options_dict
            .get("gacl_enabled")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(true);
        let gacl_auto_mode = options_dict
            .get("gacl_auto_mode")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(true);

        let bc1_transform = parse_gacl_override(options_dict.get("bc1_transform"));
        let bc2_transform = parse_gacl_override(options_dict.get("bc2_transform"));
        let bc3_transform = parse_gacl_override(options_dict.get("bc3_transform"));
        let bc4_transform = parse_gacl_override(options_dict.get("bc4_transform"));
        let bc5_transform = parse_gacl_override(options_dict.get("bc5_transform"));
        let bc6h_transform = parse_gacl_override(options_dict.get("bc6h_transform"));
        let bc7_transform = parse_gacl_override(options_dict.get("bc7_transform"));

        // RDO & Format Filters
        let rdo_enabled = options_dict
            .get("rdo_enabled")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(false);
        let rdo_reduction_pct = options_dict
            .get("rdo_reduction_pct")
            .and_then(|v| f64::try_from_variant(&v).ok())
            .map(|v| v as f32)
            .unwrap_or(5.0);
        let rdo_use_ycocg = options_dict
            .get("rdo_use_ycocg")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(true);

        let rdo_bc1 = options_dict
            .get("rdo_bc1")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(true);
        let rdo_bc2 = options_dict
            .get("rdo_bc2")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(true);
        let rdo_bc3 = options_dict
            .get("rdo_bc3")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(true);
        let rdo_bc4 = options_dict
            .get("rdo_bc4")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(false);
        let rdo_bc5 = options_dict
            .get("rdo_bc5")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(false);
        let rdo_bc6h = options_dict
            .get("rdo_bc6h")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(false);
        let rdo_bc7 = options_dict
            .get("rdo_bc7")
            .and_then(|v| bool::try_from_variant(&v).ok())
            .unwrap_or(true);

        let key_bytes = if passphrase.is_empty() {
            None
        } else {
            Some(derive_key(&passphrase))
        };

        let method = match method_str.to_lowercase().as_str() {
            "gdeflate" => CompressionMethod::GDeflate,
            "brotlig" | "brotli_g" | "brotli" => CompressionMethod::BrotliG,
            "lz4" => CompressionMethod::Lz4,
            "rans" => CompressionMethod::Rans,
            "store" => CompressionMethod::Store,
            _ => CompressionMethod::Zstd,
        };

        let options = PackerOptions {
            method,
            level,
            chunk_size: chunk_size_kb * 1024,
            enable_dedup,
            key: key_bytes,
            mip_split,
            max_tail_dim: max_tail_dim.max(32),
            tags: TAG_BASE_GAME,
            validate_chunks,
            max_partition_size: partition_size_mb * 1024 * 1024,
            gacl: GaclFormatOverrides {
                enabled: gacl_enabled,
                auto_mode: gacl_auto_mode,
                bc1_transform,
                bc2_transform,
                bc3_transform,
                bc4_transform,
                bc5_transform,
                bc6h_transform,
                bc7_transform,
                rdo_reduction_pct: if rdo_enabled { rdo_reduction_pct } else { 0.0 },
                rdo_use_ycocg,
                rdo_bc1,
                rdo_bc2,
                rdo_bc3,
                rdo_bc4,
                rdo_bc5,
                rdo_bc6h,
                rdo_bc7,
            },
            atg_profile,
            tiled_streaming,
            min_tiled_resolution,
            min_tiled_tile_count,
        };

        let file_map = match AssetPacker::build_file_map(Path::new(&in_dir)) {
            Ok(m) => m,
            Err(e) => {
                godot_error!(
                    "[GPCK Packer] Failed to index source directory '{}': {}",
                    in_dir,
                    e
                );
                return false;
            }
        };

        match AssetPacker::compress_files_to_archive(
            &file_map,
            Path::new(&out_arch),
            &options,
            |msg| {
                godot_print!("[GPCK Packer] {}", msg);
            },
        ) {
            Ok(_) => {
                godot_print!("[GPCK Packer] Successfully generated archive: {}", out_arch);
                true
            }
            Err(e) => {
                godot_error!("[GPCK Packer] Failed to pack archive '{}': {}", out_arch, e);
                false
            }
        }
    }

    #[func]
    pub fn get_archive_entries(&self) -> VariantArray {
        let mut array = VariantArray::new();
        let vfs = get_global_vfs();
        if let Ok(guard) = vfs.read() {
            for archive in guard.get_mounted_archives() {
                if let Ok(entries) = archive.get_all_entries() {
                    for e in entries {
                        let path = archive
                            .get_path_for_asset(&e)
                            .unwrap_or_else(|| Uuid::from_bytes(e.asset_id).to_string());
                        let method = CompressionMethod::from_flags(e.flags);
                        let gacl = GaclTransform::from_u32(e.gacl_transform());

                        let mut dict = Dictionary::new();
                        dict.set("path", GString::from(path));
                        dict.set("original_size", e.original_size as i64);
                        dict.set("compressed_size", e.compressed_size as i64);
                        dict.set(
                            "ratio",
                            if e.original_size > 0 {
                                (e.compressed_size as f64 / e.original_size as f64) * 100.0
                            } else {
                                100.0
                            },
                        );
                        dict.set("method", GString::from(format!("{:?}", method)));
                        dict.set("gacl", GString::from(gacl.display_name()));
                        dict.set("partition_id", e.partition_id as i64);

                        array.push(&dict.to_variant());
                    }
                }
            }
        }
        array
    }
}
