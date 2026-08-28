// crates/gpck_core/src/gpu/directstorage/windows.rs
//! # Windows DirectStorage 1.4 & D3D12 Native Subsystem
//!
//! Implements hardware-accelerated NVMe BypassIO streaming directly to VRAM
//! (Buffers, 2D Textures, and 64KB Sparse Tiled Resources) with an event-driven
//! custom decompression pool for Brotli-G and native hardware GPU GDeflate offloading.

use super::{GACL_TRANSFORM_NONE, QueuePriority};
use crate::compression::brotlig;
use crate::compression::codecs::CompressionMethod;
use crate::core::error::{GpckError, GpckResult};
use crate::gacl::GaclTransform;
use crate::gpu::directstorage_sys::*;
use crate::gpu::traits::GpuStreamingBackend;
use rayon::prelude::*;

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::System::Threading::*;
use windows::core::{GUID, HRESULT, Interface, PCWSTR};

// ============================================================================
// DirectStorage Error Parsing (from dstorageerr.h)
// ============================================================================

pub fn parse_dstorage_hresult(hr: HRESULT) -> &'static str {
    match hr.0 as u32 {
        E_DSTORAGE_ALREADY_RUNNING => {
            "E_DSTORAGE_ALREADY_RUNNING: DStorage is already running exclusively."
        }
        E_DSTORAGE_NOT_RUNNING => "E_DSTORAGE_NOT_RUNNING: DStorage is not running.",
        E_DSTORAGE_INVALID_QUEUE_CAPACITY => {
            "E_DSTORAGE_INVALID_QUEUE_CAPACITY: Invalid queue capacity parameter."
        }
        E_DSTORAGE_XVD_DEVICE_NOT_SUPPORTED => {
            "E_DSTORAGE_XVD_DEVICE_NOT_SUPPORTED: The specified XVD is not on a supported NVMe device."
        }
        E_DSTORAGE_UNSUPPORTED_VOLUME => {
            "E_DSTORAGE_UNSUPPORTED_VOLUME: The specified XVD is not on a supported volume."
        }
        E_DSTORAGE_END_OF_FILE => {
            "E_DSTORAGE_END_OF_FILE: The specified offset and length exceeds the file size."
        }
        E_DSTORAGE_REQUEST_TOO_LARGE => {
            "E_DSTORAGE_REQUEST_TOO_LARGE: The IO request is too large."
        }
        E_DSTORAGE_ACCESS_VIOLATION => {
            "E_DSTORAGE_ACCESS_VIOLATION: The destination buffer for request is not accessible."
        }
        E_DSTORAGE_UNSUPPORTED_FILE => {
            "E_DSTORAGE_UNSUPPORTED_FILE: The file is not supported by DStorage (e.g. sparse or NTFS compressed)."
        }
        E_DSTORAGE_FILE_NOT_OPEN => "E_DSTORAGE_FILE_NOT_OPEN: The file is not open.",
        E_DSTORAGE_RESERVED_FIELDS => {
            "E_DSTORAGE_RESERVED_FIELDS: A reserved field is not set to 0."
        }
        E_DSTORAGE_INVALID_BCPACK_MODE => {
            "E_DSTORAGE_INVALID_BCPACK_MODE: Invalid BCPack decompression mode."
        }
        E_DSTORAGE_INVALID_SWIZZLE_MODE => {
            "E_DSTORAGE_INVALID_SWIZZLE_MODE: Invalid swizzle mode specified."
        }
        E_DSTORAGE_INVALID_DESTINATION_SIZE => {
            "E_DSTORAGE_INVALID_DESTINATION_SIZE: Destination size is invalid."
        }
        E_DSTORAGE_QUEUE_CLOSED => {
            "E_DSTORAGE_QUEUE_CLOSED: The request targets a queue that is closed."
        }
        E_DSTORAGE_INVALID_CLUSTER_SIZE => {
            "E_DSTORAGE_INVALID_CLUSTER_SIZE: Volume formatted with unsupported cluster size."
        }
        E_DSTORAGE_TOO_MANY_QUEUES => {
            "E_DSTORAGE_TOO_MANY_QUEUES: Maximum number of queues reached."
        }
        E_DSTORAGE_INVALID_QUEUE_PRIORITY => {
            "E_DSTORAGE_INVALID_QUEUE_PRIORITY: Invalid priority specified for the queue."
        }
        E_DSTORAGE_TOO_MANY_FILES => {
            "E_DSTORAGE_TOO_MANY_FILES: Maximum number of open files reached."
        }
        E_DSTORAGE_INDEX_BOUND => "E_DSTORAGE_INDEX_BOUND: The index parameter is out of bounds.",
        E_DSTORAGE_IO_TIMEOUT => "E_DSTORAGE_IO_TIMEOUT: The IO operation has timed out.",
        E_DSTORAGE_INVALID_FILE_HANDLE => {
            "E_DSTORAGE_INVALID_FILE_HANDLE: The specified file has not been opened."
        }
        E_DSTORAGE_DEPRECATED_PREVIEW_GDK => {
            "E_DSTORAGE_DEPRECATED_PREVIEW_GDK: Deprecated preview GDK version."
        }
        E_DSTORAGE_XVD_NOT_REGISTERED => {
            "E_DSTORAGE_XVD_NOT_REGISTERED: The specified XVD is not registered or unmounted."
        }
        E_DSTORAGE_INVALID_FILE_OFFSET => {
            "E_DSTORAGE_INVALID_FILE_OFFSET: Invalid file offset for specified decompression mode."
        }
        E_DSTORAGE_INVALID_SOURCE_TYPE => {
            "E_DSTORAGE_INVALID_SOURCE_TYPE: Source type mismatch between memory and file queue."
        }
        E_DSTORAGE_INVALID_INTERMEDIATE_SIZE => {
            "E_DSTORAGE_INVALID_INTERMEDIATE_SIZE: Invalid intermediate size for decompression mode."
        }
        E_DSTORAGE_SYSTEM_NOT_SUPPORTED => {
            "E_DSTORAGE_SYSTEM_NOT_SUPPORTED: System/hardware generation does not support DirectStorage."
        }
        E_DSTORAGE_STAGING_BUFFER_LOCKED => {
            "E_DSTORAGE_STAGING_BUFFER_LOCKED: Staging buffer size can only be changed when no queues/files are open."
        }
        E_DSTORAGE_INVALID_STAGING_BUFFER_SIZE => {
            "E_DSTORAGE_INVALID_STAGING_BUFFER_SIZE: Staging buffer size is not valid."
        }
        E_DSTORAGE_STAGING_BUFFER_TOO_SMALL => {
            "E_DSTORAGE_STAGING_BUFFER_TOO_SMALL: Staging buffer is too small for this operation."
        }
        E_DSTORAGE_INVALID_FENCE => {
            "E_DSTORAGE_INVALID_FENCE: The fence is invalid or has been released."
        }
        E_DSTORAGE_INVALID_STATUS_ARRAY => {
            "E_DSTORAGE_INVALID_STATUS_ARRAY: The status array is invalid or has been released."
        }
        E_DSTORAGE_INVALID_MEMORY_QUEUE_PRIORITY => {
            "E_DSTORAGE_INVALID_MEMORY_QUEUE_PRIORITY: Only DSTORAGE_PRIORITY_REALTIME is valid for memory queues."
        }
        E_DSTORAGE_DECOMPRESSION_ERROR => {
            "E_DSTORAGE_DECOMPRESSION_ERROR: Generic error occurred during GPU/CPU decompression."
        }
        E_DSTORAGE_ZLIB_BAD_HEADER => "E_DSTORAGE_ZLIB_BAD_HEADER: ZLIB header is corrupted.",
        E_DSTORAGE_ZLIB_BAD_DATA => "E_DSTORAGE_ZLIB_BAD_DATA: ZLIB compressed data is corrupted.",
        E_DSTORAGE_ZLIB_PARITY_FAIL => {
            "E_DSTORAGE_ZLIB_PARITY_FAIL: Block-level ADLER parity check failed during ZLIB decompression."
        }
        E_DSTORAGE_BCPACK_BAD_HEADER => "E_DSTORAGE_BCPACK_BAD_HEADER: BCPack header is corrupted.",
        E_DSTORAGE_BCPACK_BAD_DATA => {
            "E_DSTORAGE_BCPACK_BAD_DATA: BCPack decoder produced excess data."
        }
        E_DSTORAGE_DECRYPTION_ERROR => {
            "E_DSTORAGE_DECRYPTION_ERROR: Error occurred during decryption."
        }
        E_DSTORAGE_PASSTHROUGH_ERROR => {
            "E_DSTORAGE_PASSTHROUGH_ERROR: Generic error during copy operation."
        }
        E_DSTORAGE_FILE_TOO_FRAGMENTED => {
            "E_DSTORAGE_FILE_TOO_FRAGMENTED: The file is too fragmented to be accessed by DirectStorage."
        }
        E_DSTORAGE_COMPRESSED_DATA_TOO_LARGE => {
            "E_DSTORAGE_COMPRESSED_DATA_TOO_LARGE: Resulting compressed size is too large for GPU decompression."
        }
        E_DSTORAGE_INVALID_DESTINATION_TYPE => {
            "E_DSTORAGE_INVALID_DESTINATION_TYPE: Target request has invalid or unsupported destination type."
        }
        E_DSTORAGE_FILEBUFFERING_REQUIRES_DISABLED_BYPASSIO => {
            "E_DSTORAGE_FILEBUFFERING_REQUIRES_DISABLED_BYPASSIO: ForceFileBuffering requires DisableBypassIO = TRUE."
        }
        E_DSTORAGE_SCRATCH_BUFFER_TOO_SMALL => {
            "E_DSTORAGE_SCRATCH_BUFFER_TOO_SMALL: Driver scratch buffer is too small to decompress buffers."
        }
        E_DSTORAGE_INVALID_GACL_SHUFFLE_TRANSFORM_TYPE => {
            "E_DSTORAGE_INVALID_GACL_SHUFFLE_TRANSFORM_TYPE: GACL transform type not supported by device/format."
        }
        _ => "UNKNOWN_DSTORAGE_ERROR",
    }
}

