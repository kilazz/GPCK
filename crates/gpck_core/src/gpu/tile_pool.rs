// crates/gpck_core/src/gpu/tile_pool.rs
//! # Direct3D 12 & Vulkan 64KB Sparse Tile Pool Manager & LRU Residency Cache

use crate::core::error::{GpckError, GpckResult};
use crate::graphics::dxgi_format::D3D12FormatTable;
use ash::vk;
use std::collections::{HashMap, VecDeque};
use uuid::Uuid;

#[cfg(windows)]
use crate::gpu::directstorage_sys::{D3D12_TILE_REGION_SIZE, D3D12_TILED_RESOURCE_COORDINATE};
#[cfg(windows)]
use windows::Win32::Graphics::Direct3D12::*;
#[cfg(windows)]
use windows::core::BOOL;

/// Unique 64KB sparse tile coordinate identifier within an asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub asset_id: Uuid,
    pub subresource: u32,
    pub tile_x: u32,
    pub tile_y: u32,
    pub tile_z: u32,
}

impl TileKey {
    #[inline(always)]
    pub fn new(asset_id: Uuid, subresource: u32, tile_x: u32, tile_y: u32) -> Self {
        Self {
            asset_id,
            subresource,
            tile_x,
            tile_y,
            tile_z: 0,
        }
    }
}

/// Result of tile allocation containing newly assigned slots and evicted slots.
#[derive(Debug, Clone, Default)]
pub struct TileAllocationPlan {
    pub newly_mapped: Vec<(TileKey, u32)>, // (TileKey, physical_slot_index)
    pub evicted: Vec<(TileKey, u32)>,      // (TileKey, freed_slot_index)
}

pub struct TilePoolManager {
    total_tiles: usize,
    free_slots: Vec<u32>,
    resident_tiles: HashMap<TileKey, u32>,
    slot_to_tile: HashMap<u32, TileKey>,
    lru_queue: VecDeque<TileKey>,
    #[cfg(windows)]
    heap: Option<ID3D12Heap>,
    vk_memory_pool: Option<vk::DeviceMemory>,
}

unsafe impl Send for TilePoolManager {}
unsafe impl Sync for TilePoolManager {}

impl TilePoolManager {
    /// Creates a new physical tile pool with a fixed memory budget.
    pub fn new(capacity_bytes: u64, _device: Option<&ash::Device>) -> Self {
        let total_tiles = (capacity_bytes / 65536).max(1) as usize;
        let free_slots: Vec<u32> = (0..total_tiles as u32).rev().collect();

        Self {
            total_tiles,
            free_slots,
            resident_tiles: HashMap::with_capacity(total_tiles),
            slot_to_tile: HashMap::with_capacity(total_tiles),
            lru_queue: VecDeque::with_capacity(total_tiles),
            #[cfg(windows)]
            heap: None,
            vk_memory_pool: None,
        }
    }

    #[cfg(windows)]
    pub fn set_d3d12_heap(&mut self, heap: ID3D12Heap) {
        self.heap = Some(heap);
    }

    #[cfg(windows)]
    pub fn heap_ptr(&self) -> Option<&ID3D12Heap> {
        self.heap.as_ref()
    }

    /// Associates an allocated Vulkan device memory pool (`VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT`) with the manager.
    pub fn set_vulkan_memory_pool(&mut self, memory: vk::DeviceMemory) {
        self.vk_memory_pool = Some(memory);
    }

    pub fn vulkan_memory_pool(&self) -> Option<vk::DeviceMemory> {
        self.vk_memory_pool
    }

    #[inline(always)]
    pub fn is_tile_resident(&self, key: &TileKey) -> bool {
        self.resident_tiles.contains_key(key)
    }

    #[inline(always)]
    pub fn get_physical_slot(&self, key: &TileKey) -> Option<u32> {
        self.resident_tiles.get(key).copied()
    }

    pub fn touch_tile(&mut self, key: &TileKey) {
        if self.resident_tiles.contains_key(key) {
            if let Some(pos) = self.lru_queue.iter().position(|k| k == key) {
                self.lru_queue.remove(pos);
            }
            self.lru_queue.push_back(*key);
        }
    }

