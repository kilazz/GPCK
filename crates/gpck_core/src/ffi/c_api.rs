// crates/gpck_core/src/ffi/c_api.rs
//! # GPCK Native C-API & Foreign Function Interface (FFI)
//!
//! Exposes thread-safe, reference-counted handles (`Arc<GameArchive>`) and RAII
//! asset slices (`GpckAssetSlice`) to guarantee zero memory corruption, lifetime
//! safety, and leak-free resource reclamation across C, C++, and native game engines.

use crate::compression::codecs::CompressionMethod;
use crate::core::asset_id::AssetIdGenerator;
use crate::core::error::GpckError;
use crate::crypto::aes_gcm::derive_key;
use crate::format::archive::GameArchive;
use crate::gacl::GaclTransform;
use crate::graphics::dxgi_format::D3D12FormatTable;
use crate::io::vfs::VirtualFileSystem;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::Arc;
use uuid::Uuid;

#[cfg(windows)]
use crate::gpu::directstorage::{GpuDirectStorage, QueuePriority};
#[cfg(windows)]
use crate::gpu::directstorage_sys::*;
#[cfg(windows)]
use windows::core::BOOL;

// Status & Error Return Codes
pub const GPCK_OK: i32 = 0;
pub const GPCK_ERR_NULL_PTR: i32 = -1;
pub const GPCK_ERR_INVALID_PATH: i32 = -2;
pub const GPCK_ERR_NOT_FOUND: i32 = -3;
pub const GPCK_ERR_BUFFER_TOO_SMALL: i32 = -4;
pub const GPCK_ERR_DECRYPTION_FAILED: i32 = -5;
pub const GPCK_ERR_IO_FAILED: i32 = -6;
pub const GPCK_ERR_NOT_UNCOMPRESSED: i32 = -7;
pub const GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED: i32 = -8;

/// Opaque wrapper around `Arc<GameArchive>`.
pub struct GpckArchive {
    pub inner: Arc<GameArchive>,
}

/// Opaque wrapper around `VirtualFileSystem`.
pub struct GpckVfs {
    pub inner: VirtualFileSystem,
}

/// RAII Asset Slice Handle.
///
/// Holds an explicit `Arc<GameArchive>` reference alongside the direct memory-mapped
/// data slice. Ensures the underlying memory map cannot be closed or unmapped while
/// the caller holds this handle.
pub struct GpckAssetSlice {
    pub _archive_ref: Arc<GameArchive>,
    pub data_ptr: *const u8,
    pub size: usize,
}

// ============================================================================
// Archive Operations
// ============================================================================

/// Opens a GPCK Archive and initializes an atomic reference-counted handle.
///
/// # Safety
/// `path` must point to a valid null-terminated C string.
/// `out_archive` must point to a valid, writable pointer location.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_archive_open(
    path: *const c_char,
    key_passphrase: *const c_char,
    out_archive: *mut *mut GpckArchive,
) -> i32 {
    if path.is_null() || out_archive.is_null() {
        return GPCK_ERR_NULL_PTR;
    }

    let c_path = unsafe { CStr::from_ptr(path) };
    let path_str = match c_path.to_str() {
        Ok(s) => s,
        Err(_) => return GPCK_ERR_INVALID_PATH,
    };

    let key_bytes = if !key_passphrase.is_null() {
        if let Ok(k_str) = (unsafe { CStr::from_ptr(key_passphrase) }).to_str() {
            if !k_str.is_empty() {
                Some(derive_key(k_str))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    match GameArchive::open_with_key(path_str, key_bytes) {
        Ok(arch) => {
            let boxed = Box::new(GpckArchive {
                inner: Arc::new(arch),
            });
            unsafe {
                *out_archive = Box::into_raw(boxed);
            }
            GPCK_OK
        }
        Err(GpckError::DecryptionFailed) => GPCK_ERR_DECRYPTION_FAILED,
        Err(_) => GPCK_ERR_IO_FAILED,
    }
}

/// Increments the reference count of the archive handle and returns a new boxed handle pointer.
///
/// # Safety
/// `archive` must be a valid pointer obtained from `gpck_archive_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_archive_retain(archive: *mut GpckArchive) -> *mut GpckArchive {
    if archive.is_null() {
        return std::ptr::null_mut();
    }
    let handle = unsafe { &*archive };
    let new_boxed = Box::new(GpckArchive {
        inner: handle.inner.clone(),
    });
    Box::into_raw(new_boxed)
}

/// Decrements the reference count and frees memory when reference count drops to zero.
///
/// # Safety
/// `archive` must be a valid pointer obtained from `gpck_archive_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_archive_release(archive: *mut GpckArchive) -> i32 {
    if archive.is_null() {
        return GPCK_ERR_NULL_PTR;
    }
    unsafe {
        drop(Box::from_raw(archive));
    }
    GPCK_OK
}

/// Closes the archive handle safely by decrementing its reference count.
///
/// # Safety
/// `archive` must be a valid pointer obtained from `gpck_archive_open` or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_archive_close(archive: *mut GpckArchive) {
    let _ = unsafe { gpck_archive_release(archive) };
}

