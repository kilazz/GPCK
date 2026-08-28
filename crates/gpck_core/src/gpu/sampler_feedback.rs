// crates/gpck_core/src/gpu/sampler_feedback.rs
//! # Direct3D 12 Sampler Feedback Map & Visible Tile Resolver
//!
//! Creates, resolves, and analyzes MinMip Sampler Feedback Maps (`DXGI_FORMAT_SAMPLER_FEEDBACK_MIN_MIP_OPAQUE`)
//! to extract the exact 64KB sparse tiles required by the GPU rasterizer in the current frame.

use super::tile_pool::{TileKey, TilePoolManager};
use crate::core::error::{GpckError, GpckResult};
use crate::gpu::directstorage::QueuePriority;
use crate::graphics::dxgi_format::D3D12FormatTable;
use crate::io::resource_manager::VramTileStreamRequest;
use std::collections::HashSet;
use uuid::Uuid;

#[cfg(windows)]
use windows::Win32::Graphics::Direct3D12::*;
#[cfg(windows)]
use windows::Win32::Graphics::Dxgi::Common::*;

/// Sampling region dimensions per Feedback Map texel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedbackRegionDimensions {
    pub width: u32,
    pub height: u32,
}

impl Default for FeedbackRegionDimensions {
    fn default() -> Self {
        Self {
            width: 16,
            height: 16,
        } // Standard 16x16 texels per feedback texel
    }
}

/// Configuration describing the paired texture and its feedback map layout.
#[derive(Debug, Clone)]
pub struct FeedbackMapConfig {
    pub paired_width: u32,
    pub paired_height: u32,
    pub mip_levels: u32,
    pub dxgi_format: u32,
    pub region: FeedbackRegionDimensions,
    pub feedback_width: u32,
    pub feedback_height: u32,
}

impl FeedbackMapConfig {
    pub fn new(
        paired_width: u32,
        paired_height: u32,
        mip_levels: u32,
        dxgi_format: u32,
        region: FeedbackRegionDimensions,
    ) -> Self {
        let feedback_width = paired_width.div_ceil(region.width);
        let feedback_height = paired_height.div_ceil(region.height);

        Self {
            paired_width,
            paired_height,
            mip_levels,
            dxgi_format,
            region,
            feedback_width,
            feedback_height,
        }
    }

    #[inline(always)]
    pub fn feedback_byte_size(&self) -> usize {
        (self.feedback_width * self.feedback_height) as usize
    }
}

pub struct SamplerFeedbackAnalyzer;

impl SamplerFeedbackAnalyzer {
    /// Analyzes readback feedback map bytes and generates stream requests for missing tiles.
    pub fn extract_missing_tiles(
        feedback_data: &[u8],
        config: &FeedbackMapConfig,
        asset_id: Uuid,
        dest_resource_ptr: *mut std::ffi::c_void,
        tile_pool: &mut TilePoolManager,
        priority: QueuePriority,
    ) -> Vec<VramTileStreamRequest> {
        let mut missing_keys = HashSet::new();
        let tile_shape = D3D12FormatTable::get_tile_shape_64k(config.dxgi_format, false);
        let tile_w = tile_shape.width_in_texels.max(1);
        let tile_h = tile_shape.height_in_texels.max(1);

        for fy in 0..config.feedback_height {
            for fx in 0..config.feedback_width {
                let idx = (fy * config.feedback_width + fx) as usize;
                let min_mip_byte = feedback_data.get(idx).copied().unwrap_or(0xFF);

                // 0xFF indicates the region was not sampled in this frame
                if min_mip_byte == 0xFF {
                    continue;
                }

                let min_mip = (min_mip_byte as u32).min(config.mip_levels.saturating_sub(1));

                // Calculate texel space coordinates at the sampled mip level
                let texel_x = (fx * config.region.width) >> min_mip;
                let texel_y = (fy * config.region.height) >> min_mip;

                let tile_x = texel_x / tile_w;
                let tile_y = texel_y / tile_h;

                let key = TileKey::new(asset_id, min_mip, tile_x, tile_y);

                if tile_pool.is_tile_resident(&key) {
                    tile_pool.touch_tile(&key);
                } else {
                    missing_keys.insert(key);
                }
            }
        }

        let requested_keys: Vec<TileKey> = missing_keys.into_iter().collect();
        let plan = tile_pool.allocate_tiles(&requested_keys);

        plan.newly_mapped
            .into_iter()
            .map(|(key, _)| VramTileStreamRequest {
                asset_id: key.asset_id,
                dest_resource_ptr,
                subresource: key.subresource,
                tile_x: key.tile_x,
                tile_y: key.tile_y,
                tile_z: key.tile_z,
                cancellation_tag: 0,
                priority,
            })
            .collect()
    }
}

