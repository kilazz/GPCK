// crates/gpck_core/src/io/resource_manager.rs
//! # Asynchronous Resource Manager, Frame Accumulator & Direct-to-VRAM Batching Engine

use crate::compression::codecs::CompressionMethod;
use crate::core::asset_id::AssetIdGenerator;
use crate::core::error::{GpckError, GpckResult};
use crate::gacl::GaclTransform;
#[cfg(windows)]
use crate::gpu::directstorage::GpuDirectStorage;
use crate::gpu::directstorage::QueuePriority;
#[cfg(windows)]
use crate::gpu::directstorage_sys::*;
use crate::graphics::dxgi_format::D3D12FormatTable;
use crate::io::vfs::VirtualFileSystem;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::{OnceCell, RwLock, Semaphore};
use uuid::Uuid;
#[cfg(windows)]
use windows::core::BOOL;

pub type CachedAsset = Arc<Vec<u8>>;
pub type AssetCell = Arc<OnceCell<CachedAsset>>;

const MAX_CONCURRENT_PREFETCH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DependencyType {
    Hard,
    Soft,
}

#[derive(Debug, Clone)]
pub struct DependencyEdge {
    pub target_id: Uuid,
    pub dep_type: DependencyType,
}

#[derive(Debug, Clone)]
pub struct VramBufferStreamRequest {
    pub asset_id: Uuid,
    pub dest_resource_ptr: *mut std::ffi::c_void,
    pub dest_offset: u64,
    pub cancellation_tag: u64,
    pub priority: QueuePriority,
}

#[derive(Debug, Clone)]
pub struct VramTextureStreamRequest {
    pub asset_id: Uuid,
    pub dest_texture_ptr: *mut std::ffi::c_void,
    pub first_subresource: u32,
    pub cancellation_tag: u64,
    pub priority: QueuePriority,
}

/// Request descriptor for streaming a single 64KB sparse tile into a D3D12 Tiled Resource.
#[derive(Debug, Clone)]
pub struct VramTileStreamRequest {
    pub asset_id: Uuid,
    pub dest_resource_ptr: *mut std::ffi::c_void, // ID3D12Resource* (Tiled Texture)
    pub subresource: u32,                         // Mip Level
    pub tile_x: u32,
    pub tile_y: u32,
    pub tile_z: u32,
    pub cancellation_tag: u64,
    pub priority: QueuePriority,
}

pub struct ResourceManager {
    vfs: Arc<StdRwLock<VirtualFileSystem>>,
    asset_cells: RwLock<HashMap<Uuid, AssetCell>>,
    dependency_graph: RwLock<HashMap<Uuid, Vec<DependencyEdge>>>,
    prefetch_semaphore: Arc<Semaphore>,
}