// ============================================================================
// Native C-ABI Bindings for Microsoft ATG ZstdGPU Library
// ============================================================================

unsafe extern "C" {
    fn ZstdGpu_CreateContext(device: *mut c_void) -> *mut c_void;
    fn ZstdGpu_Decompress(
        handle: *mut c_void,
        in_compressed_data: *const u8,
        in_compressed_size: u32,
        out_vram_buffer: *mut c_void,
        out_vram_offset: u64,
        out_uncompressed_size: u32,
    ) -> bool;
    fn ZstdGpu_DestroyContext(handle: *mut c_void);
}

// ============================================================================
// COM Interface RAII Wrappers
// ============================================================================

pub struct IDStorageFactory(*mut c_void);

unsafe impl Send for IDStorageFactory {}
unsafe impl Sync for IDStorageFactory {}

impl IDStorageFactory {
    /// Wraps a raw pointer into an `IDStorageFactory`.
    ///
    /// # Safety
    /// `ptr` must be a valid, non-null pointer to an initialized `IDStorageFactory` COM interface.
    pub unsafe fn from_raw(ptr: *mut c_void) -> Self {
        Self(ptr)
    }

    pub fn as_raw(&self) -> *mut c_void {
        self.0
    }

    /// Queries an interface on the DirectStorage Factory.
    ///
    /// # Safety
    /// `riid` and `out_ptr` must be valid, non-null pointers, and `out_ptr` must be writable.
    pub unsafe fn query_interface(&self, riid: &GUID, out_ptr: *mut *mut c_void) -> HRESULT {
        unsafe {
            let vtbl = *(self.0 as *const *const IDStorageFactoryVtbl);
            ((*vtbl).parent.QueryInterface)(self.0, riid, out_ptr)
        }
    }

    /// Creates a DirectStorage queue with the specified descriptor.
    ///
    /// # Safety
    /// `desc` must point to a valid, properly initialized `DSTORAGE_QUEUE_DESC`, and `riid` must be a valid DirectStorage queue GUID.
    pub unsafe fn create_queue(
        &self,
        desc: &DSTORAGE_QUEUE_DESC,
        riid: &GUID,
    ) -> GpckResult<IDStorageQueue> {
        let mut out: *mut c_void = std::ptr::null_mut();
        let hr = unsafe {
            let vtbl = *(self.0 as *const *const IDStorageFactoryVtbl);
            ((*vtbl).CreateQueue)(self.0, desc, riid, &mut out)
        };
        if hr.is_err() {
            return Err(GpckError::DirectStorageError {
                hresult: hr.0 as u32,
                message: "IDStorageFactory::CreateQueue failed",
            });
        }
        Ok(unsafe { IDStorageQueue::from_raw(out) })
    }