/// GPU Sampler Feedback Resource & Readback Buffer Wrapper.
#[cfg(windows)]
pub struct SamplerFeedbackTexture {
    pub feedback_resource: ID3D12Resource,
    pub readback_buffer: ID3D12Resource,
    pub config: FeedbackMapConfig,
}

#[cfg(windows)]
impl SamplerFeedbackTexture {
    /// Creates a D3D12 MinMip Sampler Feedback resource and its paired readback buffer.
    pub fn create(device: &ID3D12Device, config: FeedbackMapConfig) -> GpckResult<Self> {
        unsafe {
            // 1. Create Sampler Feedback Resource
            let heap_props = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                ..Default::default()
            };

            let res_desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Alignment: 0,
                Width: config.feedback_width as u64,
                Height: config.feedback_height,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_SAMPLER_FEEDBACK_MIN_MIP_OPAQUE,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
                Flags: D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
            };

            let mut feedback_resource: Option<ID3D12Resource> = None;
            device
                .CreateCommittedResource(
                    &heap_props,
                    D3D12_HEAP_FLAG_NONE,
                    &res_desc,
                    D3D12_RESOURCE_STATE_UNORDERED_ACCESS,
                    None,
                    &mut feedback_resource,
                )
                .map_err(|e| GpckError::DirectStorageError {
                    hresult: e.code().0 as u32,
                    message: "CreateCommittedResource for Sampler Feedback failed",
                })?;

            let feedback_resource = feedback_resource.unwrap();

            // 2. Create Readback Buffer
            let readback_props = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_READBACK,
                ..Default::default()
            };

            let aligned_byte_size = (config.feedback_byte_size() as u64 + 511) & !511;

            let readback_desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Alignment: 0,
                Width: aligned_byte_size,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: DXGI_FORMAT_UNKNOWN,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                Flags: D3D12_RESOURCE_FLAG_NONE,
            };

            let mut readback_buffer: Option<ID3D12Resource> = None;
            device
                .CreateCommittedResource(
                    &readback_props,
                    D3D12_HEAP_FLAG_NONE,
                    &readback_desc,
                    D3D12_RESOURCE_STATE_COPY_DEST,
                    None,
                    &mut readback_buffer,
                )
                .map_err(|e| GpckError::DirectStorageError {
                    hresult: e.code().0 as u32,
                    message: "CreateCommittedResource for Feedback Readback Buffer failed",
                })?;

            let readback_buffer = readback_buffer.unwrap();

            Ok(Self {
                feedback_resource,
                readback_buffer,
                config,
            })
        }
    }

    /// Reads mapped feedback bytes from CPU visible readback memory.
    pub fn read_feedback_bytes(&self) -> GpckResult<Vec<u8>> {
        let size = self.config.feedback_byte_size();
        let mut result = vec![0u8; size];

        unsafe {
            let mut mapped_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
            let read_range = D3D12_RANGE {
                Begin: 0,
                End: size,
            };

            self.readback_buffer
                .Map(0, Some(&read_range), Some(&mut mapped_ptr))
                .map_err(|e| GpckError::DirectStorageError {
                    hresult: e.code().0 as u32,
                    message: "Failed to Map Feedback Readback Buffer",
                })?;

            if !mapped_ptr.is_null() {
                std::ptr::copy_nonoverlapping(mapped_ptr as *const u8, result.as_mut_ptr(), size);
            }

            self.readback_buffer.Unmap(0, None);
        }

        Ok(result)
    }
}