impl ResourceManager {
    pub fn new(vfs: Arc<StdRwLock<VirtualFileSystem>>) -> Self {
        Self {
            vfs,
            asset_cells: RwLock::new(HashMap::new()),
            dependency_graph: RwLock::new(HashMap::new()),
            prefetch_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_PREFETCH)),
        }
    }

    pub async fn register_dependency_edge(
        &self,
        virtual_path: &str,
        dep_path: &str,
        dep_type: DependencyType,
    ) {
        let asset_id = AssetIdGenerator::generate(virtual_path);
        let dep_id = AssetIdGenerator::generate(dep_path);

        let mut graph = self.dependency_graph.write().await;
        graph.entry(asset_id).or_default().push(DependencyEdge {
            target_id: dep_id,
            dep_type,
        });
    }

    pub async fn prefetch_transitive_dependencies(
        &self,
        root_virtual_path: &str,
        include_soft_refs: bool,
        tag_filter_mask: u32,
    ) {
        let root_id = AssetIdGenerator::generate(root_virtual_path);
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        queue.push_back(root_id);
        visited.insert(root_id);

        let graph = self.dependency_graph.read().await;

        while let Some(current_id) = queue.pop_front() {
            if let Some(edges) = graph.get(&current_id) {
                for edge in edges {
                    if !include_soft_refs && edge.dep_type == DependencyType::Soft {
                        continue;
                    }

                    if visited.insert(edge.target_id) {
                        queue.push_back(edge.target_id);

                        let vfs_clone = self.vfs.clone();
                        let dep_id = edge.target_id;
                        let cell = self.get_cell(dep_id).await;
                        let sem = self.prefetch_semaphore.clone();

                        if !cell.initialized() {
                            tokio::spawn(async move {
                                let Ok(_permit) = sem.acquire().await else {
                                    crate::core::logger::log_warn(
                                        "[Prefetch] Background prefetch permit acquisition cancelled",
                                    );
                                    return;
                                };
                                let init_res: Result<&CachedAsset, GpckError> = cell
                                    .get_or_try_init(|| async {
                                        tokio::task::spawn_blocking(
                                            move || -> GpckResult<CachedAsset> {
                                                let vfs_guard = vfs_clone.read().unwrap();

                                                if let Some(entry) =
                                                    vfs_guard.try_get_entry_by_id(dep_id)
                                                    && entry.matches_tag_filter(tag_filter_mask)
                                                {
                                                    let data = vfs_guard.read_file_by_id(dep_id)?;
                                                    return Ok(Arc::new(data));
                                                }
                                                Ok(Arc::new(Vec::new()))
                                            },
                                        )
                                        .await
                                        .map_err(|e| GpckError::AssetNotFound(e.to_string()))?
                                    })
                                    .await;

                                if let Err(e) = init_res {
                                    crate::core::logger::log_error(&format!(
                                        "[Prefetch Error] Background prefetch failed for asset ID {}: {}",
                                        dep_id, e
                                    ));
                                }
                            });
                        }
                    }
                }
            }
        }
    }

    async fn get_cell(&self, asset_id: Uuid) -> AssetCell {
        {
            let read_guard = self.asset_cells.read().await;
            if let Some(cell) = read_guard.get(&asset_id) {
                return cell.clone();
            }
        }

        let mut write_guard = self.asset_cells.write().await;
        write_guard
            .entry(asset_id)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone()
    }

    pub async fn load_asset_async(&self, virtual_path: &str) -> GpckResult<CachedAsset> {
        let asset_id = AssetIdGenerator::generate(virtual_path);

        if virtual_path.to_lowercase().ends_with(".dds") {
            self.register_dependency_edge(
                virtual_path,
                &format!("{}.highmips", virtual_path),
                DependencyType::Soft,
            )
            .await;
        }

        self.prefetch_transitive_dependencies(virtual_path, false, 0)
            .await;

        let cell = self.get_cell(asset_id).await;
        let vfs_clone = self.vfs.clone();
        let virtual_path_owned = virtual_path.to_string();

        let asset_ref = cell
            .get_or_try_init(|| async {
                tokio::task::spawn_blocking(move || -> GpckResult<CachedAsset> {
                    let vfs_guard = vfs_clone.read().unwrap();
                    let data = vfs_guard.read_file(&virtual_path_owned)?;
                    Ok(Arc::new(data))
                })
                .await
                .map_err(|e| GpckError::AssetNotFound(e.to_string()))?
            })
            .await?;

        Ok(asset_ref.clone())
    }

    /// Cancels in-flight DirectStorage streaming requests matching a camera frustum or sector tag.
    #[cfg(windows)]
    pub fn cancel_requests_by_tag(&self, ds: &GpuDirectStorage, tag: u64) {
        ds.cancel_requests_with_tag(QueuePriority::Normal, !0, tag);
        ds.cancel_requests_with_tag(QueuePriority::Low, !0, tag);
    }

    #[cfg(windows)]
    pub fn stream_buffer_batch_to_vram(
        &self,
        ds: &GpuDirectStorage,
        requests: &[VramBufferStreamRequest],
    ) -> GpckResult<HashMap<QueuePriority, u64>> {
        if !ds.is_supported() {
            return Err(GpckError::DirectStorageUnsupported);
        }

        let vfs_guard = self.vfs.read().unwrap();
        let mut queued_priorities = HashSet::new();

        for req in requests {
            let (entry, archive) = vfs_guard
                .find_entry_and_archive(req.asset_id)
                .ok_or(GpckError::AssetIdNotFound(req.asset_id))?;

            let method = CompressionMethod::from_flags(entry.flags);
            let ds_format = match method {
                CompressionMethod::GDeflate => DSTORAGE_COMPRESSION_FORMAT_GDEFLATE,
                CompressionMethod::Zstd => DSTORAGE_COMPRESSION_FORMAT_ZSTD,
                CompressionMethod::BrotliG => DSTORAGE_CUSTOM_COMPRESSION_0,
                _ => DSTORAGE_COMPRESSION_FORMAT_NONE,
            };
            let gacl_transform = entry.gacl_transform() as u8;

            let gdat_path = std::path::Path::new(archive.file_path()).with_extension("gdat");
            let dstorage_file = ds.open_file(&gdat_path)?;

            let chunks = archive.get_chunk_table(&entry)?;
            let mut current_dest_offset = req.dest_offset;

            for chunk in chunks {
                if chunk.offset >= 0 {
                    let mut ds_req: DSTORAGE_REQUEST = unsafe { std::mem::zeroed() };
                    ds_req.set_file_to_buffer(
                        dstorage_file.ptr(),
                        chunk.offset as u64,
                        chunk.compressed_size,
                        req.dest_resource_ptr,
                        current_dest_offset,
                        chunk.original_size,
                        ds_format,
                        gacl_transform,
                    );
                    ds_req.CancellationTag = req.cancellation_tag;

                    ds.enqueue_buffer_request(req.priority, &ds_req);
                    queued_priorities.insert(req.priority);
                    current_dest_offset += chunk.original_size as u64;
                }
            }
        }

        let mut fences = HashMap::new();
        for priority in queued_priorities {
            let fence_val = ds.flush_and_signal(priority)?;
            fences.insert(priority, fence_val);
        }

        Ok(fences)
    }

    #[cfg(windows)]
    pub fn stream_texture_batch_to_vram(
        &self,
        ds: &GpuDirectStorage,
        requests: &[VramTextureStreamRequest],
    ) -> GpckResult<HashMap<QueuePriority, u64>> {
        if !ds.is_supported() {
            return Err(GpckError::DirectStorageUnsupported);
        }

        let vfs_guard = self.vfs.read().unwrap();
        let mut queued_priorities = HashSet::new();

        for req in requests {
            let (entry, archive) = vfs_guard
                .find_entry_and_archive(req.asset_id)
                .ok_or(GpckError::AssetIdNotFound(req.asset_id))?;

            let method = CompressionMethod::from_flags(entry.flags);
            let ds_format = match method {
                CompressionMethod::GDeflate => DSTORAGE_COMPRESSION_FORMAT_GDEFLATE,
                CompressionMethod::Zstd => DSTORAGE_COMPRESSION_FORMAT_ZSTD,
                CompressionMethod::BrotliG => DSTORAGE_CUSTOM_COMPRESSION_0,
                _ => DSTORAGE_COMPRESSION_FORMAT_NONE,
            };

            let gdat_path = std::path::Path::new(archive.file_path()).with_extension("gdat");
            let dstorage_file = ds.open_file(&gdat_path)?;
            let chunks = archive.get_chunk_table(&entry)?;

            if let Some(first_chunk) = chunks.first() {
                let mut ds_req: DSTORAGE_REQUEST = unsafe { std::mem::zeroed() };
                ds_req.set_file_to_texture(
                    dstorage_file.ptr(),
                    first_chunk.offset as u64,
                    first_chunk.compressed_size,
                    req.dest_texture_ptr,
                    req.first_subresource,
                    entry.original_size,
                    ds_format,
                );
                ds_req.CancellationTag = req.cancellation_tag;

                ds.enqueue_buffer_request(req.priority, &ds_req);
                queued_priorities.insert(req.priority);
            }
        }

        let mut fences = HashMap::new();
        for priority in queued_priorities {
            let fence_val = ds.flush_and_signal(priority)?;
            fences.insert(priority, fence_val);
        }

        Ok(fences)
    }

    /// Streams a batch of requested 64KB sparse tiles directly from NVMe storage into VRAM tiled resources.
    #[cfg(windows)]
    pub fn stream_tiles_batch_to_vram(
        &self,
        ds: &GpuDirectStorage,
        requests: &[VramTileStreamRequest],
    ) -> GpckResult<HashMap<QueuePriority, u64>> {
        if !ds.is_supported() {
            return Err(GpckError::DirectStorageUnsupported);
        }

        if requests.is_empty() {
            return Ok(HashMap::new());
        }

        let vfs_guard = self.vfs.read().unwrap();
        let mut queued_priorities = HashSet::new();

        struct ResolvedTileRequest<'a> {
            req: &'a VramTileStreamRequest,
            chunk_offset: u64,
            compressed_size: u32,
            original_size: u32,
            ds_format: u8,
            gacl_transform: u8,
            dstorage_file_ptr: *mut std::ffi::c_void,
        }

        let mut resolved_list = Vec::with_capacity(requests.len());

        for req in requests {
            let (entry, archive) = vfs_guard
                .find_entry_and_archive(req.asset_id)
                .ok_or(GpckError::AssetIdNotFound(req.asset_id))?;

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
            let dxgi_fmt = GaclTransform::from_u32(entry.gacl_transform()).to_dxgi_format();

            let (tilings, packed_info, _) = D3D12FormatTable::calculate_subresource_tilings(
                dxgi_fmt,
                width.max(1),
                height.max(1),
                1,
                mip_count.max(1),
                1,
            );

            let tile_index = if req.subresource < packed_info.num_standard_mips as u32 {
                let tiling = &tilings[req.subresource as usize];
                tiling.start_tile_index_in_overall_resource as usize
                    + (req.tile_y * tiling.width_in_tiles + req.tile_x) as usize
            } else {
                packed_info.start_tile_index_in_overall_resource as usize
            };

            let chunks = archive.get_chunk_table(&entry)?;
            let chunk = chunks.get(tile_index).ok_or_else(|| {
                GpckError::InvalidFormat(format!(
                    "Requested tile index {} out of bounds for asset {:?}",
                    tile_index, req.asset_id
                ))
            })?;

            if chunk.offset < 0 {
                continue;
            }

            let gdat_path = std::path::Path::new(archive.file_path()).with_extension("gdat");
            let dstorage_file = ds.open_file(&gdat_path)?;

            resolved_list.push(ResolvedTileRequest {
                req,
                chunk_offset: chunk.offset as u64,
                compressed_size: chunk.compressed_size,
                original_size: chunk.original_size,
                ds_format,
                gacl_transform,
                dstorage_file_ptr: dstorage_file.ptr(),
            });
        }

        resolved_list.sort_by(|a, b| {
            a.req
                .dest_resource_ptr
                .cmp(&b.req.dest_resource_ptr)
                .then(a.req.priority.cmp(&b.req.priority))
                .then(a.req.subresource.cmp(&b.req.subresource))
                .then(a.req.tile_y.cmp(&b.req.tile_y))
                .then(a.req.tile_z.cmp(&b.req.tile_z))
                .then(a.req.tile_x.cmp(&b.req.tile_x))
        });

        let mut idx = 0;
        while idx < resolved_list.len() {
            let start_item = &resolved_list[idx];
            let mut span_count = 1u32;
            let mut total_compressed = start_item.compressed_size;
            let mut total_uncompressed = start_item.original_size;

            while idx + (span_count as usize) < resolved_list.len() {
                let next_item = &resolved_list[idx + (span_count as usize)];

                let is_contiguous_coords = next_item.req.dest_resource_ptr
                    == start_item.req.dest_resource_ptr
                    && next_item.req.subresource == start_item.req.subresource
                    && next_item.req.tile_y == start_item.req.tile_y
                    && next_item.req.tile_z == start_item.req.tile_z
                    && next_item.req.tile_x == start_item.req.tile_x + span_count;

                let is_contiguous_lba = next_item.dstorage_file_ptr == start_item.dstorage_file_ptr
                    && next_item.chunk_offset
                        == start_item.chunk_offset + (total_compressed as u64)
                    && next_item.ds_format == start_item.ds_format
                    && next_item.gacl_transform == start_item.gacl_transform;

                if is_contiguous_coords && is_contiguous_lba {
                    total_compressed += next_item.compressed_size;
                    total_uncompressed += next_item.original_size;
                    span_count += 1;
                } else {
                    break;
                }
            }

            let coord = D3D12_TILED_RESOURCE_COORDINATE {
                X: start_item.req.tile_x,
                Y: start_item.req.tile_y,
                Z: start_item.req.tile_z,
                Subresource: start_item.req.subresource,
            };

            let tile_region = D3D12_TILE_REGION_SIZE {
                NumTiles: span_count,
                UseBox: BOOL(0),
                Width: span_count,
                Height: 1,
                Depth: 1,
            };

            let mut ds_req: DSTORAGE_REQUEST = unsafe { std::mem::zeroed() };
            ds_req.set_file_to_tiles(
                start_item.dstorage_file_ptr,
                start_item.chunk_offset,
                total_compressed,
                start_item.req.dest_resource_ptr,
                coord,
                tile_region,
                total_uncompressed,
                start_item.ds_format,
                start_item.gacl_transform,
            );
            ds_req.CancellationTag = start_item.req.cancellation_tag;

            ds.enqueue_tile_request(start_item.req.priority, &ds_req);
            queued_priorities.insert(start_item.req.priority);

            idx += span_count as usize;
        }

        let mut fences = HashMap::new();
        for priority in queued_priorities {
            let fence_val = ds.flush_and_signal(priority)?;
            fences.insert(priority, fence_val);
        }

        Ok(fences)
    }

    pub async fn unload_all(&self) {
        let mut cells = self.asset_cells.write().await;
        let mut graph = self.dependency_graph.write().await;
        cells.clear();
        graph.clear();
    }
}

