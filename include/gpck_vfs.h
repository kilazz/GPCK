// include/gpck_vfs.h
#ifndef GPCK_VFS_H
#define GPCK_VFS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define GPCK_OK                            0
#define GPCK_ERR_NULL_PTR                  -1
#define GPCK_ERR_INVALID_PATH              -2
#define GPCK_ERR_NOT_FOUND                 -3
#define GPCK_ERR_BUFFER_TOO_SMALL          -4
#define GPCK_ERR_DECRYPTION_FAILED         -5
#define GPCK_ERR_IO_FAILED                 -6
#define GPCK_ERR_NOT_UNCOMPRESSED          -7
#define GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED -8

typedef struct GpckArchive GpckArchive;
typedef struct GpckVfs GpckVfs;
typedef struct GpckAssetSlice GpckAssetSlice;

// Archive Lifecycle
int32_t gpck_archive_open(const char* path, const char* key_passphrase, GpckArchive** out_archive);
GpckArchive* gpck_archive_retain(GpckArchive* archive);
int32_t gpck_archive_release(GpckArchive* archive);
void gpck_archive_close(GpckArchive* archive);
int32_t gpck_archive_get_entry_count(const GpckArchive* archive, uint32_t* out_count);

// Zero-Copy Slice (< 0.1 us)
int32_t gpck_archive_acquire_asset_slice(const GpckArchive* archive, const char* virtual_path,
                                         GpckAssetSlice** out_slice);
int32_t gpck_asset_slice_get_data(const GpckAssetSlice* slice, const uint8_t** out_data_ptr, size_t* out_size);
void gpck_asset_slice_release(GpckAssetSlice* slice);

// Zero-Allocation Linear Arena Decompression
int32_t gpck_archive_read_asset_to_buffer(GpckArchive* archive, const char* virtual_path, uint8_t* out_buf,
                                          size_t max_buf_len, size_t* out_written);
int32_t gpck_archive_read_asset_by_path(GpckArchive* archive, const char* virtual_path, uint8_t* out_buf,
                                        size_t max_buf_len, size_t* out_written);
int32_t gpck_archive_read_asset_by_uuid(GpckArchive* archive, const uint8_t* uuid_bytes, uint8_t* out_buf,
                                        size_t max_buf_len, size_t* out_written);

// VFS Multi-Layer Operations
int32_t gpck_vfs_create(GpckVfs** out_vfs);
void gpck_vfs_destroy(GpckVfs* vfs);
int32_t gpck_vfs_mount_archive(GpckVfs* vfs, const char* path);
int32_t gpck_vfs_mount_directory(GpckVfs* vfs, const char* path);
int32_t gpck_vfs_read_file(GpckVfs* vfs, const char* virtual_path, uint8_t* out_buf, size_t max_buf_len,
                           size_t* out_written);

// DirectStorage 1.4 GPU & Sparse Virtual Texturing
int32_t gpck_directstorage_is_supported(void);
int32_t gpck_vfs_stream_file_to_d3d12_buffer(GpckVfs* vfs, const char* virtual_path, void* d3d12_resource,
                                             uint64_t dest_offset, int32_t priority, uint64_t* out_fence_value);
int32_t gpck_vfs_stream_file_to_d3d12_texture(GpckVfs* vfs, const char* virtual_path, void* d3d12_texture,
                                              uint32_t first_subresource, int32_t priority, uint64_t* out_fence_value);
int32_t gpck_vfs_stream_tile_to_d3d12_texture(GpckVfs* vfs, const char* virtual_path, void* d3d12_tiled_texture,
                                              uint32_t subresource, uint32_t tile_x, uint32_t tile_y, uint32_t tile_z,
                                              int32_t priority, uint64_t* out_fence_value);
int32_t gpck_vfs_wait_for_d3d12_fence(int32_t priority, uint64_t fence_value);

// Camera Preemption & Sampler Feedback Bridge
int32_t gpck_vfs_cancel_requests_by_tag(GpckVfs* vfs, int32_t priority, uint64_t mask, uint64_t tag_value);
int32_t gpck_vfs_process_sampler_feedback_map(GpckVfs* vfs, const char* virtual_path, const uint8_t* feedback_data,
                                              uint32_t feedback_size, void* d3d12_texture_ptr, int32_t priority,
                                              uint64_t camera_tag, uint32_t* out_tiles_dispatched);

#ifdef __cplusplus
}
#endif

#endif // GPCK_VFS_H