    /// Opens a file for DirectStorage access.
    ///
    /// # Safety
    /// `path` must be a valid, null-terminated wide string pointer, and `riid` must be a valid DirectStorage file GUID.
    pub unsafe fn open_file(&self, path: PCWSTR, riid: &GUID) -> GpckResult<IDStorageFile> {
        let mut out: *mut c_void = std::ptr::null_mut();
        let hr = unsafe {
            let vtbl = *(self.0 as *const *const IDStorageFactoryVtbl);
            ((*vtbl).OpenFile)(self.0, path, riid, &mut out)
        };
        if hr.is_err() {
            return Err(GpckError::DirectStorageError {
                hresult: hr.0 as u32,
                message: "IDStorageFactory::OpenFile failed",
            });
        }
        Ok(unsafe { IDStorageFile::from_raw(out) })
    }

    /// Sets DirectStorage debug flags.
    ///
    /// # Safety
    /// Must be called when no requests are in flight, and `flags` must contain valid DirectStorage debug bitflags.
    pub unsafe fn set_debug_flags(&self, flags: u32) -> GpckResult<()> {
        let hr = unsafe {
            let vtbl = *(self.0 as *const *const IDStorageFactoryVtbl);
            ((*vtbl).SetDebugFlags)(self.0, flags)
        };
        if hr.is_err() {
            return Err(GpckError::DirectStorageError {
                hresult: hr.0 as u32,
                message: "IDStorageFactory::SetDebugFlags failed",
            });
        }
        Ok(())
    }

    /// Sets DirectStorage staging buffer size.
    ///
    /// # Safety
    /// Must only be called when no DirectStorage queues or files are open or actively processing requests.
    pub unsafe fn set_staging_buffer_size(&self, size: u32) -> GpckResult<()> {
        let hr = unsafe {
            let vtbl = *(self.0 as *const *const IDStorageFactoryVtbl);
            ((*vtbl).SetStagingBufferSize)(self.0, size)
        };
        if hr.is_err() {
            return Err(GpckError::DirectStorageError {
                hresult: hr.0 as u32,
                message: "IDStorageFactory::SetStagingBufferSize failed",
            });
        }
        Ok(())
    }
}

impl Clone for IDStorageFactory {
    fn clone(&self) -> Self {
        if !self.0.is_null() {
            unsafe {
                let vtbl = *(self.0 as *const *const IDStorageFactoryVtbl);
                ((*vtbl).parent.AddRef)(self.0);
            }
        }
        Self(self.0)
    }
}

impl Drop for IDStorageFactory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let vtbl = *(self.0 as *const *const IDStorageFactoryVtbl);
                ((*vtbl).parent.Release)(self.0);
            }
            self.0 = std::ptr::null_mut();
        }
    }
}

pub struct IDStorageQueue(*mut c_void);

unsafe impl Send for IDStorageQueue {}
unsafe impl Sync for IDStorageQueue {}

impl IDStorageQueue {
    /// Wraps a raw pointer into an `IDStorageQueue`.
    ///
    /// # Safety
    /// `ptr` must be a valid, non-null pointer to an initialized `IDStorageQueue` COM interface.
    pub unsafe fn from_raw(ptr: *mut c_void) -> Self {
        Self(ptr)
    }

    pub fn as_raw(&self) -> *mut c_void {
        self.0
    }

    /// Enqueues a read request into the DirectStorage queue.
    ///
    /// # Safety
    /// `request` must point to a valid `DSTORAGE_REQUEST` whose source and destination buffers remain allocated until completion.
    pub unsafe fn enqueue_request(&self, request: &DSTORAGE_REQUEST) {
        unsafe {
            let vtbl = *(self.0 as *const *const IDStorageQueueVtbl);
            ((*vtbl).EnqueueRequest)(self.0, request);
        }
    }

    /// Cancels in-flight requests matching the provided cancellation mask and value.
    ///
    /// # Safety
    /// The queue COM interface pointer must remain valid during execution.
    pub unsafe fn cancel_requests_with_tag(&self, mask: u64, value: u64) {
        unsafe {
            let vtbl = *(self.0 as *const *const IDStorageQueueVtbl);
            ((*vtbl).CancelRequestsWithTag)(self.0, mask, value);
        }
    }

    /// Enqueues a fence signal operation after all previous requests complete.
    ///
    /// # Safety
    /// `fence` must be a valid, non-null pointer to an `ID3D12Fence` interface.
    pub unsafe fn enqueue_signal(&self, fence: *mut c_void, value: u64) {
        unsafe {
            let vtbl = *(self.0 as *const *const IDStorageQueueVtbl);
            ((*vtbl).EnqueueSignal)(self.0, fence, value);
        }
    }

    /// Submits all enqueued requests to the underlying hardware queues.
    ///
    /// # Safety
    /// The queue COM interface pointer must remain valid and not closed.
    pub unsafe fn submit(&self) {
        unsafe {
            let vtbl = *(self.0 as *const *const IDStorageQueueVtbl);
            ((*vtbl).Submit)(self.0);
        }
    }

    /// Closes the DirectStorage queue, terminating further request submissions.
    ///
    /// # Safety
    /// The queue COM interface pointer must be valid.
    pub unsafe fn close(&self) {
        if !self.0.is_null() {
            unsafe {
                let vtbl = *(self.0 as *const *const IDStorageQueueVtbl);
                ((*vtbl).Close)(self.0);
            }
        }
    }

    /// Retrieves error diagnostic record if any request failed.
    ///
    /// # Safety
    /// `record` must be a valid, properly aligned mutable pointer to a `DSTORAGE_ERROR_RECORD`.
    pub unsafe fn retrieve_error_record(&self, record: &mut DSTORAGE_ERROR_RECORD) {
        unsafe {
            let vtbl = *(self.0 as *const *const IDStorageQueueVtbl);
            ((*vtbl).RetrieveErrorRecord)(self.0, record);
        }
    }
}

impl Clone for IDStorageQueue {
    fn clone(&self) -> Self {
        if !self.0.is_null() {
            unsafe {
                let vtbl = *(self.0 as *const *const IDStorageQueueVtbl);
                ((*vtbl).parent.AddRef)(self.0);
            }
        }
        Self(self.0)
    }
}

impl Drop for IDStorageQueue {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let vtbl = *(self.0 as *const *const IDStorageQueueVtbl);
                ((*vtbl).parent.Release)(self.0);
            }
            self.0 = std::ptr::null_mut();
        }
    }
}

