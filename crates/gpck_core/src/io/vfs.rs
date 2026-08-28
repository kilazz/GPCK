// crates/gpck_core/src/io/vfs.rs
//! # Virtual File System (VFS) with Priority Layered Mounting & Sharded CDN Cache

use crate::core::asset_id::AssetIdGenerator;
use crate::core::error::{GpckError, GpckResult};
use crate::format::archive::{FileEntry, GameArchive};
use std::collections::HashMap;
use std::hash::Hasher;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock as AsyncRwLock;
use uuid::Uuid;

#[cfg(feature = "remote-cdn")]
use reqwest::header::RANGE;

const NUM_CACHE_SHARDS: usize = 32;

/// Sanitizes virtual path against directory traversal (`../` or leading `/`).
#[inline]
pub fn sanitize_relative_path(virtual_path: &str) -> Option<PathBuf> {
    let mut clean = PathBuf::new();
    for comp in Path::new(virtual_path).components() {
        if let Component::Normal(c) = comp {
            clean.push(c);
        }
    }
    if clean.as_os_str().is_empty() {
        None
    } else {
        Some(clean)
    }
}

#[inline(always)]
fn make_chunk_cache_key(asset_id: &Uuid, offset: u64) -> u64 {
    let mut hasher = twox_hash::XxHash64::default();
    hasher.write(asset_id.as_bytes());
    hasher.write_u64(offset);
    hasher.finish()
}

/// Mounted archive layer with explicit integer priority for mod/DLC overrides.
#[derive(Clone)]
pub struct MountedArchiveLayer {
    pub id: Uuid,
    pub label: String,
    pub priority: i32,
    pub archive: Arc<GameArchive>,
}

/// Mounted directory layer with explicit integer priority.
#[derive(Clone)]
pub struct MountedDirectoryLayer {
    pub id: Uuid,
    pub label: String,
    pub priority: i32,
    pub path: PathBuf,
}

struct LruNode {
    hash: u64,
    data: Vec<u8>,
    prev: Option<usize>,
    next: Option<usize>,
}

pub struct LruChunkCache {
    capacity_bytes: usize,
    current_bytes: usize,
    entries: Vec<Option<LruNode>>,
    free_indices: Vec<usize>,
    map: HashMap<u64, usize>,
    head: Option<usize>,
    tail: Option<usize>,
}

impl LruChunkCache {
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            capacity_bytes,
            current_bytes: 0,
            entries: Vec::new(),
            free_indices: Vec::new(),
            map: HashMap::new(),
            head: None,
            tail: None,
        }
    }

    fn detach(&mut self, idx: usize) {
        let (prev, next) = {
            let node = self.entries[idx].as_ref().unwrap();
            (node.prev, node.next)
        };

        if let Some(p) = prev {
            self.entries[p].as_mut().unwrap().next = next;
        } else {
            self.head = next;
        }

        if let Some(n) = next {
            self.entries[n].as_mut().unwrap().prev = prev;
        } else {
            self.tail = prev;
        }

        let node = self.entries[idx].as_mut().unwrap();
        node.prev = None;
        node.next = None;
    }

    fn attach_head(&mut self, idx: usize) {
        let old_head = self.head;
        {
            let node = self.entries[idx].as_mut().unwrap();
            node.prev = None;
            node.next = old_head;
        }

        if let Some(h) = old_head {
            self.entries[h].as_mut().unwrap().prev = Some(idx);
        } else {
            self.tail = Some(idx);
        }

        self.head = Some(idx);
    }

    pub fn get(&mut self, hash: u64) -> Option<Vec<u8>> {
        if let Some(&idx) = self.map.get(&hash) {
            self.detach(idx);
            self.attach_head(idx);
            return Some(self.entries[idx].as_ref().unwrap().data.clone());
        }
        None
    }

    pub fn insert(&mut self, hash: u64, data: Vec<u8>) {
        if let Some(&idx) = self.map.get(&hash) {
            let old_len = self.entries[idx].as_ref().unwrap().data.len();
            self.current_bytes = (self.current_bytes - old_len) + data.len();
            self.entries[idx].as_mut().unwrap().data = data;
            self.detach(idx);
            self.attach_head(idx);
            self.evict_overflow();
            return;
        }

        let new_idx = if let Some(free_idx) = self.free_indices.pop() {
            self.entries[free_idx] = Some(LruNode {
                hash,
                data: data.clone(),
                prev: None,
                next: None,
            });
            free_idx
        } else {
            let idx = self.entries.len();
            self.entries.push(Some(LruNode {
                hash,
                data: data.clone(),
                prev: None,
                next: None,
            }));
            idx
        };

        self.current_bytes += data.len();
        self.map.insert(hash, new_idx);
        self.attach_head(new_idx);
        self.evict_overflow();
    }

    fn evict_overflow(&mut self) {
        while self.current_bytes > self.capacity_bytes && self.tail.is_some() {
            let tail_idx = self.tail.unwrap();
            let tail_hash = self.entries[tail_idx].as_ref().unwrap().hash;
            let tail_len = self.entries[tail_idx].as_ref().unwrap().data.len();

            self.detach(tail_idx);
            self.map.remove(&tail_hash);
            self.entries[tail_idx] = None;
            self.free_indices.push(tail_idx);
            self.current_bytes = self.current_bytes.saturating_sub(tail_len);
        }
    }
}