/// Retrieves the total number of valid entries in the archive Table of Contents (TOC).
///
/// # Safety
/// `archive` and `out_count` must be valid non-null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_archive_get_entry_count(
    archive: *const GpckArchive,
    out_count: *mut u32,
) -> i32 {
    if archive.is_null() || out_count.is_null() {
        return GPCK_ERR_NULL_PTR;
    }

    let arch = unsafe { &(*archive).inner };
    match arch.get_all_entries() {
        Ok(entries) => {
            unsafe {
                *out_count = entries.len() as u32;
            }
            GPCK_OK
        }
        Err(_) => GPCK_ERR_IO_FAILED,
    }
}

/// Acquires a safe RAII Zero-Copy Asset Slice handle.
///
/// Holds an internal reference count on the archive, guaranteeing that the memory-mapped
/// region remains valid even if another thread closes the main archive handle.
///
/// # Safety
/// All pointers must be valid and non-null. The returned slice handle must be released
/// via `gpck_asset_slice_release` when no longer needed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_archive_acquire_asset_slice(
    archive: *const GpckArchive,
    virtual_path: *const c_char,
    out_slice: *mut *mut GpckAssetSlice,
) -> i32 {
    if archive.is_null() || virtual_path.is_null() || out_slice.is_null() {
        return GPCK_ERR_NULL_PTR;
    }

    let c_path = unsafe { CStr::from_ptr(virtual_path) };
    let path_str = match c_path.to_str() {
        Ok(s) => s,
        Err(_) => return GPCK_ERR_INVALID_PATH,
    };

    let id = AssetIdGenerator::generate(path_str);
    let arch_arc = unsafe { (*archive).inner.clone() };

    let entry = match arch_arc.try_get_entry(id) {
        Some(e) => e,
        None => return GPCK_ERR_NOT_FOUND,
    };

    if let Some(slice) = arch_arc.try_get_direct_data_slice(&entry) {
        let data_ptr = slice.as_ptr();
        let size = slice.len();
        let boxed_slice = Box::new(GpckAssetSlice {
            _archive_ref: arch_arc,
            data_ptr,
            size,
        });
        unsafe {
            *out_slice = Box::into_raw(boxed_slice);
        }
        GPCK_OK
    } else {
        GPCK_ERR_NOT_UNCOMPRESSED
    }
}

/// Reads the pointer and size from an acquired RAII asset slice.
///
/// # Safety
/// `slice`, `out_data_ptr`, and `out_size` must be valid non-null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_asset_slice_get_data(
    slice: *const GpckAssetSlice,
    out_data_ptr: *mut *const u8,
    out_size: *mut usize,
) -> i32 {
    if slice.is_null() || out_data_ptr.is_null() || out_size.is_null() {
        return GPCK_ERR_NULL_PTR;
    }
    let s = unsafe { &*slice };
    unsafe {
        *out_data_ptr = s.data_ptr;
        *out_size = s.size;
    }
    GPCK_OK
}