pub struct IDStorageFile(*mut c_void);

unsafe impl Send for IDStorageFile {}
unsafe impl Sync for IDStorageFile {}

impl IDStorageFile {
    /// Wraps a raw pointer into an `IDStorageFile`.
    ///
    /// # Safety
    /// `ptr` must be a valid, non-null pointer to an initialized `IDStorageFile` COM interface.
    pub unsafe fn from_raw(ptr: *mut c_void) -> Self {
        Self(ptr)
    }

    pub fn as_raw(&self) -> *mut c_void {
        self.0
    }

    /// Closes the DirectStorage file handle.
    ///
    /// # Safety
    /// No active enqueued queue requests may be reading from this file handle when closed.
    pub unsafe fn close(&self) {
        if !self.0.is_null() {
            unsafe {
                let vtbl = *(self.0 as *const *const IDStorageFileVtbl);
                ((*vtbl).Close)(self.0);
            }
        }
    }
}

impl Clone for IDStorageFile {
    fn clone(&self) -> Self {
        if !self.0.is_null() {
            unsafe {
                let vtbl = *(self.0 as *const *const IDStorageFileVtbl);
                ((*vtbl).parent.AddRef)(self.0);
            }
        }
        Self(self.0)
    }
}

impl Drop for IDStorageFile {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let vtbl = *(self.0 as *const *const IDStorageFileVtbl);
                ((*vtbl).parent.Release)(self.0);
            }
            self.0 = std::ptr::null_mut();
        }
    }
}

pub struct IDStorageCustomDecompressionQueue1(*mut c_void);

unsafe impl Send for IDStorageCustomDecompressionQueue1 {}
unsafe impl Sync for IDStorageCustomDecompressionQueue1 {}

impl IDStorageCustomDecompressionQueue1 {
    /// Wraps a raw pointer into an `IDStorageCustomDecompressionQueue1`.
    ///
    /// # Safety
    /// `ptr` must be a valid, non-null pointer to an `IDStorageCustomDecompressionQueue1` interface.
    pub unsafe fn from_raw(ptr: *mut c_void) -> Self {
        Self(ptr)
    }

    pub fn get_event(&self) -> HANDLE {
        unsafe {
            let vtbl = *(self.0 as *const *const IDStorageCustomDecompressionQueue1Vtbl);
            let raw_handle = ((*vtbl).GetEvent)(self.0);
            HANDLE(raw_handle)
        }
    }

    /// Retrieves custom decompression requests from the queue.
    ///
    /// # Safety
    /// `requests` must point to an array of at least `max_requests` elements, and `num_requests` must be a valid writable pointer.
    pub unsafe fn get_requests1(
        &self,
        flags: u32,
        max_requests: u32,
        requests: *mut DSTORAGE_CUSTOM_DECOMPRESSION_REQUEST,
        num_requests: *mut u32,
    ) -> HRESULT {
        unsafe {
            let vtbl = *(self.0 as *const *const IDStorageCustomDecompressionQueue1Vtbl);
            ((*vtbl).GetRequests1)(self.0, flags, max_requests, requests, num_requests)
        }
    }

    /// Submits completed decompression results back to DirectStorage.
    ///
    /// # Safety
    /// `results` must point to an array of at least `num_results` valid `DSTORAGE_CUSTOM_DECOMPRESSION_RESULT` structs.
    pub unsafe fn set_request_results(
        &self,
        num_results: u32,
        results: *const DSTORAGE_CUSTOM_DECOMPRESSION_RESULT,
    ) -> HRESULT {
        unsafe {
            let vtbl = *(self.0 as *const *const IDStorageCustomDecompressionQueue1Vtbl);
            ((*vtbl).SetRequestResults)(self.0, num_results, results)
        }
    }
}

impl Drop for IDStorageCustomDecompressionQueue1 {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let vtbl = *(self.0 as *const *const IDStorageCustomDecompressionQueue1Vtbl);
                ((*vtbl).parent.Release)(self.0);
            }
            self.0 = std::ptr::null_mut();
        }
    }
}

type DStorageSetConfiguration2Fn =
    unsafe extern "system" fn(*const DSTORAGE_CONFIGURATION2) -> HRESULT;
type DStorageGetFactoryFn = unsafe extern "system" fn(*const GUID, *mut *mut c_void) -> HRESULT;

pub struct DStorageFile {
    file: IDStorageFile,
}

impl DStorageFile {
    pub fn ptr(&self) -> *mut c_void {
        self.file.as_raw()
    }
}

fn load_dstorage_library() -> GpckResult<&'static libloading::Library> {
    static DSTORAGE_LIB: OnceLock<Option<libloading::Library>> = OnceLock::new();

    let lib_opt = DSTORAGE_LIB.get_or_init(|| {
        let mut candidates = Vec::new();
        candidates.push(PathBuf::from("dstorage.dll"));
        if let Ok(exe_path) = std::env::current_exe()
            && let Some(exe_dir) = exe_path.parent()
        {
            candidates.push(exe_dir.join("dstorage.dll"));
        }
        candidates.push(PathBuf::from("target/release/dstorage.dll"));
        candidates.push(PathBuf::from("target/debug/dstorage.dll"));

        if let Ok(entries) = std::fs::read_dir("nuget") {
            for entry in entries.filter_map(|e| e.ok()) {
                let bin_dll = entry.path().join("native/bin/x64/dstorage.dll");
                if bin_dll.exists() {
                    candidates.push(bin_dll);
                }
            }
        }

        for path in candidates {
            if let Ok(lib) = unsafe { libloading::Library::new(&path) } {
                return Some(lib);
            }
        }
        None
    });

    lib_opt.as_ref().ok_or(GpckError::DirectStorageUnsupported)
}

// ============================================================================
// DirectStorage Windows Engine Implementation
// ============================================================================

pub struct GpuDirectStorage {
    file_queues: [Option<IDStorageQueue>; 3],
    memory_queue: Option<IDStorageQueue>,
    factory: Option<IDStorageFactory>,
    fences: [Option<ID3D12Fence>; 4],
    fence_events: [HANDLE; 4],
    fence_values: [AtomicU64; 4],
    custom_decomp_queue: Option<Arc<IDStorageCustomDecompressionQueue1>>,
    custom_decomp_shutdown: Arc<AtomicBool>,
    custom_decomp_shutdown_event: Option<HANDLE>,
    custom_decomp_thread: Option<JoinHandle<()>>,
    debug_cookie: Option<u32>,
    device: Option<ID3D12Device>,
    is_supported: bool,
}