/// Frame-wide DirectStorage request accumulator preventing CPU-GPU synchronization bubbles.
pub struct DirectStorageFrameAccumulator {
    buffer_requests: StdRwLock<Vec<VramBufferStreamRequest>>,
    texture_requests: StdRwLock<Vec<VramTextureStreamRequest>>,
    tile_requests: StdRwLock<Vec<VramTileStreamRequest>>,
}

impl DirectStorageFrameAccumulator {
    pub fn new() -> Self {
        Self {
            buffer_requests: StdRwLock::new(Vec::with_capacity(64)),
            texture_requests: StdRwLock::new(Vec::with_capacity(32)),
            tile_requests: StdRwLock::new(Vec::with_capacity(256)),
        }
    }

    /// Enqueues a linear buffer streaming request into the current frame.
    pub fn push_buffer_request(&self, req: VramBufferStreamRequest) {
        let mut guard = self.buffer_requests.write().unwrap();
        guard.push(req);
    }

    /// Enqueues a 2D texture streaming request into the current frame.
    pub fn push_texture_request(&self, req: VramTextureStreamRequest) {
        let mut guard = self.texture_requests.write().unwrap();
        guard.push(req);
    }

    /// Enqueues a 64KB sparse tile request into the current frame.
    pub fn push_tile_request(&self, req: VramTileStreamRequest) {
        let mut guard = self.tile_requests.write().unwrap();
        guard.push(req);
    }