pub struct ShardedLruChunkCache {
    shards: Vec<AsyncRwLock<LruChunkCache>>,
}

impl ShardedLruChunkCache {
    pub fn new(total_capacity_bytes: usize) -> Self {
        let per_shard = (total_capacity_bytes / NUM_CACHE_SHARDS).max(64 * 1024);
        let mut shards = Vec::with_capacity(NUM_CACHE_SHARDS);
        for _ in 0..NUM_CACHE_SHARDS {
            shards.push(AsyncRwLock::new(LruChunkCache::new(per_shard)));
        }
        Self { shards }
    }

    #[inline(always)]
    fn get_shard_index(&self, hash: u64) -> usize {
        (hash as usize) % NUM_CACHE_SHARDS
    }

    pub async fn get(&self, hash: u64) -> Option<Vec<u8>> {
        let idx = self.get_shard_index(hash);
        let mut shard = self.shards[idx].write().await;
        shard.get(hash)
    }

    pub async fn insert(&self, hash: u64, data: Vec<u8>) {
        let idx = self.get_shard_index(hash);
        let mut shard = self.shards[idx].write().await;
        shard.insert(hash, data);
    }
}

pub struct VirtualFileSystem {
    archive_layers: Vec<MountedArchiveLayer>,
    directory_layers: Vec<MountedDirectoryLayer>,
    remote_urls: Vec<String>,
    #[cfg(feature = "remote-cdn")]
    async_http_client: reqwest::Client,
    chunk_lru_cache: Arc<ShardedLruChunkCache>,
}

impl VirtualFileSystem {
    pub const DEFAULT_CDN_CACHE_SIZE: usize = 256 * 1024 * 1024; // 256 MB default

    pub fn new() -> Self {
        Self::with_cache_capacity(Self::DEFAULT_CDN_CACHE_SIZE)
    }

    pub fn with_cache_capacity(cache_capacity_bytes: usize) -> Self {
        Self {
            archive_layers: Vec::new(),
            directory_layers: Vec::new(),
            remote_urls: Vec::new(),
            #[cfg(feature = "remote-cdn")]
            async_http_client: reqwest::Client::builder()
                .pool_max_idle_per_host(32)
                .build()
                .unwrap_or_default(),
            chunk_lru_cache: Arc::new(ShardedLruChunkCache::new(cache_capacity_bytes)),
        }
    }

    pub fn set_cdn_cache_capacity(&mut self, capacity_bytes: usize) {
        self.chunk_lru_cache = Arc::new(ShardedLruChunkCache::new(capacity_bytes));
    }

    // ========================================================================
    // Priority Layer Management
    // ========================================================================

    /// Mounts an archive with an explicit integer priority (e.g., Base Game: 0, DLC: 10, Mod: 100).
    /// Higher priority layers override lower priority layers.
    pub fn mount_archive_layered<P: AsRef<Path>>(
        &mut self,
        path: P,
        key: Option<[u8; 32]>,
        priority: i32,
        label: &str,
    ) -> GpckResult<Uuid> {
        let mut archive = GameArchive::open_with_key(path, key)?;
        let layer_id = Uuid::new_v4();

        let previous_archives: Vec<Arc<GameArchive>> = self
            .archive_layers
            .iter()
            .map(|l| l.archive.clone())
            .collect();

        archive.chunk_resolver = Some(Arc::new(move |hash| {
            for arch in previous_archives.iter().rev() {
                if let Some(data) = arch.resolve_base_chunk_local(hash) {
                    return Some(data);
                }
            }
            None
        }));

        self.archive_layers.push(MountedArchiveLayer {
            id: layer_id,
            label: label.to_string(),
            priority,
            archive: Arc::new(archive),
        });

        self.sort_layers();
        Ok(layer_id)
    }

