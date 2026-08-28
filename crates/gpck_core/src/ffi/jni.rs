// src/ffi/jni.rs
//! # Android Native Interface & JNI Bridge
//!
//! Exposes standard JNI entry points (`Java_com_gpck_vfs_*`) allowing seamless integration
//! with Android applications (Kotlin, Java) and Android NDK game engines.

use crate::core::asset_id::AssetIdGenerator;
use crate::crypto::aes_gcm::derive_key;
use crate::format::archive::GameArchive;
use crate::io::vfs::VirtualFileSystem;
use std::ffi::{CStr, c_char, c_void};
use std::ptr;
use std::sync::Arc;

pub type JniLong = i64;
pub type JniInt = i32;

// ============================================================================
// Android JNI C-ABI Exports (com.gpck.vfs.GpckArchive)
// ============================================================================

/// JNI: Opens a GPCK Archive on Android.
/// Signature: `(Ljava/lang/String;Ljava/lang/String;)J`
///
/// # Safety
/// `path_ptr` must be a valid null-terminated C string. `key_ptr` can be null or a valid null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_gpck_vfs_GpckArchive_nativeOpen(
    _env: *mut c_void,
    _class: *mut c_void,
    path_ptr: *const c_char,
    key_ptr: *const c_char,
) -> JniLong {
    if path_ptr.is_null() {
        return 0;
    }

    let path_str = match unsafe { CStr::from_ptr(path_ptr) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let key_bytes = if !key_ptr.is_null() {
        unsafe { CStr::from_ptr(key_ptr) }
            .to_str()
            .ok()
            .filter(|s| !s.is_empty())
            .map(derive_key)
    } else {
        None
    };

    match GameArchive::open(path_str) {
        Ok(mut arch) => {
            arch.decryption_key = key_bytes;
            let boxed = Box::new(Arc::new(arch));
            Box::into_raw(boxed) as JniLong
        }
        Err(_) => 0,
    }
}

/// JNI: Closes an archive handle on Android.
/// Signature: `(J)V`
///
/// # Safety
/// `handle` must be a valid pointer obtained from `nativeOpen` or 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_gpck_vfs_GpckArchive_nativeClose(
    _env: *mut c_void,
    _class: *mut c_void,
    handle: JniLong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut Arc<GameArchive>));
        }
    }
}

/// JNI: Reads an asset payload into a byte buffer on Android.
/// Signature: `(JLjava/lang/String;[B)I`
///
/// # Safety
/// `handle` must be a valid pointer to an archive. `path_ptr` must be a valid null-terminated C string.
/// `out_buf_ptr` must point to a writable buffer of at least `max_len` bytes, or be null to query size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_gpck_vfs_GpckArchive_nativeReadAsset(
    _env: *mut c_void,
    _class: *mut c_void,
    handle: JniLong,
    path_ptr: *const c_char,
    out_buf_ptr: *mut u8,
    max_len: usize,
) -> JniInt {
    if handle == 0 || path_ptr.is_null() {
        return -1;
    }

    let path_str = match unsafe { CStr::from_ptr(path_ptr) }.to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };

    let archive = unsafe { &*(handle as *const Arc<GameArchive>) };
    let id = AssetIdGenerator::generate(path_str);

    let entry = match archive.try_get_entry(id) {
        Some(e) => e,
        None => return -3,
    };

    let data = match archive.read_asset(&entry) {
        Ok(d) => d,
        Err(_) => return -4,
    };

    if out_buf_ptr.is_null() {
        return data.len() as JniInt;
    }

    if max_len < data.len() {
        return -5;
    }

    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), out_buf_ptr, data.len());
    }

    data.len() as JniInt
}

