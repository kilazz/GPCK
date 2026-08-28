// include/gpck_vfs.h
/**
 * @file gpck_vfs.h
 * @brief GPCK Native C-ABI Foreign Function Interface (FFI) Definitions.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#ifndef GPCK_VFS_H
#define GPCK_VFS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Status and Error Return Codes */
#define GPCK_OK                            0
#define GPCK_ERR_NULL_PTR                  -1
#define GPCK_ERR_INVALID_PATH              -2
#define GPCK_ERR_NOT_FOUND                 -3
#define GPCK_ERR_BUFFER_TOO_SMALL          -4
#define GPCK_ERR_DECRYPTION_FAILED         -5
#define GPCK_ERR_IO_FAILED                 -6
#define GPCK_ERR_NOT_UNCOMPRESSED          -7
#define GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED -8

/* Queue Priority Levels */
#define GPCK_PRIORITY_LOW    0
#define GPCK_PRIORITY_NORMAL 1
#define GPCK_PRIORITY_HIGH   2

/* Opaque Handle Types */
typedef struct GpckArchive GpckArchive;
typedef struct GpckVfs GpckVfs;
typedef struct GpckAssetSlice GpckAssetSlice;

/* ========================================================================= */
/* Archive Lifecycle & Queries                                               */
/* ========================================================================= */

/**
 * @brief Opens a GPCK Archive (.gtoc) and initializes a reference-counted handle.
 * @param path Null-terminated file path to the .gtoc archive.
 * @param key_passphrase Optional master decryption passphrase (or NULL).
 * @param out_archive Pointer to receive the allocated archive handle.
 * @return GPCK_OK on success, or an error code.
 */
int32_t gpck_archive_open(const char* path, const char* key_passphrase, GpckArchive** out_archive);

/**
 * @brief Increments the internal reference count of an archive handle.
 */
int32_t gpck_archive_retain(GpckArchive* archive);

/**
 * @brief Decrements the internal reference count and frees memory when it drops to zero.
 */
int32_t gpck_archive_release(GpckArchive* archive);

/**
 * @brief Closes the archive handle safely (alias to gpck_archive_release).
 */
void gpck_archive_close(GpckArchive* archive);

/**
 * @brief Retrieves the total number of entries in the archive Table of Contents (TOC).
 */
int32_t gpck_archive_get_entry_count(const GpckArchive* archive, uint32_t* out_count);

/**
 * @brief Acquires a safe RAII Zero-Copy Asset Slice handle.
 * @note Holds an internal reference count on the archive, guaranteeing that the memory-mapped
 *       region remains valid even if another thread closes the main archive handle.
 */
int32_t gpck_archive_acquire_asset_slice(const GpckArchive* archive, const char* virtual_path,
                                         GpckAssetSlice** out_slice);

/**
 * @brief Reads the direct pointer and size from an acquired asset slice handle.
 */
int32_t gpck_asset_slice_get_data(const GpckAssetSlice* slice, const uint8_t** out_data_ptr, size_t* out_size);

/**
 * @brief Releases an acquired asset slice handle.
 */
void gpck_asset_slice_release(GpckAssetSlice* slice);

/**
 * @brief Direct Zero-Copy memory slice access (< 0.1 us) to memory-mapped assets.
 * @warning The pointer is valid only while the parent archive handle is kept open.
 */
int32_t gpck_archive_get_direct_asset_ptr(const GpckArchive* archive, const char* virtual_path,
                                          const uint8_t** out_data_ptr, size_t* out_size);

/**
 * @brief Decompresses an asset by virtual path into a user-allocated buffer.
 */
int32_t gpck_archive_read_asset_by_path(GpckArchive* archive, const char* virtual_path, uint8_t* out_buf,
                                        size_t max_buf_len, size_t* out_written);

/**
 * @brief Decompresses an asset by 128-bit UUID into a user-allocated buffer.
 */
int32_t gpck_archive_read_asset_by_uuid(GpckArchive* archive, const uint8_t* uuid_bytes, uint8_t* out_buf,
                                        size_t max_buf_len, size_t* out_written);

/* ========================================================================= */
/* Virtual File System (VFS) Operations                                      */
/* ========================================================================= */

/**
 * @brief Creates a new Virtual File System instance.
 */
int32_t gpck_vfs_create(GpckVfs** out_vfs);

/**
 * @brief Destroys a Virtual File System instance.
 */
void gpck_vfs_destroy(GpckVfs* vfs);

/**
 * @brief Mounts an archive file (.gtoc) into the VFS search space.
 */
int32_t gpck_vfs_mount_archive(GpckVfs* vfs, const char* path);

/**
 * @brief Mounts a loose physical directory on disk into the VFS search space.
 */
int32_t gpck_vfs_mount_directory(GpckVfs* vfs, const char* path);

/**
 * @brief Reads an asset payload through the VFS by virtual path.
 */
int32_t gpck_vfs_read_file(GpckVfs* vfs, const char* virtual_path, uint8_t* out_buf, size_t max_buf_len,
                           size_t* out_written);

/* ========================================================================= */
/* DirectStorage 1.4 GPU Direct-to-VRAM Streaming                            */
/* ========================================================================= */

/**
 * @brief Returns 1 if DirectStorage 1.4 hardware offload is supported on the host, otherwise 0.
 */
int32_t gpck_directstorage_is_supported(void);

/**
 * @brief Streams an asset directly from NVMe to a D3D12 GPU Buffer (ID3D12Resource*).
 */
int32_t gpck_vfs_stream_file_to_d3d12_buffer(GpckVfs* vfs, const char* virtual_path, void* d3d12_resource,
                                             uint64_t dest_offset, int32_t priority, uint64_t* out_fence_value);

/**
 * @brief Streams an asset directly from NVMe to a D3D12 Texture2D Resource.
 */
int32_t gpck_vfs_stream_file_to_d3d12_texture(GpckVfs* vfs, const char* virtual_path, void* d3d12_texture,
                                              uint32_t first_subresource, int32_t priority, uint64_t* out_fence_value);

/**
 * @brief Streams a 64KB sparse tile directly from NVMe into a D3D12 Reserved Tiled Resource.
 */
int32_t gpck_vfs_stream_tile_to_d3d12_texture(GpckVfs* vfs, const char* virtual_path, void* d3d12_tiled_texture,
                                              uint32_t subresource, uint32_t tile_x, uint32_t tile_y, uint32_t tile_z,
                                              int32_t priority, uint64_t* out_fence_value);

/**
 * @brief Waits on the CPU until the specified DirectStorage queue fence is signaled.
 */
int32_t gpck_vfs_wait_for_d3d12_fence(int32_t priority, uint64_t fence_value);

#ifdef __cplusplus
}
#endif

#endif /* GPCK_VFS_H */