unsafe impl Send for GpuDirectStorage {}
unsafe impl Sync for GpuDirectStorage {}

impl GpuDirectStorage {
    pub fn new() -> GpckResult<Self> {
        crate::gpu::dred::DredDiagnosticEngine::configure_dred();

        let mut device: Option<ID3D12Device> = None;
        unsafe {
            if let Err(e) = D3D12CreateDevice(None, D3D_FEATURE_LEVEL_12_0, &mut device) {
                return Err(GpckError::DirectStorageError {
                    hresult: e.code().0 as u32,
                    message: "Failed to create D3D12 Device",
                });
            }
        }
        let device = device.unwrap();
        let debug_cookie = crate::gpu::debug_layer::attach_d3d12_debug_callback(&device);
        let dstorage_lib = load_dstorage_library()?;

        unsafe {
            let gpck_creator_id = GUID::from_u128(0x4750434b_0000_0000_0000_000000000000);
            if let Ok(set_config2) =
                dstorage_lib.get::<DStorageSetConfiguration2Fn>(b"DStorageSetConfiguration2\0")
            {
                let config = DSTORAGE_CONFIGURATION2 {
                    NumSubmitThreads: 0,
                    NumBuiltInCpuDecompressionThreads: 0,
                    ForceMappingLayer: 0,
                    DisableBypassIO: 0,
                    DisableTelemetry: 1,
                    DisableGpuDecompressionMetacommand: 0,
                    DisableGpuDecompression: 0,
                    ForceFileBuffering: 0,
                    CreatorID: gpck_creator_id,
                };
                let _ = set_config2(&config);
            }

            let get_factory: libloading::Symbol<DStorageGetFactoryFn> = dstorage_lib
                .get(b"DStorageGetFactory\0")
                .map_err(|_| GpckError::DirectStorageUnsupported)?;

            let factory_iid = GUID::from_u128(0x6924ea0c_c3cd_4826_b10a_f64f4ed927c1);
            let mut factory_raw: *mut c_void = std::ptr::null_mut();
            let hr = get_factory(&factory_iid, &mut factory_raw);

            if hr.is_err() || factory_raw.is_null() {
                return Err(GpckError::DirectStorageError {
                    hresult: hr.0 as u32,
                    message: "DStorageGetFactory failed",
                });
            }

            let factory = IDStorageFactory::from_raw(factory_raw);
            let _ = factory.set_staging_buffer_size(256 * 1024 * 1024);
            let _ = factory.set_debug_flags(0x01 | 0x04);

            let custom_queue_iid_primary = GUID::from_u128(0x0d47c6c9_e61a_4706_93b4_68bfe3f4aa4a);
            let custom_queue_iid_secondary =
                GUID::from_u128(0x3a22839d_5a7e_4967_85e8_0776670702a3);
            let mut custom_queue_raw: *mut c_void = std::ptr::null_mut();

            let custom_decomp_queue = if (factory
                .query_interface(&custom_queue_iid_primary, &mut custom_queue_raw)
                .is_ok()
                || factory
                    .query_interface(&custom_queue_iid_secondary, &mut custom_queue_raw)
                    .is_ok())
                && !custom_queue_raw.is_null()
            {
                Some(Arc::new(IDStorageCustomDecompressionQueue1::from_raw(
                    custom_queue_raw,
                )))
            } else {
                None
            };

            let custom_decomp_shutdown = Arc::new(AtomicBool::new(false));
            let shutdown_event = CreateEventW(None, true, false, PCWSTR::null()).map_err(|e| {
                GpckError::DirectStorageError {
                    hresult: e.code().0 as u32,
                    message: "CreateEventW for Custom Decompression shutdown failed",
                }
            })?;

            let mut custom_decomp_thread = None;

            if let Some(ref queue) = custom_decomp_queue {
                let queue_clone = queue.clone();
                let shutdown_clone = custom_decomp_shutdown.clone();
                let ds_event = queue.get_event();
                let ds_event_raw = ds_event.0 as usize;
                let shutdown_event_raw = shutdown_event.0 as usize;

                let handle = std::thread::Builder::new()
                    .name("gpck-brotlig-decomp-pool".to_string())
                    .spawn(move || {
                        let ds_event = HANDLE(ds_event_raw as *mut c_void);
                        let shutdown_event = HANDLE(shutdown_event_raw as *mut c_void);
                        let wait_handles = [ds_event, shutdown_event];

                        let mut requests =
                            [std::mem::zeroed::<DSTORAGE_CUSTOM_DECOMPRESSION_REQUEST>(); 64];
                        let mut results =
                            [std::mem::zeroed::<DSTORAGE_CUSTOM_DECOMPRESSION_RESULT>(); 64];

                        while !shutdown_clone.load(Ordering::Relaxed) {
                            let wait_res = WaitForMultipleObjects(&wait_handles, false, INFINITE);

                            if wait_res == WAIT_EVENT(WAIT_OBJECT_0.0 + 1) {
                                break;
                            }

                            if wait_res == WAIT_OBJECT_0 {
                                let mut num_requests = 0u32;
                                let hr = queue_clone.get_requests1(
                                    DSTORAGE_GET_REQUEST_FLAG_SELECT_ALL,
                                    64,
                                    requests.as_mut_ptr(),
                                    &mut num_requests,
                                );

                                if hr.is_ok() && num_requests > 0 {
                                    let active_requests = &requests[..num_requests as usize];

                                    let work_items: Vec<_> = active_requests
                                        .iter()
                                        .enumerate()
                                        .map(|(i, req)| {
                                            (
                                                i,
                                                req.Id,
                                                req.CompressionFormat,
                                                req.SrcBuffer as usize,
                                                req.SrcSize as usize,
                                                req.DstBuffer as usize,
                                                req.DstSize as usize,
                                            )
                                        })
                                        .collect();

                                    let decomp_results: Vec<_> = work_items
                                        .into_par_iter()
                                        .map(|(i, id, format, src_ptr, src_sz, dst_ptr, dst_sz)| {
                                            let mut success = false;
                                            if format == DSTORAGE_CUSTOM_COMPRESSION_0 {
                                                let src_slice = std::slice::from_raw_parts(
                                                    src_ptr as *const u8,
                                                    src_sz,
                                                );
                                                let dst_slice = std::slice::from_raw_parts_mut(
                                                    dst_ptr as *mut u8,
                                                    dst_sz,
                                                );

                                                if let Ok(decomp) =
                                                    brotlig::decompress(src_slice, dst_sz)
                                                {
                                                    dst_slice[..decomp.len().min(dst_sz)]
                                                        .copy_from_slice(&decomp);
                                                    success = true;
                                                }
                                            }

                                            (
                                                i,
                                                DSTORAGE_CUSTOM_DECOMPRESSION_RESULT {
                                                    Id: id,
                                                    Result: if success { S_OK } else { E_FAIL },
                                                    _pad: 0,
                                                },
                                            )
                                        })
                                        .collect();

                                    for (i, res) in decomp_results {
                                        results[i] = res;
                                    }

                                    let _ = queue_clone
                                        .set_request_results(num_requests, results.as_ptr());
                                }
                            }
                        }
                    })
                    .ok();
                custom_decomp_thread = handle;
            }

            let device_ptr = Interface::as_raw(&device);
            let queue_iid = GUID::from_u128(0xcfdbd83f_9e06_4fda_8ea5_69042137f49b);

            let mem_queue_desc = DSTORAGE_QUEUE_DESC {
                SourceType: DSTORAGE_REQUEST_SOURCE_MEMORY,
                Capacity: 2048,
                Priority: DSTORAGE_PRIORITY_REALTIME,
                _pad: [0; 5],
                Name: std::ptr::null(),
                Device: device_ptr,
            };
            let memory_queue = factory.create_queue(&mem_queue_desc, &queue_iid)?;

            let priorities = [
                DSTORAGE_PRIORITY_LOW,
                DSTORAGE_PRIORITY_NORMAL,
                DSTORAGE_PRIORITY_HIGH,
            ];
            let mut queues = Vec::new();
            for &prio in &priorities {
                let file_queue_desc = DSTORAGE_QUEUE_DESC {
                    SourceType: DSTORAGE_REQUEST_SOURCE_FILE,
                    Capacity: 2048,
                    Priority: prio,
                    _pad: [0; 5],
                    Name: std::ptr::null(),
                    Device: device_ptr,
                };
                queues.push(factory.create_queue(&file_queue_desc, &queue_iid)?);
            }

            let file_queues = [
                Some(queues[0].clone()),
                Some(queues[1].clone()),
                Some(queues[2].clone()),
            ];

            let mut fences: [Option<ID3D12Fence>; 4] = [None, None, None, None];
            let mut fence_events: [HANDLE; 4] = [HANDLE::default(); 4];

            for i in 0..4 {
                let fence: ID3D12Fence =
                    device.CreateFence(0, D3D12_FENCE_FLAG_NONE).map_err(|e| {
                        GpckError::DirectStorageError {
                            hresult: e.code().0 as u32,
                            message: "CreateFence failed",
                        }
                    })?;
                let event = CreateEventW(None, false, false, PCWSTR::null()).map_err(|e| {
                    GpckError::DirectStorageError {
                        hresult: e.code().0 as u32,
                        message: "CreateEventW failed",
                    }
                })?;
                fences[i] = Some(fence);
                fence_events[i] = event;
            }

            Ok(Self {
                file_queues,
                memory_queue: Some(memory_queue),
                factory: Some(factory),
                fences,
                fence_events,
                fence_values: [
                    AtomicU64::new(0),
                    AtomicU64::new(0),
                    AtomicU64::new(0),
                    AtomicU64::new(0),
                ],
                custom_decomp_queue,
                custom_decomp_shutdown,
                custom_decomp_shutdown_event: Some(shutdown_event),
                custom_decomp_thread,
                debug_cookie,
                device: Some(device),
                is_supported: true,
            })
        }
    }