    pub fn allocate_tiles(&mut self, requested_keys: &[TileKey]) -> TileAllocationPlan {
        let mut plan = TileAllocationPlan::default();

        for &key in requested_keys {
            if self.is_tile_resident(&key) {
                self.touch_tile(&key);
                continue;
            }

            let slot = if let Some(free_slot) = self.free_slots.pop() {
                free_slot
            } else if let Some(evicted_key) = self.lru_queue.pop_front() {
                if let Some(evicted_slot) = self.resident_tiles.remove(&evicted_key) {
                    self.slot_to_tile.remove(&evicted_slot);
                    plan.evicted.push((evicted_key, evicted_slot));
                    evicted_slot
                } else {
                    continue;
                }
            } else {
                continue;
            };

            self.resident_tiles.insert(key, slot);
            self.slot_to_tile.insert(slot, key);
            self.lru_queue.push_back(key);
            plan.newly_mapped.push((key, slot));
        }

        plan
    }

    /// Synchronizes virtual-to-physical tile mappings on a DirectX 12 Command Queue.
    ///
    /// # Safety
    /// - `command_queue` must point to an active and valid `ID3D12CommandQueue`.
    /// - `tiled_resource` must be a valid reserved `ID3D12Resource` created with tiled layout.
    /// - The allocated D3D12 tile pool heap (`self.heap`) must remain valid and bound during execution.
    #[cfg(windows)]
    pub unsafe fn update_gpu_tile_mappings(
        &self,
        command_queue: &ID3D12CommandQueue,
        tiled_resource: &ID3D12Resource,
        plan: &TileAllocationPlan,
    ) -> GpckResult<()> {
        let heap = self
            .heap
            .as_ref()
            .ok_or(GpckError::DirectStorageUnsupported)?;

        // 1. Unmap Evicted Tiles (Map to NULL)
        for (evicted_key, _) in &plan.evicted {
            let coord = D3D12_TILED_RESOURCE_COORDINATE {
                X: evicted_key.tile_x,
                Y: evicted_key.tile_y,
                Z: evicted_key.tile_z,
                Subresource: evicted_key.subresource,
            };
            let region = D3D12_TILE_REGION_SIZE {
                NumTiles: 1,
                UseBox: BOOL(0),
                Width: 1,
                Height: 1,
                Depth: 1,
            };
            let range_flags = D3D12_TILE_RANGE_FLAG_NULL;
            let tile_count = 1u32;

            unsafe {
                command_queue.UpdateTileMappings(
                    tiled_resource,
                    1,
                    Some(&coord),
                    Some(&region),
                    None,
                    1,
                    Some(&range_flags),
                    None,
                    Some(&tile_count),
                    D3D12_TILE_MAPPING_FLAG_NONE,
                );
            }
        }

        // 2. Map Newly Allocated Physical Slots
        for (key, slot) in &plan.newly_mapped {
            let coord = D3D12_TILED_RESOURCE_COORDINATE {
                X: key.tile_x,
                Y: key.tile_y,
                Z: key.tile_z,
                Subresource: key.subresource,
            };
            let region = D3D12_TILE_REGION_SIZE {
                NumTiles: 1,
                UseBox: BOOL(0),
                Width: 1,
                Height: 1,
                Depth: 1,
            };
            let range_flags = D3D12_TILE_RANGE_FLAG_NONE;
            let heap_offset = *slot;
            let tile_count = 1u32;

            unsafe {
                command_queue.UpdateTileMappings(
                    tiled_resource,
                    1,
                    Some(&coord),
                    Some(&region),
                    Some(heap),
                    1,
                    Some(&range_flags),
                    Some(&heap_offset),
                    Some(&tile_count),
                    D3D12_TILE_MAPPING_FLAG_NONE,
                );
            }
        }

        Ok(())
    }

