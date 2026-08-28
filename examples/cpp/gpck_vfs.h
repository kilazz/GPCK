// examples/cpp/gpck_vfs.h
/**
 * @file gpck_vfs.h
 * @brief GPCK High-Performance Asset Packaging & GPU DirectStorage VFS C-API.
 *
 * Provides sub-microsecond zero-copy asset retrieval, multithreaded decompression,
 * and DirectStorage 1.4 direct-to-VRAM GPU streaming (Linear Buffers, Swizzled Textures,
 * and 64KB Sparse Hardware Tiled Resources).
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#ifndef GPCK_VFS_H
#define GPCK_VFS_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// ============================================================================
// Status & Error Return Codes
// ============================================================================

#define GPCK_OK                            0
#define GPCK_ERR_NULL_PTR                  -1
#define GPCK_ERR_INVALID_PATH              -2
#define GPCK_ERR_NOT_FOUND                 -3
#define GPCK_ERR_BUFFER_TOO_SMALL          -4
#define GPCK_ERR_DECRYPTION_FAILED         -5
#define GPCK_ERR_IO_FAILED                 -6
#define GPCK_ERR_NOT_UNCOMPRESSED          -7
#define GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED -8

// ============================================================================
// Compression Codecs
// ============================================================================

#define GPCK_COMPRESSION_STORE    1
#define GPCK_COMPRESSION_GDEFLATE 2
#define GPCK_COMPRESSION_ZSTD     3
#define GPCK_COMPRESSION_LZ4      4
#define GPCK_COMPRESSION_RANS     5
#define GPCK_COMPRESSION_BROTLIG  6

// ============================================================================
// DirectStorage Queue Priorities
// ============================================================================

#define GPCK_PRIORITY_LOW    0
#define GPCK_PRIORITY_NORMAL 1
#define GPCK_PRIORITY_HIGH   2

// ============================================================================
// Opaque Handles
// ============================================================================

typedef struct GpckArchive GpckArchive;
typedef struct GpckVfs GpckVfs;

// ============================================================================
// Archive Operations
// ============================================================================

/**
 * @brief Opens a GPCK Table of Contents (.gtoc) binary archive with an optional AES key.
 * @param path Null-terminated path to the .gtoc file.
 * @param key_passphrase Master passphrase for AES-256-GCM decryption, or NULL if unencrypted.
 * @param out_archive Output pointer to receive the archive handle.
 * @return GPCK_OK on success, or an error code.
 */
int32_t gpck_archive_open(const char* path, const char* key_passphrase, GpckArchive** out_archive);

/**
 * @brief Increments the reference count of the archive handle.
 */
int32_t gpck_archive_retain(GpckArchive* archive);

/**
 * @brief Decrements the reference count and frees memory when reference count drops to zero.
 */
int32_t gpck_archive_release(GpckArchive* archive);

/**
 * @brief Closes the archive handle safely (aliases gpck_archive_release).
 */
void gpck_archive_close(GpckArchive* archive);

/**
 * @brief Retrieves the total number of assets indexed in the Table of Contents (TOC).
 */
int32_t gpck_archive_get_entry_count(const GpckArchive* archive, uint32_t* out_count);

/**
 * @brief Direct Zero-Copy memory slice access (< 0.1 us).
 * Returns a direct pointer to the memory-mapped asset if stored uncompressed.
 * Valid as long as the archive handle is held open.
 */
int32_t gpck_archive_get_direct_asset_ptr(const GpckArchive* archive, const char* virtual_path,
                                          const uint8_t** out_data_ptr, size_t* out_size);

/**
 * @brief Reads and decompresses an asset by virtual path into a user-allocated buffer.
 * If out_buf is NULL, only out_written is updated with the required buffer size.
 */
int32_t gpck_archive_read_asset_by_path(GpckArchive* archive, const char* virtual_path, uint8_t* out_buf,
                                        size_t max_buf_len, size_t* out_written);

/**
 * @brief Reads and decompresses an asset by 128-bit UUID into a user-allocated buffer.
 */
int32_t gpck_archive_read_asset_by_uuid(GpckArchive* archive, const uint8_t uuid_bytes[16], uint8_t* out_buf,
                                        size_t max_buf_len, size_t* out_written);

// ============================================================================
// Virtual File System (VFS) Operations
// ============================================================================

/**
 * @brief Creates a new Virtual File System instance.
 */
int32_t gpck_vfs_create(GpckVfs** out_vfs);

/**
 * @brief Destroys a Virtual File System instance.
 */
void gpck_vfs_destroy(GpckVfs* vfs);

/**
 * @brief Mounts an archive file into the VFS search space.
 */
int32_t gpck_vfs_mount_archive(GpckVfs* vfs, const char* path);

/**
 * @brief Mounts a loose physical directory on disk into the VFS search space.
 */
int32_t gpck_vfs_mount_directory(GpckVfs* vfs, const char* path);

/**
 * @brief Reads a file through the VFS hierarchy by virtual path.
 */
int32_t gpck_vfs_read_file(GpckVfs* vfs, const char* virtual_path, uint8_t* out_buf, size_t max_buf_len,
                           size_t* out_written);

// ============================================================================
// DirectStorage 1.4 GPU Direct-to-VRAM Streaming
// ============================================================================

/**
 * @brief Returns 1 if DirectStorage 1.4 hardware offload (BypassIO) is supported on the host GPU.
 */
int32_t gpck_directstorage_is_supported(void);

/**
 * @brief Streams an asset directly from NVMe storage to a D3D12 Linear GPU Buffer (VRAM).
 */
int32_t gpck_vfs_stream_file_to_d3d12_buffer(GpckVfs* vfs, const char* virtual_path, void* d3d12_resource,
                                             uint64_t dest_offset, int32_t priority, uint64_t* out_fence_value);

/**
 * @brief Streams an asset directly into a swizzled D3D12 2D Texture Resource.
 */
int32_t gpck_vfs_stream_file_to_d3d12_texture(GpckVfs* vfs, const char* virtual_path, void* d3d12_texture,
                                              uint32_t first_subresource, int32_t priority, uint64_t* out_fence_value);

/**
 * @brief Streams a specific 64KB sparse hardware tile from NVMe storage directly to a D3D12 Reserved Tiled Resource.
 * @param vfs Pointer to active VFS instance.
 * @param virtual_path Virtual path of the tiled texture asset.
 * @param d3d12_tiled_texture Pointer to ID3D12Resource allocated with 64KB undefined swizzle.
 * @param subresource Mip level index.
 * @param tile_x Horizontal tile coordinate.
 * @param tile_y Vertical tile coordinate.
 * @param tile_z Depth tile coordinate (usually 0 for 2D textures).
 * @param priority Queue priority (GPCK_PRIORITY_LOW, GPCK_PRIORITY_NORMAL, GPCK_PRIORITY_HIGH).
 * @param out_fence_value Output pointer receiving the synchronization fence value.
 */
int32_t gpck_vfs_stream_tile_to_d3d12_texture(GpckVfs* vfs, const char* virtual_path, void* d3d12_tiled_texture,
                                              uint32_t subresource, uint32_t tile_x, uint32_t tile_y, uint32_t tile_z,
                                              int32_t priority, uint64_t* out_fence_value);

/**
 * @brief Blocks CPU execution until the specified DirectStorage hardware queue signals the fence.
 */
int32_t gpck_vfs_wait_for_d3d12_fence(int32_t priority, uint64_t fence_value);

#ifdef __cplusplus
}
#endif

#endif // GPCK_VFS_H