    pub fn is_supported(&self) -> bool {
        self.is_supported
    }

    pub fn open_file<P: AsRef<Path>>(&self, path: P) -> GpckResult<DStorageFile> {
        use windows::core::HSTRING;
        let path_hstring = HSTRING::from(path.as_ref());
        let file_iid = GUID::from_u128(0x5de95e7b_955a_4868_a73c_243b29f4b8da);

        let factory = self
            .factory
            .as_ref()
            .ok_or(GpckError::DirectStorageUnsupported)?;
        unsafe {
            let file = factory.open_file(PCWSTR(path_hstring.as_ptr()), &file_iid)?;
            Ok(DStorageFile { file })
        }
    }

    pub fn create_vram_buffer(&self, size: u64) -> GpckResult<ID3D12Resource> {
        let device = self
            .device
            .as_ref()
            .ok_or(GpckError::DirectStorageUnsupported)?;
        unsafe {
            let heap_props = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                ..Default::default()
            };

            let res_desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: size,
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
                Alignment: 0,
            };

            let mut resource: Option<ID3D12Resource> = None;
            device
                .CreateCommittedResource(
                    &heap_props,
                    D3D12_HEAP_FLAG_NONE,
                    &res_desc,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut resource,
                )
                .map_err(|e| GpckError::DirectStorageError {
                    hresult: e.code().0 as u32,
                    message: "CreateCommittedResource for VRAM failed",
                })?;