    /// Mounts a loose directory with an explicit integer priority.
    pub fn mount_directory_layered<P: AsRef<Path>>(
        &mut self,
        path: P,
        priority: i32,
        label: &str,
    ) -> Uuid {
        let layer_id = Uuid::new_v4();
        self.directory_layers.push(MountedDirectoryLayer {
            id: layer_id,
            label: label.to_string(),
            priority,
            path: path.as_ref().to_path_buf(),
        });
        self.sort_layers();
        layer_id
    }

    /// Re-orders layers by priority descending (highest priority first).
    pub fn sort_layers(&mut self) {
        self.archive_layers
            .sort_by_key(|b| std::cmp::Reverse(b.priority));
        self.directory_layers
            .sort_by_key(|b| std::cmp::Reverse(b.priority));
    }

    /// Updates the priority of an archive layer at runtime.
    pub fn set_archive_layer_priority(&mut self, layer_id: Uuid, new_priority: i32) -> bool {
        if let Some(layer) = self.archive_layers.iter_mut().find(|l| l.id == layer_id) {
            layer.priority = new_priority;
            self.sort_layers();
            true
        } else {
            false
        }
    }

    /// Unmounts a layer by its UUID.
    pub fn unmount_layer(&mut self, layer_id: Uuid) -> bool {
        let initial_arch_len = self.archive_layers.len();
        self.archive_layers.retain(|l| l.id != layer_id);
        let initial_dir_len = self.directory_layers.len();
        self.directory_layers.retain(|l| l.id != layer_id);

        initial_arch_len != self.archive_layers.len()
            || initial_dir_len != self.directory_layers.len()
    }

    // Standard backward-compatible wrappers
    pub fn mount_archive<P: AsRef<Path>>(&mut self, path: P) -> GpckResult<()> {
        self.mount_archive_layered(path, None, 0, "BaseGame")
            .map(|_| ())
    }

    pub fn mount_archive_with_key<P: AsRef<Path>>(
        &mut self,
        path: P,
        key: Option<[u8; 32]>,
    ) -> GpckResult<()> {
        self.mount_archive_layered(path, key, 0, "BaseGame")
            .map(|_| ())
    }

    pub fn mount_directory<P: AsRef<Path>>(&mut self, path: P) {
        self.mount_directory_layered(path, 0, "LooseAssets");
    }

    pub fn mount_remote_url(&mut self, url: &str) {
        self.remote_urls.push(url.trim_end_matches('/').to_string());
    }

    pub fn get_mounted_archives(&self) -> Vec<Arc<GameArchive>> {
        self.archive_layers
            .iter()
            .map(|l| l.archive.clone())
            .collect()
    }

    // ========================================================================
    // Asset Lookup and Resolution
    // ========================================================================

    #[inline(always)]
    pub fn try_get_entry_by_id(&self, asset_id: Uuid) -> Option<FileEntry> {
        for layer in &self.archive_layers {
            if let Some(entry) = layer.archive.try_get_entry(asset_id) {
                return Some(entry);
            }
        }
        None
    }

    #[inline(always)]
    pub fn find_entry_and_archive(&self, asset_id: Uuid) -> Option<(FileEntry, Arc<GameArchive>)> {
        for layer in &self.archive_layers {
            if let Some(entry) = layer.archive.try_get_entry(asset_id) {
                return Some((entry, layer.archive.clone()));
            }
        }
        None
    }

