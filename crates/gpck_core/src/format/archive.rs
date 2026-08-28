// crates/gpck_core/src/format/archive.rs
//! # GPCK Binary Archive Reader & Memory Map
//!
//! Uses Memory Mapping for `.gtoc` metadata and optional zero-copy `.gdat` access,
//! while providing lock-free positional I/O and Linux `io_uring` O_DIRECT kernel bypass
//! during parallel CPU and GPU streaming.

use crate::compression::codecs::{Codec, CompressionMethod};
use crate::core::error::{GpckError, GpckResult};
use crate::format::chd::ChdLookup;
pub use crate::format::chd::hash_asset_id_with_seed;
use crate::io::direct_io::LinuxDirectIoReader;
use crate::io::stream::ArchiveStream;
use bytemuck::{Pod, Zeroable};
use memmap2::Mmap;
use std::collections::HashSet;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ArchiveFlags: u32 {
        const IS_COMPRESSED   = 1 << 0;
        const ENCRYPTED_META  = 1 << 1;
        const DELETED         = 1 << 2;
        const STREAMING       = 1 << 8;
        const BOOT_TAIL       = 1 << 11; // 64KB Packed Mip Tail placed in Partition 0 (Boot)
    }
}

pub const MAGIC_INT: u32 = 0x4B435047; // "GPCK"
pub const FLAG_IS_COMPRESSED: u32 = ArchiveFlags::IS_COMPRESSED.bits();
pub const FLAG_ENCRYPTED_META: u32 = ArchiveFlags::ENCRYPTED_META.bits();
pub const FLAG_DELETED: u32 = ArchiveFlags::DELETED.bits();
pub const FLAG_STREAMING: u32 = ArchiveFlags::STREAMING.bits();
pub const FLAG_BOOT_TAIL: u32 = ArchiveFlags::BOOT_TAIL.bits();

pub const MASK_METHOD: u32 = 0x38;
pub const TYPE_TEXTURE: u32 = 1 << 6;
pub const TYPE_MESHLET_CONTAINER: u32 = 1 << 7; // .gmesh
pub const TYPE_DMM_CONTAINER: u32 = 1 << 8; // .gdmm (Displaced Micro-Meshes)
pub const TYPE_DGF_CONTAINER: u32 = 1 << 9; // .dgf  (AMD Dense Geometry Format 128B)
pub const TYPE_TILED_RESOURCE: u32 = 1 << 10; // 64KB Sparse Hardware Tiled Resource
pub const MASK_ALIGNMENT: u32 = 0xFF000000;
pub const SHIFT_ALIGNMENT: u32 = 24;

pub const MASK_GACL_TRANSFORM: u32 = 0x003F0000;
pub const SHIFT_GACL_TRANSFORM: u32 = 16;

pub const TAG_BASE_GAME: u32 = 1 << 0;
pub const TAG_HD_TEXTURES: u32 = 1 << 1;
pub const TAG_AUDIO_EN: u32 = 1 << 2;
pub const TAG_AUDIO_RU: u32 = 1 << 3;
pub const TAG_DLC_EXPANSION: u32 = 1 << 4;

pub type ChunkResolver = Arc<dyn Fn(u64) -> Option<Vec<u8>> + Send + Sync>;