/// Releases an acquired RAII asset slice handle and decrements the archive refcount.
///
/// # Safety
/// `slice` must be a valid pointer obtained from `gpck_archive_acquire_asset_slice` or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_asset_slice_release(slice: *mut GpckAssetSlice) {
    if !slice.is_null() {
        unsafe {
            drop(Box::from_raw(slice));
        }
    }
}

/// Direct Zero-Copy memory slice access (< 0.1 µs).
/// Returns a direct pointer to the memory-mapped asset if stored uncompressed.
///
/// # Safety
/// All pointers must be valid and non-null. The returned pointer is strictly valid
/// only while the `archive` handle is held open. For multithreaded pipelines,
/// `gpck_archive_acquire_asset_slice` is recommended.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_archive_get_direct_asset_ptr(
    archive: *const GpckArchive,
    virtual_path: *const c_char,
    out_data_ptr: *mut *const u8,
    out_size: *mut usize,
) -> i32 {
    if archive.is_null() || virtual_path.is_null() || out_data_ptr.is_null() || out_size.is_null() {
        return GPCK_ERR_NULL_PTR;
    }

    let c_path = unsafe { CStr::from_ptr(virtual_path) };
    let path_str = match c_path.to_str() {
        Ok(s) => s,
        Err(_) => return GPCK_ERR_INVALID_PATH,
    };

    let id = AssetIdGenerator::generate(path_str);
    let arch = unsafe { &(*archive).inner };

    let entry = match arch.try_get_entry(id) {
        Some(e) => e,
        None => return GPCK_ERR_NOT_FOUND,
    };

    if let Some(slice) = arch.try_get_direct_data_slice(&entry) {
        unsafe {
            *out_data_ptr = slice.as_ptr();
            *out_size = slice.len();
        }
        GPCK_OK
    } else {
        GPCK_ERR_NOT_UNCOMPRESSED
    }
}

/// Reads an asset from the archive by virtual file path into a user-allocated buffer.
///
/// # Safety
/// `archive`, `virtual_path`, and `out_written` must be valid non-null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_archive_read_asset_by_path(
    archive: *mut GpckArchive,
    virtual_path: *const c_char,
    out_buf: *mut u8,
    max_buf_len: usize,
    out_written: *mut usize,
) -> i32 {
    if archive.is_null() || virtual_path.is_null() || out_written.is_null() {
        return GPCK_ERR_NULL_PTR;
    }

    let c_path = unsafe { CStr::from_ptr(virtual_path) };
    let path_str = match c_path.to_str() {
        Ok(s) => s,
        Err(_) => return GPCK_ERR_INVALID_PATH,
    };

    let id = AssetIdGenerator::generate(path_str);
    let arch = unsafe { &(*archive).inner };

    let entry = match arch.try_get_entry(id) {
        Some(e) => e,
        None => return GPCK_ERR_NOT_FOUND,
    };

    let data = match arch.read_asset(&entry) {
        Ok(d) => d,
        Err(_) => return GPCK_ERR_IO_FAILED,
    };

    unsafe {
        *out_written = data.len();
    }

    if out_buf.is_null() {
        return GPCK_OK;
    }

    if max_buf_len < data.len() {
        return GPCK_ERR_BUFFER_TOO_SMALL;
    }

    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), out_buf, data.len());
    }

    GPCK_OK
}

/// Reads an asset from the archive by 128-bit UUID into a user-allocated buffer.
///
/// # Safety
/// `archive`, `uuid_bytes`, and `out_written` must be valid non-null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_archive_read_asset_by_uuid(
    archive: *mut GpckArchive,
    uuid_bytes: *const u8,
    out_buf: *mut u8,
    max_buf_len: usize,
    out_written: *mut usize,
) -> i32 {
    if archive.is_null() || uuid_bytes.is_null() || out_written.is_null() {
        return GPCK_ERR_NULL_PTR;
    }

    let mut id_array = [0u8; 16];
    unsafe {
        ptr::copy_nonoverlapping(uuid_bytes, id_array.as_mut_ptr(), 16);
    }
    let id = Uuid::from_bytes(id_array);

    let arch = unsafe { &(*archive).inner };
    let entry = match arch.try_get_entry(id) {
        Some(e) => e,
        None => return GPCK_ERR_NOT_FOUND,
    };

    let data = match arch.read_asset(&entry) {
        Ok(d) => d,
        Err(_) => return GPCK_ERR_IO_FAILED,
    };

    unsafe {
        *out_written = data.len();
    }

    if out_buf.is_null() {
        return GPCK_OK;
    }

    if max_buf_len < data.len() {
        return GPCK_ERR_BUFFER_TOO_SMALL;
    }

    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), out_buf, data.len());
    }

    GPCK_OK
}