    /// Synchronizes virtual-to-physical tile residency in Vulkan via `vkQueueBindSparse`.
    ///
    /// Cross-platform mirror of DirectX 12 `UpdateTileMappings`. Binds physical 64KB pages
    /// from the device memory pool to virtual coordinates of a `VK_IMAGE_CREATE_SPARSE_RESIDENCY_BIT` image.
    ///
    /// # Safety
    /// - `device` and `queue` must be valid, initialized Vulkan handles supporting sparse binding operations.
    /// - `sparse_image` must be an active `vk::Image` created with `VK_IMAGE_CREATE_SPARSE_BINDING_BIT` and `VK_IMAGE_CREATE_SPARSE_RESIDENCY_BIT`.
    /// - The allocated device memory pool (`self.vk_memory_pool`) must remain allocated and valid for the duration of the binding.
    /// - `fence` must be a valid `vk::Fence` or `vk::Fence::null()`.
    pub unsafe fn update_vulkan_tile_mappings(
        &self,
        device: &ash::Device,
        queue: vk::Queue,
        sparse_image: vk::Image,
        dxgi_fmt: u32,
        plan: &TileAllocationPlan,
        fence: vk::Fence,
    ) -> GpckResult<()> {
        let memory_pool = self.vk_memory_pool.ok_or_else(|| {
            GpckError::VulkanError("Vulkan tile pool device memory has not been set".to_string())
        })?;

        let tile_shape = D3D12FormatTable::get_tile_shape_64k(dxgi_fmt, false);
        let mut image_binds = Vec::with_capacity(plan.newly_mapped.len() + plan.evicted.len());

        // 1. Unmap Evicted Tiles (vk::DeviceMemory::null())
        for (evicted_key, _) in &plan.evicted {
            let offset_texels = vk::Offset3D {
                x: (evicted_key.tile_x * tile_shape.width_in_texels) as i32,
                y: (evicted_key.tile_y * tile_shape.height_in_texels) as i32,
                z: (evicted_key.tile_z * tile_shape.depth_in_texels) as i32,
            };

            let extent_texels = vk::Extent3D {
                width: tile_shape.width_in_texels,
                height: tile_shape.height_in_texels,
                depth: tile_shape.depth_in_texels,
            };

            image_binds.push(vk::SparseImageMemoryBind {
                subresource: vk::ImageSubresource {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: evicted_key.subresource,
                    array_layer: 0,
                },
                offset: offset_texels,
                extent: extent_texels,
                memory: vk::DeviceMemory::null(), // NULL memory unbinds page
                memory_offset: 0,
                flags: vk::SparseMemoryBindFlags::empty(),
            });
        }

        // 2. Map Newly Allocated Physical 64KB Slots
        for (key, slot) in &plan.newly_mapped {
            let offset_texels = vk::Offset3D {
                x: (key.tile_x * tile_shape.width_in_texels) as i32,
                y: (key.tile_y * tile_shape.height_in_texels) as i32,
                z: (key.tile_z * tile_shape.depth_in_texels) as i32,
            };

            let extent_texels = vk::Extent3D {
                width: tile_shape.width_in_texels,
                height: tile_shape.height_in_texels,
                depth: tile_shape.depth_in_texels,
            };

            let physical_byte_offset = (*slot as u64) * 65536;

            image_binds.push(vk::SparseImageMemoryBind {
                subresource: vk::ImageSubresource {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: key.subresource,
                    array_layer: 0,
                },
                offset: offset_texels,
                extent: extent_texels,
                memory: memory_pool,
                memory_offset: physical_byte_offset,
                flags: vk::SparseMemoryBindFlags::empty(),
            });
        }

        if image_binds.is_empty() {
            return Ok(());
        }

        let image_bind_info = [vk::SparseImageMemoryBindInfo {
            image: sparse_image,
            bind_count: image_binds.len() as u32,
            p_binds: image_binds.as_ptr(),
            ..Default::default()
        }];

        let bind_info = [vk::BindSparseInfo {
            s_type: vk::StructureType::BIND_SPARSE_INFO,
            image_bind_count: 1,
            p_image_binds: image_bind_info.as_ptr(),
            ..Default::default()
        }];

        unsafe {
            device
                .queue_bind_sparse(queue, &bind_info, fence)
                .map_err(|e| {
                    GpckError::VulkanError(format!("vkQueueBindSparse failed: {:?}", e))
                })?;
        }

        Ok(())
    }

    #[inline(always)]
    pub fn stats(&self) -> (usize, usize, usize) {
        (
            self.resident_tiles.len(),
            self.free_slots.len(),
            self.total_tiles,
        )
    }

    pub fn clear(&mut self) {
        self.resident_tiles.clear();
        self.slot_to_tile.clear();
        self.lru_queue.clear();
        self.free_slots = (0..self.total_tiles as u32).rev().collect();
    }
}