            resource.ok_or(GpckError::DirectStorageError {
                hresult: 0x80004005,
                message: "Failed to allocate VRAM ID3D12Resource",
            })
        }
    }

    pub fn create_tiled_vram_texture2d(
        &self,
        width: u32,
        height: u32,
        mip_levels: u16,
        format: DXGI_FORMAT,
    ) -> GpckResult<ID3D12Resource> {
        let device = self
            .device
            .as_ref()
            .ok_or(GpckError::DirectStorageUnsupported)?;

        let res_desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Alignment: 0,
            Width: width as u64,
            Height: height,
            DepthOrArraySize: 1,
            MipLevels: mip_levels,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_64KB_UNDEFINED_SWIZZLE,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        };

        let mut resource: Option<ID3D12Resource> = None;
        unsafe {
            device
                .CreateReservedResource(&res_desc, D3D12_RESOURCE_STATE_COMMON, None, &mut resource)
                .map_err(|e| GpckError::DirectStorageError {
                    hresult: e.code().0 as u32,
                    message: "CreateReservedResource for Tiled Texture2D failed",
                })?;
        }

        resource.ok_or(GpckError::DirectStorageError {
            hresult: 0x80004005,
            message: "Failed to allocate Tiled ID3D12Resource",
        })
    }

    pub fn create_tile_pool_heap(&self, size_bytes: u64) -> GpckResult<ID3D12Heap> {
        let device = self
            .device
            .as_ref()
            .ok_or(GpckError::DirectStorageUnsupported)?;

        let heap_desc = D3D12_HEAP_DESC {
            SizeInBytes: size_bytes,
            Properties: D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
                MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
                CreationNodeMask: 1,
                VisibleNodeMask: 1,
            },
            Alignment: D3D12_DEFAULT_RESOURCE_PLACEMENT_ALIGNMENT as u64,
            Flags: D3D12_HEAP_FLAG_ALLOW_ONLY_NON_RT_DS_TEXTURES,
        };

        let mut heap: Option<ID3D12Heap> = None;
        unsafe {
            device.CreateHeap(&heap_desc, &mut heap).map_err(|e| {
                GpckError::DirectStorageError {
                    hresult: e.code().0 as u32,
                    message: "CreateHeap for Tile Pool failed",
                }
            })?;
        }

        heap.ok_or(GpckError::DirectStorageError {
            hresult: 0x80004005,
            message: "Failed to allocate Tile Pool ID3D12Heap",
        })
    }

    pub fn calculate_subresource_row_pitch(width_pixels: u32, block_size_bytes: u32) -> u32 {
        let width_in_blocks = width_pixels.div_ceil(4);
        let unaligned_pitch = width_in_blocks * block_size_bytes;
        (unaligned_pitch + 255) & !255
    }

    pub fn enqueue_buffer_request(&self, priority: QueuePriority, request: &DSTORAGE_REQUEST) {
        if self.is_supported
            && let Some(Some(queue)) = self.file_queues.get(priority as usize)
        {
            unsafe {
                queue.enqueue_request(request);
            }
        }
    }

    pub fn enqueue_tile_request(&self, priority: QueuePriority, request: &DSTORAGE_REQUEST) {
        self.enqueue_buffer_request(priority, request);
    }

    pub fn cancel_requests_with_tag(&self, priority: QueuePriority, mask: u64, value: u64) {
        if self.is_supported
            && let Some(Some(queue)) = self.file_queues.get(priority as usize)
        {
            unsafe {
                queue.cancel_requests_with_tag(mask, value);
            }
        }
    }

    pub fn flush_and_signal(&self, priority: QueuePriority) -> GpckResult<u64> {
        if !self.is_supported {
            return Err(GpckError::DirectStorageUnsupported);
        }

        let q_idx = priority as usize;
        let fence = self.fences[q_idx]
            .as_ref()
            .ok_or(GpckError::DirectStorageUnsupported)?;
        let queue = self.file_queues[q_idx]
            .as_ref()
            .ok_or(GpckError::DirectStorageUnsupported)?;

        let val = self.fence_values[q_idx].fetch_add(1, Ordering::SeqCst) + 1;
        unsafe {
            queue.enqueue_signal(Interface::as_raw(fence), val);
            queue.submit();
        }
        Ok(val)
    }

    pub fn wait_for_fence(&self, priority: QueuePriority, fence_val: u64) -> GpckResult<()> {
        self.wait_for_fence_timeout(priority, fence_val, 10_000)
    }

    pub fn wait_for_fence_timeout(
        &self,
        priority: QueuePriority,
        fence_val: u64,
        timeout_ms: u32,
    ) -> GpckResult<()> {
        let q_idx = priority as usize;
        let fence = self.fences[q_idx]
            .as_ref()
            .ok_or(GpckError::DirectStorageUnsupported)?;
        let event = self.fence_events[q_idx];

        unsafe {
            if fence.GetCompletedValue() < fence_val {
                fence.SetEventOnCompletion(fence_val, event).map_err(|e| {
                    GpckError::DirectStorageError {
                        hresult: e.code().0 as u32,
                        message: "SetEventOnCompletion failed",
                    }
                })?;

                let wait_res = WaitForSingleObject(event, timeout_ms);

                if wait_res == WAIT_TIMEOUT {
                    if let Some(Some(queue)) = self.file_queues.get(q_idx) {
                        let mut error_record: DSTORAGE_ERROR_RECORD = std::mem::zeroed();
                        queue.retrieve_error_record(&mut error_record);
                        if error_record.FailureCount > 0 {
                            let hr = error_record.FirstFailure.HResult;
                            let msg = parse_dstorage_hresult(hr);
                            crate::core::logger::log_error(&format!(
                                "[DirectStorage Error] Queue {:?} failed with HRESULT 0x{:08X}: {}",
                                priority, hr.0 as u32, msg
                            ));
                        }
                    }

                    if let Some(ref dev) = self.device
                        && let Some(report) =
                            crate::gpu::dred::DredDiagnosticEngine::analyze_device_removal(dev)
                    {
                        crate::core::logger::log_error(&format!(
                            "[DRED Device Removed Report]\n{}",
                            report
                        ));
                    }

                    return Err(GpckError::DirectStorageError {
                        hresult: E_DSTORAGE_IO_TIMEOUT,
                        message: "DirectStorage operation timed out waiting for GPU fence completion",
                    });
                }
            }
        }
        Ok(())
    }

    pub fn decompress_batch_gpu(
        &self,
        compressed_data: &[u8],
        uncompressed_size: usize,
    ) -> GpckResult<ID3D12Resource> {
        let vram_resource = self.create_vram_buffer(uncompressed_size as u64)?;
        let mut request: DSTORAGE_REQUEST = unsafe { std::mem::zeroed() };
        request.set_memory_to_buffer(
            compressed_data.as_ptr() as *const c_void,
            compressed_data.len() as u32,
            Interface::as_raw(&vram_resource),
            0,
            uncompressed_size as u32,
            DSTORAGE_COMPRESSION_FORMAT_GDEFLATE,
            GACL_TRANSFORM_NONE,
        );

        self.enqueue_memory_request(&request)?;
        Ok(vram_resource)
    }

    pub fn decompress_batch_gpu_zstd(
        &self,
        compressed_data: &[u8],
        uncompressed_size: usize,
        gacl_transform: u8,
    ) -> GpckResult<ID3D12Resource> {
        let vram_resource = self.create_vram_buffer(uncompressed_size as u64)?;
        let device = self
            .device
            .as_ref()
            .ok_or(GpckError::DirectStorageUnsupported)?;
        let device_ptr = Interface::as_raw(device);
        let dest_ptr = Interface::as_raw(&vram_resource);

        unsafe {
            let zstd_ctx = ZstdGpu_CreateContext(device_ptr);
            if !zstd_ctx.is_null() {
                let success = ZstdGpu_Decompress(
                    zstd_ctx,
                    compressed_data.as_ptr(),
                    compressed_data.len() as u32,
                    dest_ptr,
                    0,
                    uncompressed_size as u32,
                );

                ZstdGpu_DestroyContext(zstd_ctx);

                if success {
                    return Ok(vram_resource);
                }
            }

            let mut request: DSTORAGE_REQUEST = std::mem::zeroed();
            request.set_memory_to_buffer(
                compressed_data.as_ptr() as *const c_void,
                compressed_data.len() as u32,
                dest_ptr,
                0,
                uncompressed_size as u32,
                DSTORAGE_COMPRESSION_FORMAT_ZSTD,
                gacl_transform,
            );

            self.enqueue_memory_request(&request)?;
        }

        Ok(vram_resource)
    }

    pub fn decompress_batch_gpu_brotlig(
        &self,
        compressed_data: &[u8],
        uncompressed_size: usize,
    ) -> GpckResult<ID3D12Resource> {
        let vram_resource = self.create_vram_buffer(uncompressed_size as u64)?;
        let mut request: DSTORAGE_REQUEST = unsafe { std::mem::zeroed() };
        request.set_memory_to_buffer(
            compressed_data.as_ptr() as *const c_void,
            compressed_data.len() as u32,
            Interface::as_raw(&vram_resource),
            0,
            uncompressed_size as u32,
            DSTORAGE_CUSTOM_COMPRESSION_0,
            GACL_TRANSFORM_NONE,
        );

        self.enqueue_memory_request(&request)?;
        Ok(vram_resource)
    }

    pub fn decompress_tile_gpu(
        &self,
        compressed_tile: &[u8],
        dest_tiled_texture: &ID3D12Resource,
        coord: D3D12_TILED_RESOURCE_COORDINATE,
        tile_region: D3D12_TILE_REGION_SIZE,
        compression_format: u8,
        gacl_transform: u8,
    ) -> GpckResult<()> {
        let mut request: DSTORAGE_REQUEST = unsafe { std::mem::zeroed() };
        request.set_memory_to_tiles(
            compressed_tile.as_ptr() as *const c_void,
            compressed_tile.len() as u32,
            Interface::as_raw(dest_tiled_texture),
            coord,
            tile_region,
            D3D12_TILED_RESOURCE_TILE_SIZE_IN_BYTES,
            compression_format,
            gacl_transform,
        );

        self.enqueue_memory_request(&request)
    }

    fn enqueue_memory_request(&self, request: &DSTORAGE_REQUEST) -> GpckResult<()> {
        let memory_queue = self
            .memory_queue
            .as_ref()
            .ok_or(GpckError::DirectStorageUnsupported)?;
        let fence = self.fences[3]
            .as_ref()
            .ok_or(GpckError::DirectStorageUnsupported)?;
        let event = self.fence_events[3];

        unsafe {
            memory_queue.enqueue_request(request);
            let val = self.fence_values[3].fetch_add(1, Ordering::SeqCst) + 1;
            let fence_ptr = Interface::as_raw(fence);
            memory_queue.enqueue_signal(fence_ptr, val);
            memory_queue.submit();

            if fence.GetCompletedValue() < val {
                let _ = fence.SetEventOnCompletion(val, event);
                let wait_res = WaitForSingleObject(event, 10_000);
                if wait_res == WAIT_TIMEOUT {
                    if let Some(ref dev) = self.device
                        && let Some(report) =
                            crate::gpu::dred::DredDiagnosticEngine::analyze_device_removal(dev)
                    {
                        crate::core::logger::log_error(&format!(
                            "[DRED Device Removed Report]\n{}",
                            report
                        ));
                    }
                    return Err(GpckError::DirectStorageError {
                        hresult: E_DSTORAGE_IO_TIMEOUT,
                        message: "Memory queue timed out waiting for GPU fence",
                    });
                }
            }

            let mut error_record: DSTORAGE_ERROR_RECORD = std::mem::zeroed();
            memory_queue.retrieve_error_record(&mut error_record);
            if error_record.FailureCount > 0 {
                let hr = error_record.FirstFailure.HResult;
                let err_msg = parse_dstorage_hresult(hr);
                return Err(GpckError::DirectStorageError {
                    hresult: hr.0 as u32,
                    message: err_msg,
                });
            }
        }
        Ok(())
    }
}