// ============================================================================
// Virtual File System (VFS) Operations
// ============================================================================

/// Creates a new Virtual File System instance.
///
/// # Safety
/// `out_vfs` must point to a valid, writable pointer location.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_vfs_create(out_vfs: *mut *mut GpckVfs) -> i32 {
    if out_vfs.is_null() {
        return GPCK_ERR_NULL_PTR;
    }
    let vfs = Box::new(GpckVfs {
        inner: VirtualFileSystem::new(),
    });
    unsafe {
        *out_vfs = Box::into_raw(vfs);
    }
    GPCK_OK
}

/// Destroys a Virtual File System instance.
///
/// # Safety
/// `vfs` must be a valid pointer obtained from `gpck_vfs_create` or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_vfs_destroy(vfs: *mut GpckVfs) {
    if !vfs.is_null() {
        unsafe {
            drop(Box::from_raw(vfs));
        }
    }
}

/// Mounts an archive file into the VFS search space.
///
/// # Safety
/// `vfs` and `path` must be valid non-null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_vfs_mount_archive(vfs: *mut GpckVfs, path: *const c_char) -> i32 {
    if vfs.is_null() || path.is_null() {
        return GPCK_ERR_NULL_PTR;
    }

    let c_path = unsafe { CStr::from_ptr(path) };
    let path_str = match c_path.to_str() {
        Ok(s) => s,
        Err(_) => return GPCK_ERR_INVALID_PATH,
    };

    let vfs_ref = unsafe { &mut (*vfs).inner };
    match vfs_ref.mount_archive(path_str) {
        Ok(_) => GPCK_OK,
        Err(_) => GPCK_ERR_IO_FAILED,
    }
}

/// Mounts a loose directory into the VFS search space.
///
/// # Safety
/// `vfs` and `path` must be valid non-null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_vfs_mount_directory(vfs: *mut GpckVfs, path: *const c_char) -> i32 {
    if vfs.is_null() || path.is_null() {
        return GPCK_ERR_NULL_PTR;
    }

    let c_path = unsafe { CStr::from_ptr(path) };
    let path_str = match c_path.to_str() {
        Ok(s) => s,
        Err(_) => return GPCK_ERR_INVALID_PATH,
    };

    let vfs_ref = unsafe { &mut (*vfs).inner };
    vfs_ref.mount_directory(path_str);
    GPCK_OK
}

/// Reads a file from the VFS by virtual path.
///
/// # Safety
/// `vfs`, `virtual_path`, and `out_written` must be valid non-null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_vfs_read_file(
    vfs: *mut GpckVfs,
    virtual_path: *const c_char,
    out_buf: *mut u8,
    max_buf_len: usize,
    out_written: *mut usize,
) -> i32 {
    if vfs.is_null() || virtual_path.is_null() || out_written.is_null() {
        return GPCK_ERR_NULL_PTR;
    }

    let c_path = unsafe { CStr::from_ptr(virtual_path) };
    let path_str = match c_path.to_str() {
        Ok(s) => s,
        Err(_) => return GPCK_ERR_INVALID_PATH,
    };

    let vfs_ref = unsafe { &(*vfs).inner };
    let data = match vfs_ref.read_file(path_str) {
        Ok(d) => d,
        Err(_) => return GPCK_ERR_NOT_FOUND,
    };

    unsafe {
        *out_written = data.len();
    }

    if out_buf.is_null() {
        return GPCK_OK;
    }

    if max_buf_len < data.len() {
        return GPCK_ERR_BUFFER_TOO_SMALL;
    }

    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), out_buf, data.len());
    }

    GPCK_OK
}

