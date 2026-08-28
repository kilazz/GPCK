// crates/gpck_core/src/gpu/directstorage_sys.rs
//! # Microsoft DirectStorage 1.4 Raw FFI Bindings
//!
//! 1:1 bit-exact binary layout matching official Microsoft `dstorage.h` and `dstorageerr.h`.

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use std::ffi::c_void;

#[cfg(windows)]
use windows::core::{GUID, HRESULT, PCWSTR};

#[cfg(windows)]
pub use windows::Win32::Graphics::Direct3D12::{
    D3D12_TILE_REGION_SIZE, D3D12_TILED_RESOURCE_COORDINATE,
    D3D12_TILED_RESOURCE_TILE_SIZE_IN_BYTES,
};

#[cfg(not(windows))]
pub type GUID = [u8; 16];
#[cfg(not(windows))]
pub type HRESULT = i32;
#[cfg(not(windows))]
pub type PCWSTR = *const u16;

#[cfg(not(windows))]
pub const D3D12_TILED_RESOURCE_TILE_SIZE_IN_BYTES: u32 = 65536;

#[cfg(not(windows))]
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct D3D12_TILED_RESOURCE_COORDINATE {
    pub X: u32,
    pub Y: u32,
    pub Z: u32,
    pub Subresource: u32,
}

#[cfg(not(windows))]
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct D3D12_TILE_REGION_SIZE {
    pub NumTiles: u32,
    pub UseBox: i32,
    pub Width: u32,
    pub Height: u16,
    pub Depth: u16,
}

// ============================================================================
// DirectStorage SDK Constants & Enums
// ============================================================================

pub const DSTORAGE_SDK_VERSION: u32 = 400;

pub const DSTORAGE_MIN_QUEUE_CAPACITY: u16 = 0x80;
pub const DSTORAGE_MAX_QUEUE_CAPACITY: u16 = 0x2000;
pub const DSTORAGE_REQUEST_MAX_NAME: usize = 64;

// DirectStorage Priority Constants (INT8 matching C++ enum DSTORAGE_PRIORITY)
pub const DSTORAGE_PRIORITY_LOW: i8 = -1;
pub const DSTORAGE_PRIORITY_NORMAL: i8 = 0;
pub const DSTORAGE_PRIORITY_HIGH: i8 = 1;
pub const DSTORAGE_PRIORITY_REALTIME: i8 = 2;

#[repr(i8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DSTORAGE_PRIORITY {
    LOW = DSTORAGE_PRIORITY_LOW,
    NORMAL = DSTORAGE_PRIORITY_NORMAL,
    HIGH = DSTORAGE_PRIORITY_HIGH,
    REALTIME = DSTORAGE_PRIORITY_REALTIME,
}

// Built-in & Custom Compression Formats
pub const DSTORAGE_COMPRESSION_FORMAT_NONE: u8 = 0;
pub const DSTORAGE_COMPRESSION_FORMAT_GDEFLATE: u8 = 1;
pub const DSTORAGE_COMPRESSION_FORMAT_ZSTD: u8 = 2;
pub const DSTORAGE_CUSTOM_COMPRESSION_0: u8 = 0x80; // AMD Brotli-G Custom Codec Tag

// Custom Decompression Flags
pub const DSTORAGE_CUSTOM_DECOMPRESSION_FLAG_NONE: u32 = 0;
pub const DSTORAGE_CUSTOM_DECOMPRESSION_FLAG_DEST_UNALLOCATED: u32 = 0x01;

// Get Requests Flags (for IDStorageCustomDecompressionQueue1::GetRequests1)
pub const DSTORAGE_GET_REQUEST_FLAG_SELECT_CUSTOM: u32 = 0x01;
pub const DSTORAGE_GET_REQUEST_FLAG_SELECT_BUILTIN: u32 = 0x02;
pub const DSTORAGE_GET_REQUEST_FLAG_SELECT_ALL: u32 = 0x03;