impl GpuStreamingBackend for GpuDirectStorage {
    fn name(&self) -> &str {
        "DirectStorage 1.4 Native (D3D12 NVMe BypassIO -> VRAM)"
    }

    fn is_hardware_accelerated(&self) -> bool {
        self.is_supported
    }

    fn decompress(
        &self,
        _compressed: &[u8],
        _target_size: usize,
        _method: CompressionMethod,
    ) -> GpckResult<Vec<u8>> {
        Err(GpckError::DirectStorageError {
            hresult: 0x80004001,
            message: "DirectStorage streams directly from NVMe to VRAM.",
        })
    }

    fn decompress_and_unshuffle(
        &self,
        _compressed: &[u8],
        _target_size: usize,
        _method: CompressionMethod,
        _transform: GaclTransform,
        _width_pixels: usize,
    ) -> GpckResult<Vec<u8>> {
        Err(GpckError::DirectStorageError {
            hresult: 0x80004001,
            message: "DirectStorage streams directly from NVMe to VRAM.",
        })
    }
}

impl Drop for GpuDirectStorage {
    fn drop(&mut self) {
        unsafe {
            self.custom_decomp_shutdown.store(true, Ordering::SeqCst);
            if let Some(event) = self.custom_decomp_shutdown_event {
                let _ = SetEvent(event);
            }

            if let Some(handle) = self.custom_decomp_thread.take() {
                let _ = handle.join();
            }

            if let Some(event) = self.custom_decomp_shutdown_event.take()
                && !event.is_invalid()
            {
                let _ = CloseHandle(event);
            }

            for i in 0..3 {
                if let Some(queue) = self.file_queues[i].take() {
                    queue.close();
                }
            }
            if let Some(queue) = self.memory_queue.take() {
                queue.close();
            }

            for i in 0..4 {
                self.fences[i].take();
                if !self.fence_events[i].is_invalid() {
                    let _ = CloseHandle(self.fence_events[i]);
                }
            }

            self.custom_decomp_queue.take();
            self.factory.take();

            if let Some(cookie) = self.debug_cookie.take()
                && let Some(device) = self.device.as_ref()
            {
                crate::gpu::debug_layer::detach_d3d12_debug_callback(device, cookie);
            }
            self.device.take();
        }
    }
}