// ============================================================================
// DirectStorage 1.4 GPU Direct-to-VRAM Streaming C-API
// ============================================================================

/// Returns 1 if DirectStorage 1.4 hardware offload is supported on the host, otherwise 0.
#[unsafe(no_mangle)]
pub extern "C" fn gpck_directstorage_is_supported() -> i32 {
    #[cfg(windows)]
    {
        if let Ok(ds) = GpuDirectStorage::new()
            && ds.is_supported()
        {
            return 1;
        }
        0
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Streams an asset directly from NVMe storage to a D3D12 Linear GPU Buffer (VRAM) without CPU staging.
///
/// # Safety
/// All pointers must be valid and non-null. `d3d12_resource` must point to a valid ID3D12Resource.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_vfs_stream_file_to_d3d12_buffer(
    vfs: *mut GpckVfs,
    virtual_path: *const c_char,
    d3d12_resource: *mut c_void,
    dest_offset: u64,
    priority: i32,
    out_fence_value: *mut u64,
) -> i32 {
    #[cfg(windows)]
    {
        if vfs.is_null()
            || virtual_path.is_null()
            || d3d12_resource.is_null()
            || out_fence_value.is_null()
        {
            return GPCK_ERR_NULL_PTR;
        }

        let c_path = unsafe { CStr::from_ptr(virtual_path) };
        let path_str = match c_path.to_str() {
            Ok(s) => s,
            Err(_) => return GPCK_ERR_INVALID_PATH,
        };

        let ds = match GpuDirectStorage::new() {
            Ok(d) if d.is_supported() => d,
            _ => return GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED,
        };

        let asset_id = AssetIdGenerator::generate(path_str);
        let vfs_ref = unsafe { &(*vfs).inner };

        let (entry, archive) = match vfs_ref.find_entry_and_archive(asset_id) {
            Some(pair) => pair,
            None => return GPCK_ERR_NOT_FOUND,
        };

        let method = CompressionMethod::from_flags(entry.flags);
        let ds_format = match method {
            CompressionMethod::GDeflate => DSTORAGE_COMPRESSION_FORMAT_GDEFLATE,
            CompressionMethod::Zstd => DSTORAGE_COMPRESSION_FORMAT_ZSTD,
            CompressionMethod::BrotliG => DSTORAGE_CUSTOM_COMPRESSION_0,
            _ => DSTORAGE_COMPRESSION_FORMAT_NONE,
        };
        let gacl_transform = entry.gacl_transform() as u8;

        let gdat_path = std::path::Path::new(archive.file_path()).with_extension("gdat");
        let dstorage_file = match ds.open_file(&gdat_path) {
            Ok(f) => f,
            Err(_) => return GPCK_ERR_IO_FAILED,
        };

        let chunks = match archive.get_chunk_table(&entry) {
            Ok(c) => c,
            Err(_) => return GPCK_ERR_IO_FAILED,
        };

        let q_prio = match priority {
            2 => QueuePriority::High,
            0 => QueuePriority::Low,
            _ => QueuePriority::Normal,
        };

        let mut current_offset = dest_offset;
        for chunk in chunks {
            if chunk.offset >= 0 {
                let mut ds_req: DSTORAGE_REQUEST = unsafe { std::mem::zeroed() };
                ds_req.set_file_to_buffer(
                    dstorage_file.ptr(),
                    chunk.offset as u64,
                    chunk.compressed_size,
                    d3d12_resource,
                    current_offset,
                    chunk.original_size,
                    ds_format,
                    gacl_transform,
                );

                ds.enqueue_buffer_request(q_prio, &ds_req);
                current_offset += chunk.original_size as u64;
            }
        }

        match ds.flush_and_signal(q_prio) {
            Ok(fence) => {
                unsafe {
                    *out_fence_value = fence;
                }
                GPCK_OK
            }
            Err(_) => GPCK_ERR_IO_FAILED,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (
            vfs,
            virtual_path,
            d3d12_resource,
            dest_offset,
            priority,
            out_fence_value,
        );
        GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED
    }
}

/// Direct-to-Texture streaming: Unpacks directly into a swizzled D3D12 2D Texture Resource.
///
/// # Safety
/// All pointers must be valid and non-null. `d3d12_texture` must point to an ID3D12Resource with TEXTURE2D dimension.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_vfs_stream_file_to_d3d12_texture(
    vfs: *mut GpckVfs,
    virtual_path: *const c_char,
    d3d12_texture: *mut c_void,
    first_subresource: u32,
    priority: i32,
    out_fence_value: *mut u64,
) -> i32 {
    #[cfg(windows)]
    {
        if vfs.is_null()
            || virtual_path.is_null()
            || d3d12_texture.is_null()
            || out_fence_value.is_null()
        {
            return GPCK_ERR_NULL_PTR;
        }

        let c_path = unsafe { CStr::from_ptr(virtual_path) };
        let path_str = match c_path.to_str() {
            Ok(s) => s,
            Err(_) => return GPCK_ERR_INVALID_PATH,
        };

        let ds = match GpuDirectStorage::new() {
            Ok(d) if d.is_supported() => d,
            _ => return GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED,
        };

        let asset_id = AssetIdGenerator::generate(path_str);
        let vfs_ref = unsafe { &(*vfs).inner };

        let (entry, archive) = match vfs_ref.find_entry_and_archive(asset_id) {
            Some(pair) => pair,
            None => return GPCK_ERR_NOT_FOUND,
        };

        let method = CompressionMethod::from_flags(entry.flags);
        let ds_format = match method {
            CompressionMethod::GDeflate => DSTORAGE_COMPRESSION_FORMAT_GDEFLATE,
            CompressionMethod::Zstd => DSTORAGE_COMPRESSION_FORMAT_ZSTD,
            CompressionMethod::BrotliG => DSTORAGE_CUSTOM_COMPRESSION_0,
            _ => DSTORAGE_COMPRESSION_FORMAT_NONE,
        };

        let gdat_path = std::path::Path::new(archive.file_path()).with_extension("gdat");
        let dstorage_file = match ds.open_file(&gdat_path) {
            Ok(f) => f,
            Err(_) => return GPCK_ERR_IO_FAILED,
        };

        let chunks = match archive.get_chunk_table(&entry) {
            Ok(c) => c,
            Err(_) => return GPCK_ERR_IO_FAILED,
        };

        let q_prio = match priority {
            2 => QueuePriority::High,
            0 => QueuePriority::Low,
            _ => QueuePriority::Normal,
        };

        if let Some(chunk) = chunks.first() {
            let mut ds_req: DSTORAGE_REQUEST = unsafe { std::mem::zeroed() };
            ds_req.set_file_to_texture(
                dstorage_file.ptr(),
                chunk.offset as u64,
                chunk.compressed_size,
                d3d12_texture,
                first_subresource,
                entry.original_size,
                ds_format,
            );

            ds.enqueue_buffer_request(q_prio, &ds_req);
        }

        match ds.flush_and_signal(q_prio) {
            Ok(fence) => {
                unsafe {
                    *out_fence_value = fence;
                }
                GPCK_OK
            }
            Err(_) => GPCK_ERR_IO_FAILED,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (
            vfs,
            virtual_path,
            d3d12_texture,
            first_subresource,
            priority,
            out_fence_value,
        );
        GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED
    }
}

/// Streams a specific 64KB sparse tile directly from NVMe storage to a D3D12 Reserved Tiled Resource (VRAM).
///
/// # Safety
/// All pointers must be valid and non-null. `d3d12_tiled_texture` must point to a valid tiled ID3D12Resource.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gpck_vfs_stream_tile_to_d3d12_texture(
    vfs: *mut GpckVfs,
    virtual_path: *const c_char,
    d3d12_tiled_texture: *mut c_void,
    subresource: u32,
    tile_x: u32,
    tile_y: u32,
    tile_z: u32,
    priority: i32,
    out_fence_value: *mut u64,
) -> i32 {
    #[cfg(windows)]
    {
        if vfs.is_null()
            || virtual_path.is_null()
            || d3d12_tiled_texture.is_null()
            || out_fence_value.is_null()
        {
            return GPCK_ERR_NULL_PTR;
        }

        let c_path = unsafe { CStr::from_ptr(virtual_path) };
        let path_str = match c_path.to_str() {
            Ok(s) => s,
            Err(_) => return GPCK_ERR_INVALID_PATH,
        };

        let ds = match GpuDirectStorage::new() {
            Ok(d) if d.is_supported() => d,
            _ => return GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED,
        };

        let asset_id = AssetIdGenerator::generate(path_str);
        let vfs_ref = unsafe { &(*vfs).inner };

        let (entry, archive) = match vfs_ref.find_entry_and_archive(asset_id) {
            Some(pair) => pair,
            None => return GPCK_ERR_NOT_FOUND,
        };

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

        // O(1) Arithmetic Tile Index Resolution
        let (tilings, packed_info, _total_tiles) = D3D12FormatTable::calculate_subresource_tilings(
            dxgi_fmt,
            width.max(1),
            height.max(1),
            1,
            mip_count.max(1),
            1,
        );

        let tile_index = if subresource < packed_info.num_standard_mips as u32 {
            let tiling = &tilings[subresource as usize];
            tiling.start_tile_index_in_overall_resource as usize
                + (tile_y * tiling.width_in_tiles + tile_x) as usize
        } else {
            packed_info.start_tile_index_in_overall_resource as usize
        };

        let chunks = match archive.get_chunk_table(&entry) {
            Ok(c) => c,
            Err(_) => return GPCK_ERR_IO_FAILED,
        };

        let chunk = match chunks.get(tile_index) {
            Some(c) => c,
            None => return GPCK_ERR_NOT_FOUND,
        };

        if chunk.offset < 0 {
            return GPCK_ERR_IO_FAILED;
        }

        let gdat_path = std::path::Path::new(archive.file_path()).with_extension("gdat");
        let dstorage_file = match ds.open_file(&gdat_path) {
            Ok(f) => f,
            Err(_) => return GPCK_ERR_IO_FAILED,
        };

        let q_prio = match priority {
            2 => QueuePriority::High,
            0 => QueuePriority::Low,
            _ => QueuePriority::Normal,
        };

        let coord = D3D12_TILED_RESOURCE_COORDINATE {
            X: tile_x,
            Y: tile_y,
            Z: tile_z,
            Subresource: subresource,
        };

        let tile_region = D3D12_TILE_REGION_SIZE {
            NumTiles: 1,
            UseBox: BOOL(0),
            Width: 1,
            Height: 1,
            Depth: 1,
        };

        let mut ds_req: DSTORAGE_REQUEST = unsafe { std::mem::zeroed() };
        ds_req.set_file_to_tiles(
            dstorage_file.ptr(),
            chunk.offset as u64,
            chunk.compressed_size,
            d3d12_tiled_texture,
            coord,
            tile_region,
            chunk.original_size,
            ds_format,
            gacl_transform,
        );

        ds.enqueue_tile_request(q_prio, &ds_req);

        match ds.flush_and_signal(q_prio) {
            Ok(fence) => {
                unsafe {
                    *out_fence_value = fence;
                }
                GPCK_OK
            }
            Err(_) => GPCK_ERR_IO_FAILED,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (
            vfs,
            virtual_path,
            d3d12_tiled_texture,
            subresource,
            tile_x,
            tile_y,
            tile_z,
            priority,
            out_fence_value,
        );
        GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED
    }
}

/// Waits on the CPU until the specified DirectStorage hardware queue signals the fence.
#[unsafe(no_mangle)]
pub extern "C" fn gpck_vfs_wait_for_d3d12_fence(priority: i32, fence_value: u64) -> i32 {
    #[cfg(windows)]
    {
        let q_prio = match priority {
            2 => QueuePriority::High,
            0 => QueuePriority::Low,
            _ => QueuePriority::Normal,
        };

        if let Ok(ds) = GpuDirectStorage::new()
            && ds.wait_for_fence(q_prio, fence_value).is_ok()
        {
            return GPCK_OK;
        }
        GPCK_ERR_IO_FAILED
    }
    #[cfg(not(windows))]
    {
        let _ = (priority, fence_value);
        GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED
    }
}