// GACL De-shuffle Transform Types
pub const DSTORAGE_GACL_SHUFFLE_TRANSFORM_NONE: u8 = 0;
pub const DSTORAGE_GACL_SHUFFLE_TRANSFORM_BC1: u8 = 1;
pub const DSTORAGE_GACL_SHUFFLE_TRANSFORM_BC3: u8 = 2;
pub const DSTORAGE_GACL_SHUFFLE_TRANSFORM_BC4: u8 = 3;
pub const DSTORAGE_GACL_SHUFFLE_TRANSFORM_BC5: u8 = 4;

// Request Source Types (UINT64 in dstorage.h)
pub const DSTORAGE_REQUEST_SOURCE_FILE: u64 = 0;
pub const DSTORAGE_REQUEST_SOURCE_MEMORY: u64 = 1;

// Request Destination Types (UINT64 in dstorage.h)
pub const DSTORAGE_REQUEST_DESTINATION_MEMORY: u64 = 0;
pub const DSTORAGE_REQUEST_DESTINATION_BUFFER: u64 = 1;
pub const DSTORAGE_REQUEST_DESTINATION_TEXTURE_REGION: u64 = 2;
pub const DSTORAGE_REQUEST_DESTINATION_MULTIPLE_SUBRESOURCES: u64 = 3;
pub const DSTORAGE_REQUEST_DESTINATION_TILES: u64 = 4;
pub const DSTORAGE_REQUEST_DESTINATION_MULTIPLE_SUBRESOURCES_RANGE: u64 = 5;

// ============================================================================
// Structure Definitions (Bit-exact 1:1 with Microsoft C++ dstorage.h)
// ============================================================================