    /// Submits all accumulated requests across the frame to DirectStorage hardware queues
    /// in a single batch, signaling fences once before Present().
    #[cfg(windows)]
    pub fn flush_frame(
        &self,
        manager: &ResourceManager,
        ds: &GpuDirectStorage,
    ) -> GpckResult<HashMap<QueuePriority, u64>> {
        let mut results = HashMap::new();

        let mut buf_guard = self.buffer_requests.write().unwrap();
        if !buf_guard.is_empty() {
            let fences = manager.stream_buffer_batch_to_vram(ds, &buf_guard)?;
            results.extend(fences);
            buf_guard.clear();
        }

        let mut tex_guard = self.texture_requests.write().unwrap();
        if !tex_guard.is_empty() {
            let fences = manager.stream_texture_batch_to_vram(ds, &tex_guard)?;
            results.extend(fences);
            tex_guard.clear();
        }

        let mut tile_guard = self.tile_requests.write().unwrap();
        if !tile_guard.is_empty() {
            let fences = manager.stream_tiles_batch_to_vram(ds, &tile_guard)?;
            results.extend(fences);
            tile_guard.clear();
        }

        Ok(results)
    }

    pub fn clear(&self) {
        self.buffer_requests.write().unwrap().clear();
        self.texture_requests.write().unwrap().clear();
        self.tile_requests.write().unwrap().clear();
    }
}

impl Default for DirectStorageFrameAccumulator {
    fn default() -> Self {
        Self::new()
    }
}
