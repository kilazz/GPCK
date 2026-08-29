// include/gpck.hpp
#pragma once

#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "gpck_vfs.h"

namespace gpck {

inline bool is_directstorage_supported()
{
    return gpck_directstorage_is_supported() != 0;
}

class AssetSlice
{
public:
    explicit AssetSlice(GpckAssetSlice* slice) : m_slice(slice) {}
    ~AssetSlice()
    {
        if (m_slice) {
            gpck_asset_slice_release(m_slice);
        }
    }
    AssetSlice(const AssetSlice&) = delete;
    AssetSlice& operator=(const AssetSlice&) = delete;
    AssetSlice(AssetSlice&& other) noexcept : m_slice(other.m_slice) { other.m_slice = nullptr; }

    const uint8_t* data() const
    {
        const uint8_t* ptr = nullptr;
        size_t sz = 0;
        if (m_slice && gpck_asset_slice_get_data(m_slice, &ptr, &sz) == GPCK_OK) {
            return ptr;
        }
        return nullptr;
    }

    size_t size() const
    {
        const uint8_t* ptr = nullptr;
        size_t sz = 0;
        if (m_slice && gpck_asset_slice_get_data(m_slice, &ptr, &sz) == GPCK_OK) {
            return sz;
        }
        return 0;
    }

private:
    GpckAssetSlice* m_slice = nullptr;
};

class Archive
{
public:
    static std::shared_ptr<Archive> open(const std::string& path, const std::string& passphrase = "")
    {
        GpckArchive* raw = nullptr;
        int32_t res = gpck_archive_open(path.c_str(), passphrase.empty() ? nullptr : passphrase.c_str(), &raw);
        if (res == GPCK_OK && raw) {
            return std::make_shared<Archive>(raw);
        }
        return nullptr;
    }

    explicit Archive(GpckArchive* raw) : m_archive(raw) {}
    ~Archive()
    {
        if (m_archive) {
            gpck_archive_release(m_archive);
        }
    }

    uint32_t entry_count() const
    {
        uint32_t count = 0;
        gpck_archive_get_entry_count(m_archive, &count);
        return count;
    }

    // Zero-Copy Slice (< 0.1 us)
    std::unique_ptr<AssetSlice> acquire_slice(const std::string& virtual_path) const
    {
        GpckAssetSlice* raw_slice = nullptr;
        if (gpck_archive_acquire_asset_slice(m_archive, virtual_path.c_str(), &raw_slice) == GPCK_OK && raw_slice) {
            return std::make_unique<AssetSlice>(raw_slice);
        }
        return nullptr;
    }

    // Zero-Allocation Linear Arena Read (directly into preallocated memory)
    int32_t read_asset_to_buffer(const std::string& virtual_path, uint8_t* dest_buf, size_t dest_capacity,
                                 size_t* out_written)
    {
        return gpck_archive_read_asset_to_buffer(m_archive, virtual_path.c_str(), dest_buf, dest_capacity, out_written);
    }

private:
    GpckArchive* m_archive = nullptr;
};

class Vfs
{
public:
    static std::unique_ptr<Vfs> create()
    {
        GpckVfs* raw = nullptr;
        if (gpck_vfs_create(&raw) == GPCK_OK && raw) {
            return std::make_unique<Vfs>(raw);
        }
        return nullptr;
    }

    explicit Vfs(GpckVfs* raw) : m_vfs(raw) {}
    ~Vfs()
    {
        if (m_vfs) {
            gpck_vfs_destroy(m_vfs);
        }
    }

    bool mount_archive(const std::string& path) { return gpck_vfs_mount_archive(m_vfs, path.c_str()) == GPCK_OK; }

    bool mount_directory(const std::string& path) { return gpck_vfs_mount_directory(m_vfs, path.c_str()) == GPCK_OK; }

    // Preemption: Cancel stale streaming requests upon fast camera turns
    void cancel_requests_by_tag(int32_t priority, uint64_t mask, uint64_t tag_value)
    {
        gpck_vfs_cancel_requests_by_tag(m_vfs, priority, mask, tag_value);
    }

    // Sampler Feedback Tile Dispatch Bridge
    uint32_t process_sampler_feedback(const std::string& virtual_path, const uint8_t* feedback_data,
                                      uint32_t feedback_size, void* d3d12_tex_ptr, int32_t priority,
                                      uint64_t camera_tag)
    {
        uint32_t dispatched = 0;
        gpck_vfs_process_sampler_feedback_map(m_vfs, virtual_path.c_str(), feedback_data, feedback_size, d3d12_tex_ptr,
                                              priority, camera_tag, &dispatched);
        return dispatched;
    }

    // DirectStorage Tile Stream
    bool stream_tile(const std::string& virtual_path, void* d3d12_tiled_texture, uint32_t mip, uint32_t tile_x,
                     uint32_t tile_y, uint32_t tile_z, int32_t priority, uint64_t* out_fence)
    {
        return gpck_vfs_stream_tile_to_d3d12_texture(m_vfs, virtual_path.c_str(), d3d12_tiled_texture, mip, tile_x,
                                                     tile_y, tile_z, priority, out_fence) == GPCK_OK;
    }

private:
    GpckVfs* m_vfs = nullptr;
};

} // namespace gpck