/// Exact 32-byte layout of `DSTORAGE_QUEUE_DESC`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_QUEUE_DESC {
    pub SourceType: u64, // DSTORAGE_REQUEST_SOURCE_TYPE is UINT64 (8 bytes, offset 0..8)
    pub Capacity: u16,   // UINT16 (2 bytes, offset 8..10)
    pub Priority: i8,    // DSTORAGE_PRIORITY (1 byte, offset 10..11)
    pub _pad: [u8; 5],   // Compiler alignment padding (5 bytes, offset 11..16)
    pub Name: *const u8, // const CHAR* (8 bytes, offset 16..24)
    pub Device: *mut c_void, // ID3D12Device* (8 bytes, offset 24..32)
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_CONFIGURATION2 {
    pub NumSubmitThreads: u32,
    pub NumBuiltInCpuDecompressionThreads: i32,
    pub ForceMappingLayer: i32,
    pub DisableBypassIO: i32,
    pub DisableTelemetry: i32,
    pub DisableGpuDecompressionMetacommand: i32,
    pub DisableGpuDecompression: i32,
    pub ForceFileBuffering: i32,
    pub CreatorID: GUID,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_SOURCE_FILE {
    pub Source: *mut c_void, // IDStorageFile* (8 bytes)
    pub Offset: u64,         // 8 bytes
    pub Size: u32,           // 4 bytes
    pub _pad: u32,           // 4 bytes
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_SOURCE_MEMORY {
    pub Source: *const c_void, // 8 bytes
    pub Size: u32,             // 4 bytes
    pub _pad: u32,             // 4 bytes
    pub _pad2: u64,            // 8 bytes
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union DSTORAGE_SOURCE {
    pub Memory: DSTORAGE_SOURCE_MEMORY,
    pub File: DSTORAGE_SOURCE_FILE,
    pub _raw: [u8; 24],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_DESTINATION_MEMORY {
    pub Buffer: *mut c_void,
    pub Size: u32,
    pub _pad: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_DESTINATION_BUFFER {
    pub Resource: *mut c_void,
    pub Offset: u64,
    pub Size: u32,
    pub _pad: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_DESTINATION_TEXTURE_REGION {
    pub Resource: *mut c_void,
    pub SubresourceIndex: u32,
    pub Region: [u32; 6], // D3D12_BOX (24 bytes)
    pub _pad: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_DESTINATION_MULTIPLE_SUBRESOURCES {
    pub Resource: *mut c_void,
    pub FirstSubresource: u32,
    pub _pad: u32,
}

/// 40-byte DSTORAGE_DESTINATION_TILES (Matches dstorage.h)
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_DESTINATION_TILES {
    pub Resource: *mut c_void, // ID3D12Resource* (Tiled Texture)
    pub TiledRegionStartCoordinate: D3D12_TILED_RESOURCE_COORDINATE,
    pub TileRegionSize: D3D12_TILE_REGION_SIZE,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union DSTORAGE_DESTINATION {
    pub Memory: DSTORAGE_DESTINATION_MEMORY,
    pub Buffer: DSTORAGE_DESTINATION_BUFFER,
    pub Texture: DSTORAGE_DESTINATION_TEXTURE_REGION,
    pub MultipleSubresources: DSTORAGE_DESTINATION_MULTIPLE_SUBRESOURCES,
    pub Tiles: DSTORAGE_DESTINATION_TILES,
    pub _raw_padding: [u8; 40],
}

/// Exact 16-byte layout of `DSTORAGE_REQUEST_OPTIONS` matching Microsoft SDK bitfields.
#[repr(C)]
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct DSTORAGE_REQUEST_OPTIONS {
    pub word0: u64, // CompressionFormat (8) | GaclTransformType (8) | Reserved1[6] (48)
    pub word1: u64, // SourceType (1) | DestinationType (7) | Reserved (56)
}

impl DSTORAGE_REQUEST_OPTIONS {
    #[inline(always)]
    pub fn new(
        compression_format: u8,
        gacl_transform: u8,
        source_type: u64,
        destination_type: u64,
    ) -> Self {
        let w0 = (compression_format as u64) | ((gacl_transform as u64) << 8);
        let w1 = (source_type & 0x01) | ((destination_type & 0x7F) << 1);
        Self {
            word0: w0,
            word1: w1,
        }
    }
}

/// Exact 104-byte DirectStorage Request structure matching Microsoft `dstorage.h`.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_REQUEST {
    pub Options: DSTORAGE_REQUEST_OPTIONS, // 16 bytes (offset 0..16)
    pub Source: DSTORAGE_SOURCE,           // 24 bytes (offset 16..40)
    pub Destination: DSTORAGE_DESTINATION, // 40 bytes (offset 40..80)
    pub UncompressedSize: u32,             // 4 bytes  (offset 80..84)
    pub _pad: u32,                         // 4 bytes  (offset 84..88)
    pub CancellationTag: u64,              // 8 bytes  (offset 88..96)
    pub Name: *const u8,                   // 8 bytes  (offset 96..104)
}

impl DSTORAGE_REQUEST {
    #[allow(clippy::too_many_arguments)]
    pub fn set_file_to_buffer(
        &mut self,
        file: *mut c_void,
        file_offset: u64,
        compressed_size: u32,
        dest_resource: *mut c_void,
        dest_offset: u64,
        uncompressed_size: u32,
        compression_format: u8,
        gacl_transform: u8,
    ) {
        self.Options = DSTORAGE_REQUEST_OPTIONS::new(
            compression_format,
            gacl_transform,
            DSTORAGE_REQUEST_SOURCE_FILE,
            DSTORAGE_REQUEST_DESTINATION_BUFFER,
        );

        self.Source.File = DSTORAGE_SOURCE_FILE {
            Source: file,
            Offset: file_offset,
            Size: compressed_size,
            _pad: 0,
        };

        self.Destination.Buffer = DSTORAGE_DESTINATION_BUFFER {
            Resource: dest_resource,
            Offset: dest_offset,
            Size: uncompressed_size,
            _pad: 0,
        };

        self.UncompressedSize = uncompressed_size;
        self._pad = 0;
        self.CancellationTag = 0;
        self.Name = std::ptr::null();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_file_to_texture(
        &mut self,
        file: *mut c_void,
        file_offset: u64,
        compressed_size: u32,
        dest_texture: *mut c_void,
        first_mip_level: u32,
        uncompressed_size: u32,
        compression_format: u8,
    ) {
        self.Options = DSTORAGE_REQUEST_OPTIONS::new(
            compression_format,
            DSTORAGE_GACL_SHUFFLE_TRANSFORM_NONE,
            DSTORAGE_REQUEST_SOURCE_FILE,
            DSTORAGE_REQUEST_DESTINATION_MULTIPLE_SUBRESOURCES,
        );

        self.Source.File = DSTORAGE_SOURCE_FILE {
            Source: file,
            Offset: file_offset,
            Size: compressed_size,
            _pad: 0,
        };

        self.Destination.MultipleSubresources = DSTORAGE_DESTINATION_MULTIPLE_SUBRESOURCES {
            Resource: dest_texture,
            FirstSubresource: first_mip_level,
            _pad: 0,
        };

        self.UncompressedSize = uncompressed_size;
        self._pad = 0;
        self.CancellationTag = 0;
        self.Name = std::ptr::null();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_file_to_tiles(
        &mut self,
        file: *mut c_void,
        file_offset: u64,
        compressed_size: u32,
        dest_resource: *mut c_void,
        tiled_region_start_coordinate: D3D12_TILED_RESOURCE_COORDINATE,
        tile_region_size: D3D12_TILE_REGION_SIZE,
        uncompressed_tile_size: u32,
        compression_format: u8,
        gacl_transform: u8,
    ) {
        self.Options = DSTORAGE_REQUEST_OPTIONS::new(
            compression_format,
            gacl_transform,
            DSTORAGE_REQUEST_SOURCE_FILE,
            DSTORAGE_REQUEST_DESTINATION_TILES,
        );

        self.Source.File = DSTORAGE_SOURCE_FILE {
            Source: file,
            Offset: file_offset,
            Size: compressed_size,
            _pad: 0,
        };

        self.Destination.Tiles = DSTORAGE_DESTINATION_TILES {
            Resource: dest_resource,
            TiledRegionStartCoordinate: tiled_region_start_coordinate,
            TileRegionSize: tile_region_size,
        };

        self.UncompressedSize = uncompressed_tile_size;
        self._pad = 0;
        self.CancellationTag = 0;
        self.Name = std::ptr::null();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_memory_to_buffer(
        &mut self,
        source_ptr: *const c_void,
        compressed_size: u32,
        dest_resource: *mut c_void,
        dest_offset: u64,
        uncompressed_size: u32,
        compression_format: u8,
        gacl_transform: u8,
    ) {
        self.Options = DSTORAGE_REQUEST_OPTIONS::new(
            compression_format,
            gacl_transform,
            DSTORAGE_REQUEST_SOURCE_MEMORY,
            DSTORAGE_REQUEST_DESTINATION_BUFFER,
        );

        self.Source.Memory = DSTORAGE_SOURCE_MEMORY {
            Source: source_ptr,
            Size: compressed_size,
            _pad: 0,
            _pad2: 0,
        };

        self.Destination.Buffer = DSTORAGE_DESTINATION_BUFFER {
            Resource: dest_resource,
            Offset: dest_offset,
            Size: uncompressed_size,
            _pad: 0,
        };

        self.UncompressedSize = uncompressed_size;
        self._pad = 0;
        self.CancellationTag = 0;
        self.Name = std::ptr::null();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_memory_to_tiles(
        &mut self,
        source_ptr: *const c_void,
        compressed_size: u32,
        dest_resource: *mut c_void,
        tiled_region_start_coordinate: D3D12_TILED_RESOURCE_COORDINATE,
        tile_region_size: D3D12_TILE_REGION_SIZE,
        uncompressed_tile_size: u32,
        compression_format: u8,
        gacl_transform: u8,
    ) {
        self.Options = DSTORAGE_REQUEST_OPTIONS::new(
            compression_format,
            gacl_transform,
            DSTORAGE_REQUEST_SOURCE_MEMORY,
            DSTORAGE_REQUEST_DESTINATION_TILES,
        );

        self.Source.Memory = DSTORAGE_SOURCE_MEMORY {
            Source: source_ptr,
            Size: compressed_size,
            _pad: 0,
            _pad2: 0,
        };

        self.Destination.Tiles = DSTORAGE_DESTINATION_TILES {
            Resource: dest_resource,
            TiledRegionStartCoordinate: tiled_region_start_coordinate,
            TileRegionSize: tile_region_size,
        };

        self.UncompressedSize = uncompressed_tile_size;
        self._pad = 0;
        self.CancellationTag = 0;
        self.Name = std::ptr::null();
    }
}

// ============================================================================
// DirectStorage Custom Decompression ABI (DirectStorage 1.2+)
// ============================================================================

/// 48-byte DSTORAGE_CUSTOM_DECOMPRESSION_REQUEST
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_CUSTOM_DECOMPRESSION_REQUEST {
    pub Id: u64,
    pub CompressionFormat: u8,
    pub Reserved: [u8; 3],
    pub Flags: u32,
    pub SrcSize: u64,
    pub SrcBuffer: *const c_void,
    pub DstSize: u64,
    pub DstBuffer: *mut c_void,
}

/// 16-byte DSTORAGE_CUSTOM_DECOMPRESSION_RESULT
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_CUSTOM_DECOMPRESSION_RESULT {
    pub Id: u64,
    pub Result: HRESULT,
    pub _pad: u32,
}

// ============================================================================
// DirectStorage Error Diagnosis & Diagnostic Structs
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_ERROR_PARAMETERS_REQUEST {
    pub Filename: [u16; 260],      // MAX_PATH wide chars (520 bytes)
    pub RequestName: [u8; 64],     // DSTORAGE_REQUEST_MAX_NAME (64 bytes)
    pub Request: DSTORAGE_REQUEST, // 104 bytes (520 + 64 + 104 = 688 bytes)
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_ERROR_PARAMETERS_STATUS {
    pub StatusArray: *mut c_void,
    pub Index: u32,
    pub _pad: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_ERROR_PARAMETERS_SIGNAL {
    pub Fence: *mut c_void,
    pub Value: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_ERROR_PARAMETERS_EVENT {
    pub Handle: *mut c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union DSTORAGE_ERROR_PARAMETERS {
    pub Request: DSTORAGE_ERROR_PARAMETERS_REQUEST,
    pub Status: DSTORAGE_ERROR_PARAMETERS_STATUS,
    pub Signal: DSTORAGE_ERROR_PARAMETERS_SIGNAL,
    pub Event: DSTORAGE_ERROR_PARAMETERS_EVENT,
    pub _raw_padding: [u8; 688],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_ERROR_FIRST_FAILURE {
    pub HResult: HRESULT,
    pub CommandType: i32,
    pub Params: DSTORAGE_ERROR_PARAMETERS, // 688 bytes
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DSTORAGE_ERROR_RECORD {
    pub FailureCount: u32,
    pub _pad: u32,
    pub FirstFailure: DSTORAGE_ERROR_FIRST_FAILURE,
}

// Compile-time static assertions ensuring byte-exact binary ABI compatibility with dstorage.h
const _: () = assert!(std::mem::size_of::<DSTORAGE_QUEUE_DESC>() == 32);
const _: () = assert!(std::mem::size_of::<DSTORAGE_REQUEST_OPTIONS>() == 16);
const _: () = assert!(std::mem::size_of::<DSTORAGE_SOURCE>() == 24);
const _: () = assert!(std::mem::size_of::<DSTORAGE_DESTINATION_TILES>() == 40);
const _: () = assert!(std::mem::size_of::<DSTORAGE_DESTINATION>() == 40);
const _: () = assert!(std::mem::size_of::<DSTORAGE_REQUEST>() == 104);
const _: () = assert!(std::mem::size_of::<DSTORAGE_CUSTOM_DECOMPRESSION_REQUEST>() == 48);
const _: () = assert!(std::mem::size_of::<DSTORAGE_CUSTOM_DECOMPRESSION_RESULT>() == 16);
const _: () = assert!(std::mem::size_of::<DSTORAGE_ERROR_PARAMETERS_REQUEST>() == 688);
const _: () = assert!(std::mem::size_of::<DSTORAGE_ERROR_RECORD>() == 704);

// ============================================================================
// DirectStorage Error Codes (from official dstorageerr.h)
// ============================================================================

pub const E_DSTORAGE_ALREADY_RUNNING: u32 = 0x89240001;
pub const E_DSTORAGE_NOT_RUNNING: u32 = 0x89240002;
pub const E_DSTORAGE_INVALID_QUEUE_CAPACITY: u32 = 0x89240003;
pub const E_DSTORAGE_XVD_DEVICE_NOT_SUPPORTED: u32 = 0x89240004;
pub const E_DSTORAGE_UNSUPPORTED_VOLUME: u32 = 0x89240005;
pub const E_DSTORAGE_END_OF_FILE: u32 = 0x89240007;
pub const E_DSTORAGE_REQUEST_TOO_LARGE: u32 = 0x89240008;
pub const E_DSTORAGE_ACCESS_VIOLATION: u32 = 0x89240009;
pub const E_DSTORAGE_UNSUPPORTED_FILE: u32 = 0x8924000A;
pub const E_DSTORAGE_FILE_NOT_OPEN: u32 = 0x8924000B;
pub const E_DSTORAGE_RESERVED_FIELDS: u32 = 0x8924000C;
pub const E_DSTORAGE_INVALID_BCPACK_MODE: u32 = 0x8924000D;
pub const E_DSTORAGE_INVALID_SWIZZLE_MODE: u32 = 0x8924000E;
pub const E_DSTORAGE_INVALID_DESTINATION_SIZE: u32 = 0x8924000F;
pub const E_DSTORAGE_QUEUE_CLOSED: u32 = 0x89240010;
pub const E_DSTORAGE_INVALID_CLUSTER_SIZE: u32 = 0x89240011;
pub const E_DSTORAGE_TOO_MANY_QUEUES: u32 = 0x89240012;
pub const E_DSTORAGE_INVALID_QUEUE_PRIORITY: u32 = 0x89240013;
pub const E_DSTORAGE_TOO_MANY_FILES: u32 = 0x89240014;
pub const E_DSTORAGE_INDEX_BOUND: u32 = 0x89240015;
pub const E_DSTORAGE_IO_TIMEOUT: u32 = 0x89240016;
pub const E_DSTORAGE_INVALID_FILE_HANDLE: u32 = 0x89240017;
pub const E_DSTORAGE_DEPRECATED_PREVIEW_GDK: u32 = 0x89240018;
pub const E_DSTORAGE_XVD_NOT_REGISTERED: u32 = 0x89240019;
pub const E_DSTORAGE_INVALID_FILE_OFFSET: u32 = 0x8924001A;
pub const E_DSTORAGE_INVALID_SOURCE_TYPE: u32 = 0x8924001B;
pub const E_DSTORAGE_INVALID_INTERMEDIATE_SIZE: u32 = 0x8924001C;
pub const E_DSTORAGE_SYSTEM_NOT_SUPPORTED: u32 = 0x8924001D;
pub const E_DSTORAGE_STAGING_BUFFER_LOCKED: u32 = 0x8924001F;
pub const E_DSTORAGE_INVALID_STAGING_BUFFER_SIZE: u32 = 0x89240020;
pub const E_DSTORAGE_STAGING_BUFFER_TOO_SMALL: u32 = 0x89240021;
pub const E_DSTORAGE_INVALID_FENCE: u32 = 0x89240022;
pub const E_DSTORAGE_INVALID_STATUS_ARRAY: u32 = 0x89240023;
pub const E_DSTORAGE_INVALID_MEMORY_QUEUE_PRIORITY: u32 = 0x89240024;
pub const E_DSTORAGE_DECOMPRESSION_ERROR: u32 = 0x89240030;
pub const E_DSTORAGE_ZLIB_BAD_HEADER: u32 = 0x89240031;
pub const E_DSTORAGE_ZLIB_BAD_DATA: u32 = 0x89240032;
pub const E_DSTORAGE_ZLIB_PARITY_FAIL: u32 = 0x89240033;
pub const E_DSTORAGE_BCPACK_BAD_HEADER: u32 = 0x89240034;
pub const E_DSTORAGE_BCPACK_BAD_DATA: u32 = 0x89240035;
pub const E_DSTORAGE_DECRYPTION_ERROR: u32 = 0x89240036;
pub const E_DSTORAGE_PASSTHROUGH_ERROR: u32 = 0x89240037;
pub const E_DSTORAGE_FILE_TOO_FRAGMENTED: u32 = 0x89240038;
pub const E_DSTORAGE_COMPRESSED_DATA_TOO_LARGE: u32 = 0x89240039;
pub const E_DSTORAGE_INVALID_DESTINATION_TYPE: u32 = 0x89240040;
pub const E_DSTORAGE_FILEBUFFERING_REQUIRES_DISABLED_BYPASSIO: u32 = 0x89240041;
pub const E_DSTORAGE_SCRATCH_BUFFER_TOO_SMALL: u32 = 0x89240042;
pub const E_DSTORAGE_INVALID_GACL_SHUFFLE_TRANSFORM_TYPE: u32 = 0x89240043;

// ============================================================================
// COM VTable Bindings
// ============================================================================

#[repr(C)]
pub struct IUnknown_Vtbl {
    pub QueryInterface: unsafe extern "system" fn(
        this: *mut c_void,
        riid: *const GUID,
        ppvObject: *mut *mut c_void,
    ) -> HRESULT,
    pub AddRef: unsafe extern "system" fn(this: *mut c_void) -> u32,
    pub Release: unsafe extern "system" fn(this: *mut c_void) -> u32,
}

#[repr(C)]
pub struct IDStorageFactoryVtbl {
    pub parent: IUnknown_Vtbl,
    pub CreateQueue: unsafe extern "system" fn(
        this: *mut c_void,
        desc: *const DSTORAGE_QUEUE_DESC,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT,
    pub OpenFile: unsafe extern "system" fn(
        this: *mut c_void,
        path: PCWSTR,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT,
    pub CreateStatusArray: unsafe extern "system" fn(
        this: *mut c_void,
        capacity: u32,
        name: *const u8,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT,
    pub SetDebugFlags: unsafe extern "system" fn(this: *mut c_void, flags: u32) -> HRESULT,
    pub SetStagingBufferSize: unsafe extern "system" fn(this: *mut c_void, size: u32) -> HRESULT,
}

#[repr(C)]
pub struct IDStorageQueueVtbl {
    pub parent: IUnknown_Vtbl,
    pub EnqueueRequest:
        unsafe extern "system" fn(this: *mut c_void, request: *const DSTORAGE_REQUEST),
    pub EnqueueStatus:
        unsafe extern "system" fn(this: *mut c_void, statusArray: *mut c_void, index: u32),
    pub EnqueueSignal: unsafe extern "system" fn(this: *mut c_void, fence: *mut c_void, value: u64),
    pub Submit: unsafe extern "system" fn(this: *mut c_void),
    pub CancelRequestsWithTag: unsafe extern "system" fn(this: *mut c_void, mask: u64, value: u64),
    pub Close: unsafe extern "system" fn(this: *mut c_void),
    pub GetErrorEvent: unsafe extern "system" fn(this: *mut c_void) -> *mut c_void,
    pub RetrieveErrorRecord:
        unsafe extern "system" fn(this: *mut c_void, record: *mut DSTORAGE_ERROR_RECORD),
    pub Query: unsafe extern "system" fn(this: *mut c_void, info: *mut c_void),
}

#[repr(C)]
pub struct IDStorageFileVtbl {
    pub parent: IUnknown_Vtbl,
    pub Close: unsafe extern "system" fn(this: *mut c_void),
    pub GetFileInformation:
        unsafe extern "system" fn(this: *mut c_void, info: *mut c_void) -> HRESULT,
}

#[repr(C)]
pub struct IDStorageCustomDecompressionQueue1Vtbl {
    pub parent: IUnknown_Vtbl,
    pub GetEvent: unsafe extern "system" fn(this: *mut c_void) -> *mut c_void,
    pub GetRequests: unsafe extern "system" fn(
        this: *mut c_void,
        max_requests: u32,
        requests: *mut DSTORAGE_CUSTOM_DECOMPRESSION_REQUEST,
        num_requests: *mut u32,
    ) -> HRESULT,
    pub SetRequestResults: unsafe extern "system" fn(
        this: *mut c_void,
        num_results: u32,
        results: *const DSTORAGE_CUSTOM_DECOMPRESSION_RESULT,
    ) -> HRESULT,
    pub GetRequests1: unsafe extern "system" fn(
        this: *mut c_void,
        flags: u32,
        max_requests: u32,
        requests: *mut DSTORAGE_CUSTOM_DECOMPRESSION_REQUEST,
        num_requests: *mut u32,
    ) -> HRESULT,
}