/// JNI: Direct zero-copy pointer access (< 0.1 µs) to memory-mapped assets.
/// Signature: `(JLjava/lang/String;[J)J`
///
/// # Safety
/// `handle` must be a valid pointer to an archive. `path_ptr` and `out_size_ptr` must be valid non-null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_gpck_vfs_GpckArchive_nativeGetDirectAssetPtr(
    _env: *mut c_void,
    _class: *mut c_void,
    handle: JniLong,
    path_ptr: *const c_char,
    out_size_ptr: *mut usize,
) -> JniLong {
    if handle == 0 || path_ptr.is_null() || out_size_ptr.is_null() {
        return 0;
    }

    let path_str = match unsafe { CStr::from_ptr(path_ptr) }.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let archive = unsafe { &*(handle as *const Arc<GameArchive>) };
    let id = AssetIdGenerator::generate(path_str);

    let entry = match archive.try_get_entry(id) {
        Some(e) => e,
        None => return 0,
    };

    if let Some(slice) = archive.try_get_direct_data_slice(&entry) {
        unsafe {
            *out_size_ptr = slice.len();
        }
        slice.as_ptr() as JniLong
    } else {
        0
    }
}

// ============================================================================
// Android JNI C-ABI Exports (com.gpck.vfs.GpckVfs)
// ============================================================================

/// JNI: Creates a Virtual File System instance on Android.
/// Signature: `()J`
#[unsafe(no_mangle)]
pub extern "C" fn Java_com_gpck_vfs_GpckVfs_nativeCreate(
    _env: *mut c_void,
    _class: *mut c_void,
) -> JniLong {
    let vfs = Box::new(VirtualFileSystem::new());
    Box::into_raw(vfs) as JniLong
}

/// JNI: Destroys a Virtual File System instance on Android.
/// Signature: `(J)V`
///
/// # Safety
/// `handle` must be a valid pointer obtained from `nativeCreate` or 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_gpck_vfs_GpckVfs_nativeDestroy(
    _env: *mut c_void,
    _class: *mut c_void,
    handle: JniLong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut VirtualFileSystem));
        }
    }
}

/// JNI: Mounts an archive into the Android VFS.
/// Signature: `(JLjava/lang/String;)I`
///
/// # Safety
/// `handle` must be a valid pointer to a VFS instance. `path_ptr` must be a valid null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_gpck_vfs_GpckVfs_nativeMountArchive(
    _env: *mut c_void,
    _class: *mut c_void,
    handle: JniLong,
    path_ptr: *const c_char,
) -> JniInt {
    if handle == 0 || path_ptr.is_null() {
        return -1;
    }

    let path_str = match unsafe { CStr::from_ptr(path_ptr) }.to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };

    let vfs = unsafe { &mut *(handle as *mut VirtualFileSystem) };
    match vfs.mount_archive(path_str) {
        Ok(_) => 0,
        Err(_) => -3,
    }
}

/// JNI: Reads an asset file through the Android VFS.
/// Signature: `(JLjava/lang/String;[B)I`
///
/// # Safety
/// `handle` must be a valid pointer to a VFS instance. `path_ptr` must be a valid null-terminated C string.
/// `out_buf_ptr` must point to a writable buffer of at least `max_len` bytes, or be null to query size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Java_com_gpck_vfs_GpckVfs_nativeReadFile(
    _env: *mut c_void,
    _class: *mut c_void,
    handle: JniLong,
    path_ptr: *const c_char,
    out_buf_ptr: *mut u8,
    max_len: usize,
) -> JniInt {
    if handle == 0 || path_ptr.is_null() {
        return -1;
    }

    let path_str = match unsafe { CStr::from_ptr(path_ptr) }.to_str() {
        Ok(s) => s,
        Err(_) => return -2,
    };

    let vfs = unsafe { &*(handle as *const VirtualFileSystem) };
    let data = match vfs.read_file(path_str) {
        Ok(d) => d,
        Err(_) => return -3,
    };

    if out_buf_ptr.is_null() {
        return data.len() as JniInt;
    }

    if max_len < data.len() {
        return -4;
    }

    unsafe {
        ptr::copy_nonoverlapping(data.as_ptr(), out_buf_ptr, data.len());
    }

    data.len() as JniInt
}