/// Performs a thread-safe positional read without mutating the file's shared seek pointer.
pub fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_exact_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            let bytes_read = file.seek_read(buf, offset)?;
            if bytes_read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Reached EOF before filling buffer during positional read",
                ));
            }
            let tmp = buf;
            buf = &mut tmp[bytes_read..];
            offset += bytes_read as u64;
        }
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        compile_error!("Positional file reading is not implemented for this target OS");
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct ArchiveHeader {
    pub magic: u32,
    pub version: i32,
    pub master_toc_offset: i64,
    pub bundle_count: i32,
    pub _pad0: i32,
    pub total_uncompressed_size: i64,
    pub padding_longs: [i64; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct BundleEntry {
    pub bundle_id: u64,
    pub toc_offset: i64,
    pub toc_size: i64,
    pub name_table_offset: i64,
    pub name_table_size: i64,
    pub chunk_table_offset: i64,
    pub chunk_table_size: i64,
    pub file_count: i32,
    pub hash_table_capacity: i32,
    pub seed_table_offset: i64,
    pub seed_count: i32,
    pub _pad0: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub asset_id: [u8; 16],
    pub data_offset: i64,
    pub chunk_table_offset: i64,
    pub name_offset: i64,
    pub compressed_size: u32,
    pub original_size: u32,
    pub flags: u32,
    pub meta1: u32,
    pub meta2: u32,
    pub tags: u32,
    pub partition_id: u32,
    pub chunk_count: i32,
    pub sub_chunk_offset: u32,
    pub sub_chunk_size: u32,
}

impl FileEntry {
    #[inline(always)]
    pub fn gacl_transform(&self) -> u32 {
        (self.flags & MASK_GACL_TRANSFORM) >> SHIFT_GACL_TRANSFORM
    }

    #[inline(always)]
    pub fn matches_tag_filter(&self, tag_mask: u32) -> bool {
        if tag_mask == 0 {
            return true;
        }
        (self.tags & tag_mask) != 0
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq, Eq)]
pub struct ChunkInfo {
    pub offset: i64,
    pub compressed_size: u32,
    pub original_size: u32,
    pub hash: u64,
}

pub struct GameArchive {
    file_path: String,
    _toc_file: File,
    data_file: File,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    direct_io_reader: Option<LinuxDirectIoReader>,
    toc_mmap: Mmap,
    data_mmap: Option<Mmap>,
    header: ArchiveHeader,
    bundles: Vec<BundleEntry>,
    pub decryption_key: Option<[u8; 32]>,
    pub chunk_resolver: Option<ChunkResolver>,
}

impl GameArchive {
    pub fn open<P: AsRef<Path>>(path: P) -> GpckResult<Self> {
        Self::open_with_key(path, None)
    }

    pub fn open_with_key<P: AsRef<Path>>(path: P, key: Option<[u8; 32]>) -> GpckResult<Self> {
        let path_ref = path.as_ref();
        let mut toc_path = path_ref.to_path_buf();
        let mut data_path = path_ref.to_path_buf();

        if path_ref.extension().and_then(|s| s.to_str()) == Some("gdat") {
            toc_path.set_extension("gtoc");
        } else {
            data_path.set_extension("gdat");
            toc_path.set_extension("gtoc");
        }

        let toc_file = File::open(&toc_path).map_err(GpckError::Io)?;
        let data_file = File::open(&data_path).map_err(GpckError::Io)?;

        let direct_io_reader = LinuxDirectIoReader::open(&data_path).ok();

        let toc_mmap = unsafe { Mmap::map(&toc_file).map_err(GpckError::Io)? };
        let data_mmap = unsafe { Mmap::map(&data_file).ok() };

        let header_size = std::mem::size_of::<ArchiveHeader>();
        if toc_mmap.len() < header_size {
            return Err(GpckError::InvalidFormat(
                "TOC file is too short".to_string(),
            ));
        }

        let header: ArchiveHeader = bytemuck::pod_read_unaligned(&toc_mmap[0..header_size]);
        if header.magic != MAGIC_INT {
            return Err(GpckError::InvalidMagic(header.magic));
        }

        let mut bundles = Vec::with_capacity(header.bundle_count as usize);
        let bundle_size = std::mem::size_of::<BundleEntry>();
        let start = header.master_toc_offset as usize;

        for i in 0..header.bundle_count as usize {
            let offset = start + i * bundle_size;
            let bundle_slice = toc_mmap.get(offset..offset + bundle_size).ok_or_else(|| {
                GpckError::InvalidFormat("Corrupt bundle offset in TOC".to_string())
            })?;
            let bundle: BundleEntry = bytemuck::pod_read_unaligned(bundle_slice);
            bundles.push(bundle);
        }

        Ok(Self {
            file_path: toc_path.to_string_lossy().to_string(),
            _toc_file: toc_file,
            data_file,
            direct_io_reader,
            toc_mmap,
            data_mmap,
            header,
            bundles,
            decryption_key: key,
            chunk_resolver: None,
        })
    }

    pub fn file_path(&self) -> &str {
        &self.file_path
    }

    pub fn total_uncompressed_size(&self) -> i64 {
        self.header.total_uncompressed_size
    }

    pub fn try_get_direct_data_slice(&self, entry: &FileEntry) -> Option<&[u8]> {
        if (entry.flags & FLAG_IS_COMPRESSED) != 0 {
            return None;
        }

        if entry.gacl_transform() != 0 {
            return None;
        }

        let method = CompressionMethod::from_flags(entry.flags);
        if method != CompressionMethod::Store && (entry.flags & FLAG_IS_COMPRESSED) != 0 {
            return None;
        }

        if entry.data_offset < 0 {
            return None;
        }

        if entry.chunk_count > 1 && entry.sub_chunk_size == 0 {
            return None;
        }

        let mmap = self.data_mmap.as_ref()?;

        let (start, end) = if entry.sub_chunk_size > 0 {
            let s = entry.data_offset as usize + entry.sub_chunk_offset as usize;
            let e = s + entry.sub_chunk_size as usize;
            (s, e)
        } else {
            let s = entry.data_offset as usize;
            let e = s + entry.original_size as usize;
            (s, e)
        };

        if end <= mmap.len() {
            Some(&mmap[start..end])
        } else {
            None
        }
    }

    pub fn try_get_entry(&self, id: Uuid) -> Option<FileEntry> {
        for bundle in self.bundles.iter().rev() {
            if let Some(entry) = ChdLookup::query_entry_from_mmap(
                &self.toc_mmap,
                id,
                bundle.toc_offset as usize,
                bundle.hash_table_capacity as usize,
                bundle.seed_table_offset as usize,
                bundle.seed_count as usize,
            ) {
                return Some(entry);
            }

            let entry_size = std::mem::size_of::<FileEntry>();
            let capacity = bundle.hash_table_capacity as usize;
            let id_bytes = id.as_bytes();
            for i in 0..capacity {
                let offset = bundle.toc_offset as usize + i * entry_size;
                if let Some(slice) = self.toc_mmap.get(offset..offset + entry_size) {
                    let entry: FileEntry = bytemuck::pod_read_unaligned(slice);
                    if entry.asset_id == *id_bytes && (entry.flags & FLAG_DELETED) == 0 {
                        return Some(entry);
                    }
                }
            }
        }
        None
    }

    pub fn get_all_entries(&self) -> GpckResult<Vec<FileEntry>> {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();
        let entry_size = std::mem::size_of::<FileEntry>();
        let zero_id = [0u8; 16];

        for bundle in self.bundles.iter().rev() {
            let capacity = bundle.hash_table_capacity as usize;
            for i in 0..capacity {
                let offset = bundle.toc_offset as usize + i * entry_size;
                if let Some(slice) = self.toc_mmap.get(offset..offset + entry_size) {
                    let entry: FileEntry = bytemuck::pod_read_unaligned(slice);
                    if entry.asset_id != zero_id && (entry.flags & FLAG_DELETED) == 0 {
                        let id = Uuid::from_bytes(entry.asset_id);
                        if seen.insert(id) {
                            entries.push(entry);
                        }
                    }
                }
            }
        }
        Ok(entries)
    }

    pub fn get_chunk_table(&self, entry: &FileEntry) -> GpckResult<Vec<ChunkInfo>> {
        let count = entry.chunk_count as usize;
        if count == 0 {
            return Ok(Vec::new());
        }

        let offset = entry.chunk_table_offset as usize;

        let table_bytes = if (entry.flags & FLAG_ENCRYPTED_META) != 0 {
            #[cfg(feature = "crypto")]
            {
                let key = self.decryption_key.ok_or_else(|| {
                    GpckError::Crypto("Encrypted archive requires a valid 32-byte key".to_string())
                })?;

                let enc_size = 28 + count * std::mem::size_of::<ChunkInfo>();
                let slice = self
                    .toc_mmap
                    .get(offset..offset + enc_size)
                    .ok_or_else(|| {
                        GpckError::InvalidFormat(
                            "Corrupted encrypted chunk table offset".to_string(),
                        )
                    })?;

                crate::crypto::aes_gcm::decrypt_chunk_table(slice, &key)?
            }
            #[cfg(not(feature = "crypto"))]
            {
                return Err(GpckError::Crypto(
                    "Encrypted archive encountered, but 'crypto' feature is not enabled in this build"
                        .to_string(),
                ));
            }
        } else {
            let size = count * std::mem::size_of::<ChunkInfo>();
            let slice = self.toc_mmap.get(offset..offset + size).ok_or_else(|| {
                GpckError::InvalidFormat("Corrupted chunk table offset".to_string())
            })?;
            slice.to_vec()
        };

        let chunk_size = std::mem::size_of::<ChunkInfo>();
        let mut chunks = Vec::with_capacity(count);
        for i in 0..count {
            let start = i * chunk_size;
            let end = start + chunk_size;
            let slice = table_bytes.get(start..end).ok_or_else(|| {
                GpckError::InvalidFormat("Corrupted chunk info byte slice".to_string())
            })?;
            chunks.push(bytemuck::pod_read_unaligned::<ChunkInfo>(slice));
        }

        Ok(chunks)
    }

    /// Reads raw chunk data utilizing Linux io_uring O_DIRECT bypass if available,
    /// otherwise mmap or lock-free positional read.
    pub fn read_raw_chunk(&self, chunk: &ChunkInfo) -> GpckResult<Vec<u8>> {
        if chunk.offset < 0 {
            return Err(GpckError::InvalidFormat(
                "Invalid negative chunk offset in archive".to_string(),
            ));
        }

        #[cfg(target_os = "linux")]
        if let Some(ref direct_io) = self.direct_io_reader
            && direct_io.is_direct_io_active()
            && let Ok(data) =
                direct_io.read_exact_at(chunk.offset as u64, chunk.compressed_size as usize)
        {
            return Ok(data);
        }

        let start = chunk.offset as usize;
        let end = start + chunk.compressed_size as usize;

        if let Some(ref mmap) = self.data_mmap
            && let Some(slice) = mmap.get(start..end)
        {
            return Ok(slice.to_vec());
        }

        let mut buf = vec![0u8; chunk.compressed_size as usize];
        read_exact_at(&self.data_file, &mut buf, chunk.offset as u64).map_err(GpckError::Io)?;
        Ok(buf)
    }

    pub fn resolve_base_chunk_local(&self, hash: u64) -> Option<Vec<u8>> {
        let entries = self.get_all_entries().ok()?;
        for entry in entries {
            if let Ok(chunks) = self.get_chunk_table(&entry) {
                for chunk in chunks {
                    if chunk.hash == hash && chunk.offset != -1 {
                        let method = CompressionMethod::from_flags(entry.flags);
                        let raw_bytes = self.read_raw_chunk(&chunk).ok()?;
                        return Codec::decompress(&raw_bytes, chunk.original_size as usize, method)
                            .ok();
                    }
                }
            }
        }
        None
    }

    pub fn resolve_base_chunk(&self, hash: u64) -> Option<Vec<u8>> {
        if let Some(data) = self.resolve_base_chunk_local(hash) {
            return Some(data);
        }
        if let Some(resolver) = &self.chunk_resolver {
            return resolver(hash);
        }
        None
    }

    pub fn read_asset(&self, entry: &FileEntry) -> GpckResult<Vec<u8>> {
        if let Some(slice) = self.try_get_direct_data_slice(entry) {
            return Ok(slice.to_vec());
        }

        let chunks = self.get_chunk_table(entry)?;
        let mut full_payload = Vec::with_capacity(entry.original_size as usize);
        let method = CompressionMethod::from_flags(entry.flags);

        for chunk in chunks {
            let data = if chunk.offset == -1 {
                let chunk_hash = chunk.hash;
                self.resolve_base_chunk(chunk_hash).ok_or_else(|| {
                    GpckError::AssetNotFound(format!(
                        "Base chunk {:016X} not found for delta patch",
                        chunk_hash
                    ))
                })?
            } else {
                let raw_chunk = self.read_raw_chunk(&chunk)?;
                if chunk.compressed_size == chunk.original_size
                    || (entry.flags & FLAG_IS_COMPRESSED) == 0
                {
                    raw_chunk
                } else {
                    Codec::decompress(&raw_chunk, chunk.original_size as usize, method)?
                }
            };
            full_payload.extend_from_slice(&data);
        }

        if entry.sub_chunk_size > 0 {
            let start = entry.sub_chunk_offset as usize;
            let end = start + entry.sub_chunk_size as usize;
            let slice = full_payload.get(start..end).ok_or_else(|| {
                GpckError::InvalidFormat("Sub-chunk slice out of bounds".to_string())
            })?;
            return Ok(slice.to_vec());
        }

        Ok(full_payload)
    }

    pub fn open_stream(self: &Arc<Self>, entry: &FileEntry) -> GpckResult<ArchiveStream> {
        let chunks = self.get_chunk_table(entry)?;
        Ok(ArchiveStream::new(
            self.clone(),
            chunks,
            entry.original_size as u64,
            entry.flags,
        ))
    }

    pub fn get_path_for_asset(&self, entry: &FileEntry) -> Option<String> {
        if entry.name_offset == 0 {
            return None;
        }

        let p = entry.name_offset as usize + 16;
        let len_slice = self.toc_mmap.get(p..p + 2)?;
        let len = u16::from_le_bytes(len_slice.try_into().ok()?) as usize;
        let name_bytes = self.toc_mmap.get(p + 2..p + 2 + len)?;
        String::from_utf8(name_bytes.to_vec()).ok()
    }
}