    pub fn find_entry_relaxed(&self, path: &str) -> Option<(FileEntry, Arc<GameArchive>)> {
        let clean_path = path.trim_start_matches("res://");
        let file_name = Path::new(clean_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(clean_path);

        self.find_entry_and_archive(AssetIdGenerator::generate(clean_path))
            .or_else(|| self.find_entry_and_archive(AssetIdGenerator::generate(path)))
            .or_else(|| self.find_entry_and_archive(AssetIdGenerator::generate(file_name)))
    }

    pub fn read_file_relaxed(&self, path: &str) -> GpckResult<Vec<u8>> {
        let clean_path = path.trim_start_matches("res://");
        let file_name = Path::new(clean_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(clean_path);

        self.read_file(clean_path)
            .or_else(|_| self.read_file(path))
            .or_else(|_| self.read_file(file_name))
    }

    /// Reads a file querying the unified priority stack (Directories vs Archives by layer priority).
    pub fn read_file(&self, virtual_path: &str) -> GpckResult<Vec<u8>> {
        let clean_rel = sanitize_relative_path(virtual_path);
        let asset_id = AssetIdGenerator::generate(virtual_path);

        let mut arch_idx = 0;
        let mut dir_idx = 0;

        while arch_idx < self.archive_layers.len() || dir_idx < self.directory_layers.len() {
            let arch_pri = self
                .archive_layers
                .get(arch_idx)
                .map(|l| l.priority)
                .unwrap_or(i32::MIN);
            let dir_pri = self
                .directory_layers
                .get(dir_idx)
                .map(|l| l.priority)
                .unwrap_or(i32::MIN);

            if dir_pri >= arch_pri && dir_idx < self.directory_layers.len() {
                if let Some(ref rel) = clean_rel {
                    let full_path = self.directory_layers[dir_idx].path.join(rel);
                    if full_path.exists()
                        && full_path.starts_with(&self.directory_layers[dir_idx].path)
                    {
                        return std::fs::read(full_path).map_err(GpckError::Io);
                    }
                }
                dir_idx += 1;
            } else if arch_idx < self.archive_layers.len() {
                if let Some(entry) = self.archive_layers[arch_idx]
                    .archive
                    .try_get_entry(asset_id)
                {
                    return self.archive_layers[arch_idx].archive.read_asset(&entry);
                }
                arch_idx += 1;
            }
        }

        Err(GpckError::AssetNotFound(virtual_path.to_string()))
    }

    pub fn read_file_by_id(&self, asset_id: Uuid) -> GpckResult<Vec<u8>> {
        for layer in &self.archive_layers {
            if let Some(entry) = layer.archive.try_get_entry(asset_id) {
                return layer.archive.read_asset(&entry);
            }
        }
        Err(GpckError::AssetIdNotFound(asset_id))
    }

    pub async fn read_file_async(&self, virtual_path: &str) -> GpckResult<Vec<u8>> {
        if let Some(clean_rel) = sanitize_relative_path(virtual_path) {
            for dir_layer in &self.directory_layers {
                let full_path = dir_layer.path.join(&clean_rel);
                if full_path.exists() && full_path.starts_with(&dir_layer.path) {
                    return tokio::fs::read(full_path).await.map_err(GpckError::Io);
                }
            }
        }

        let asset_id = AssetIdGenerator::generate(virtual_path);

        for layer in &self.archive_layers {
            if let Some(entry) = layer.archive.try_get_entry(asset_id) {
                let arch_clone = layer.archive.clone();
                let res = tokio::task::spawn_blocking(move || arch_clone.read_asset(&entry))
                    .await
                    .map_err(std::io::Error::other)??;
                return Ok(res);
            }
        }

        if !self.remote_urls.is_empty() {
            return self
                .fetch_remote_chunk_range_async(asset_id, 0, 128 * 1024)
                .await;
        }

        Err(GpckError::AssetNotFound(virtual_path.to_string()))
    }

    pub async fn fetch_remote_chunk_range_async(
        &self,
        asset_id: Uuid,
        offset: u64,
        size: usize,
    ) -> GpckResult<Vec<u8>> {
        #[cfg(feature = "remote-cdn")]
        {
            let chunk_cache_key = make_chunk_cache_key(&asset_id, offset);

            if let Some(cached_data) = self.chunk_lru_cache.get(chunk_cache_key).await {
                return Ok(cached_data);
            }

            let range_header = format!("bytes={}-{}", offset, offset + size as u64 - 1);
            let mut last_err = String::new();

            for base_url in &self.remote_urls {
                let url = format!("{}/chunks/{}.iochunk", base_url, asset_id);
                let response = self
                    .async_http_client
                    .get(&url)
                    .header(RANGE, &range_header)
                    .send()
                    .await;

                match response {
                    Ok(resp) if resp.status().is_success() || resp.status() == 206 => {
                        if let Ok(bytes) = resp.bytes().await {
                            let data = bytes.to_vec();
                            self.chunk_lru_cache
                                .insert(chunk_cache_key, data.clone())
                                .await;
                            return Ok(data);
                        }
                    }
                    Ok(resp) => {
                        last_err = format!("HTTP Status {}", resp.status());
                    }
                    Err(e) => {
                        last_err = e.to_string();
                    }
                }
            }

            Err(GpckError::CdnNetworkError {
                url: format!("Asset ID {:?}", asset_id),
                message: if last_err.is_empty() {
                    "All remote CDN endpoints failed".to_string()
                } else {
                    last_err
                },
            })
        }
        #[cfg(not(feature = "remote-cdn"))]
        {
            let _ = (asset_id, offset, size);
            Err(GpckError::CdnNetworkError {
                url: format!("Asset ID {:?}", asset_id),
                message:
                    "Remote CDN chunk streaming is disabled ('remote-cdn' feature not compiled)"
                        .to_string(),
            })
        }
    }
}

impl Default for VirtualFileSystem {
    fn default() -> Self {
        Self::new()
    }
}
