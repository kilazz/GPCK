// include/gpck.hpp
/**
 * @file gpck.hpp
 * @brief Modern C++17/C++20 RAII Smart Pointers & Zero-Overhead SDK for GPCK.
 *
 * SPDX-License-Identifier: MIT OR Apache-2.0
 */

#pragma once

#include <memory>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "gpck_vfs.h"

namespace gpck {

/**
 * @brief Priority levels for DirectStorage GPU queues.
 */
enum class Priority : int32_t
{
    Low = GPCK_PRIORITY_LOW,
    Normal = GPCK_PRIORITY_NORMAL,
    High = GPCK_PRIORITY_HIGH
};

/**
 * @brief Converts GPCK C-API error codes into human-readable strings.
 */
inline const char* error_to_string(int32_t code) noexcept
{
    switch (code) {
    case GPCK_OK:
        return "GPCK_OK: Operation completed successfully.";
    case GPCK_ERR_NULL_PTR:
        return "GPCK_ERR_NULL_PTR: Null pointer argument provided.";
    case GPCK_ERR_INVALID_PATH:
        return "GPCK_ERR_INVALID_PATH: Invalid virtual file path.";
    case GPCK_ERR_NOT_FOUND:
        return "GPCK_ERR_NOT_FOUND: Asset or file not found.";
    case GPCK_ERR_BUFFER_TOO_SMALL:
        return "GPCK_ERR_BUFFER_TOO_SMALL: Destination buffer too small.";
    case GPCK_ERR_DECRYPTION_FAILED:
        return "GPCK_ERR_DECRYPTION_FAILED: Invalid passphrase or corrupted payload.";
    case GPCK_ERR_IO_FAILED:
        return "GPCK_ERR_IO_FAILED: Low-level file or memory I/O failed.";
    case GPCK_ERR_NOT_UNCOMPRESSED:
        return "GPCK_ERR_NOT_UNCOMPRESSED: Asset is compressed or chunked; zero-copy unavailable.";
    case GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED:
        return "GPCK_ERR_DIRECTSTORAGE_UNSUPPORTED: DirectStorage 1.4 unavailable on host.";
    default:
        return "UNKNOWN_ERROR: Unrecognized error code.";
    }
}

/**
 * @brief Exception thrown on fatal GPCK operations when using throwing API overloads.
 */
class Exception : public std::runtime_error
{
public:
    explicit Exception(int32_t code, const std::string& msg = "")
        : std::runtime_error(msg.empty() ? error_to_string(code) : (msg + ": " + error_to_string(code))), m_code(code)
    {
    }

    [[nodiscard]] int32_t code() const noexcept { return m_code; }

private:
    int32_t m_code;
};

/* Custom Deleters for std::unique_ptr */
struct VfsDeleter
{
    void operator()(GpckVfs* p) const noexcept
    {
        if (p)
            gpck_vfs_destroy(p);
    }
};

struct ArchiveDeleter
{
    void operator()(GpckArchive* p) const noexcept
    {
        if (p)
            gpck_archive_release(p);
    }
};

struct AssetSliceDeleter
{
    void operator()(GpckAssetSlice* p) const noexcept
    {
        if (p)
            gpck_asset_slice_release(p);
    }
};

using VfsRawPtr = std::unique_ptr<GpckVfs, VfsDeleter>;
using ArchiveRawPtr = std::unique_ptr<GpckArchive, ArchiveDeleter>;
using AssetSliceRawPtr = std::unique_ptr<GpckAssetSlice, AssetSliceDeleter>;

/**
 * @brief Safe RAII Zero-Copy Asset Slice.
 *
 * Holds an internal reference count on the owning archive memory map,
 * guaranteeing zero data races and complete lifetime safety.
 */
class AssetSlice
{
public:
    AssetSlice() noexcept = default;

    explicit AssetSlice(GpckAssetSlice* raw) noexcept : m_handle(raw) { update_pointers(); }

    AssetSlice(AssetSlice&& other) noexcept
        : m_handle(std::move(other.m_handle)), m_data(other.m_data), m_size(other.m_size)
    {
        other.m_data = nullptr;
        other.m_size = 0;
    }

    AssetSlice& operator=(AssetSlice&& other) noexcept
    {
        if (this != &other) {
            m_handle = std::move(other.m_handle);
            m_data = other.m_data;
            m_size = other.m_size;
            other.m_data = nullptr;
            other.m_size = 0;
        }
        return *this;
    }

    AssetSlice(const AssetSlice&) = delete;
    AssetSlice& operator=(const AssetSlice&) = delete;

    [[nodiscard]] bool valid() const noexcept { return m_handle != nullptr && m_data != nullptr; }
    [[nodiscard]] explicit operator bool() const noexcept { return valid(); }

    [[nodiscard]] const uint8_t* data() const noexcept { return m_data; }
    [[nodiscard]] size_t size() const noexcept { return m_size; }
    [[nodiscard]] bool empty() const noexcept { return m_size == 0; }

    [[nodiscard]] std::string_view string_view() const noexcept
    {
        return {reinterpret_cast<const char*>(m_data), m_size};
    }

    [[nodiscard]] GpckAssetSlice* get() const noexcept { return m_handle.get(); }

    void reset() noexcept
    {
        m_handle.reset();
        m_data = nullptr;
        m_size = 0;
    }

private:
    void update_pointers() noexcept
    {
        if (m_handle) {
            gpck_asset_slice_get_data(m_handle.get(), &m_data, &m_size);
        } else {
            m_data = nullptr;
            m_size = 0;
        }
    }

    AssetSliceRawPtr m_handle{nullptr};
    const uint8_t* m_data{nullptr};
    size_t m_size{0};
};

/**
 * @brief RAII GPCK Archive Package Controller.
 */
class Archive
{
public:
    Archive() noexcept = default;

    explicit Archive(GpckArchive* raw) noexcept : m_handle(raw) {}

    Archive(Archive&&) noexcept = default;
    Archive& operator=(Archive&&) noexcept = default;

    Archive(const Archive& other) noexcept
    {
        if (other.m_handle) {
            gpck_archive_retain(other.m_handle.get());
            m_handle.reset(other.m_handle.get());
        }
    }

    Archive& operator=(const Archive& other) noexcept
    {
        if (this != &other) {
            if (other.m_handle) {
                gpck_archive_retain(other.m_handle.get());
                m_handle.reset(other.m_handle.get());
            } else {
                m_handle.reset();
            }
        }
        return *this;
    }

    /**
     * @brief Opens an archive from disk.
     */
    static std::optional<Archive> open(std::string_view path, const char* passphrase = nullptr) noexcept
    {
        GpckArchive* raw = nullptr;
        std::string null_terminated_path(path);
        if (gpck_archive_open(null_terminated_path.c_str(), passphrase, &raw) == GPCK_OK && raw) {
            return Archive(raw);
        }
        return std::nullopt;
    }

    /**
     * @brief Opens an archive or throws an Exception on failure.
     */
    static Archive open_or_throw(std::string_view path, const char* passphrase = nullptr)
    {
        GpckArchive* raw = nullptr;
        std::string null_terminated_path(path);
        int32_t res = gpck_archive_open(null_terminated_path.c_str(), passphrase, &raw);
        if (res != GPCK_OK || !raw) {
            throw Exception(res, "Failed to open archive: " + null_terminated_path);
        }
        return Archive(raw);
    }

    [[nodiscard]] bool valid() const noexcept { return m_handle != nullptr; }
    [[nodiscard]] explicit operator bool() const noexcept { return valid(); }
    [[nodiscard]] GpckArchive* get() const noexcept { return m_handle.get(); }

    [[nodiscard]] uint32_t entry_count() const noexcept
    {
        uint32_t count = 0;
        if (m_handle && gpck_archive_get_entry_count(m_handle.get(), &count) == GPCK_OK) {
            return count;
        }
        return 0;
    }

    /**
     * @brief Acquires an RAII zero-copy memory-mapped slice (< 0.1 us).
     */
    [[nodiscard]] std::optional<AssetSlice> acquire_slice(std::string_view virtual_path) const noexcept
    {
        if (!m_handle)
            return std::nullopt;
        GpckAssetSlice* raw_slice = nullptr;
        std::string path_str(virtual_path);
        if (gpck_archive_acquire_asset_slice(m_handle.get(), path_str.c_str(), &raw_slice) == GPCK_OK && raw_slice) {
            return AssetSlice(raw_slice);
        }
        return std::nullopt;
    }

    /**
     * @brief Decompresses an asset into a pre-allocated memory buffer.
     */
    int32_t read_asset_to_buffer(std::string_view virtual_path, uint8_t* out_buf, size_t max_buf_len,
                                 size_t* out_written = nullptr) const noexcept
    {
        if (!m_handle)
            return GPCK_ERR_NULL_PTR;
        size_t written = 0;
        std::string path_str(virtual_path);
        int32_t res = gpck_archive_read_asset_by_path(m_handle.get(), path_str.c_str(), out_buf, max_buf_len, &written);
        if (out_written)
            *out_written = written;
        return res;
    }

    /**
     * @brief Decompresses an asset and returns an allocated byte vector.
     */
    [[nodiscard]] std::optional<std::vector<uint8_t>> read_asset(std::string_view virtual_path) const
    {
        if (!m_handle)
            return std::nullopt;
        std::string path_str(virtual_path);

        size_t required_size = 0;
        int32_t res = gpck_archive_read_asset_by_path(m_handle.get(), path_str.c_str(), nullptr, 0, &required_size);

        if (res != GPCK_OK || required_size == 0)
            return std::nullopt;

        std::vector<uint8_t> buffer(required_size);
        size_t written = 0;
        res = gpck_archive_read_asset_by_path(m_handle.get(), path_str.c_str(), buffer.data(), buffer.size(), &written);

        if (res == GPCK_OK) {
            buffer.resize(written);
            return buffer;
        }
        return std::nullopt;
    }

private:
    ArchiveRawPtr m_handle{nullptr};
};

/**
 * @brief RAII Virtual File System (VFS) Controller.
 */
class Vfs
{
public:
    Vfs() noexcept = default;

    explicit Vfs(GpckVfs* raw) noexcept : m_handle(raw) {}

    Vfs(Vfs&&) noexcept = default;
    Vfs& operator=(Vfs&&) noexcept = default;

    Vfs(const Vfs&) = delete;
    Vfs& operator=(const Vfs&) = delete;

    /**
     * @brief Creates a new VFS instance.
     */
    static std::optional<Vfs> create() noexcept
    {
        GpckVfs* raw = nullptr;
        if (gpck_vfs_create(&raw) == GPCK_OK && raw) {
            return Vfs(raw);
        }
        return std::nullopt;
    }

    [[nodiscard]] bool valid() const noexcept { return m_handle != nullptr; }
    [[nodiscard]] explicit operator bool() const noexcept { return valid(); }
    [[nodiscard]] GpckVfs* get() const noexcept { return m_handle.get(); }

    /**
     * @brief Mounts an archive file (.gtoc) into the VFS search space.
     */
    bool mount_archive(std::string_view path) noexcept
    {
        if (!m_handle)
            return false;
        std::string path_str(path);
        return gpck_vfs_mount_archive(m_handle.get(), path_str.c_str()) == GPCK_OK;
    }

    /**
     * @brief Mounts a loose directory into the VFS search space.
     */
    bool mount_directory(std::string_view path) noexcept
    {
        if (!m_handle)
            return false;
        std::string path_str(path);
        return gpck_vfs_mount_directory(m_handle.get(), path_str.c_str()) == GPCK_OK;
    }

    /**
     * @brief Reads a file through the VFS into a byte vector.
     */
    [[nodiscard]] std::optional<std::vector<uint8_t>> read_file(std::string_view virtual_path) const
    {
        if (!m_handle)
            return std::nullopt;
        std::string path_str(virtual_path);

        size_t required_size = 0;
        int32_t res = gpck_vfs_read_file(m_handle.get(), path_str.c_str(), nullptr, 0, &required_size);

        if (res != GPCK_OK || required_size == 0)
            return std::nullopt;

        std::vector<uint8_t> buffer(required_size);
        size_t written = 0;
        res = gpck_vfs_read_file(m_handle.get(), path_str.c_str(), buffer.data(), buffer.size(), &written);

        if (res == GPCK_OK) {
            buffer.resize(written);
            return buffer;
        }
        return std::nullopt;
    }

    /**
     * @brief Reads a text file through the VFS as a std::string.
     */
    [[nodiscard]] std::optional<std::string> read_text(std::string_view virtual_path) const
    {
        auto bytes = read_file(virtual_path);
        if (bytes) {
            return std::string(reinterpret_cast<const char*>(bytes->data()), bytes->size());
        }
        return std::nullopt;
    }

    /**
     * @brief DirectStorage: Streams an asset directly to a D3D12 GPU Buffer.
     */
    [[nodiscard]] std::optional<uint64_t> stream_to_d3d12_buffer(std::string_view virtual_path, void* d3d12_resource,
                                                                 uint64_t dest_offset = 0,
                                                                 Priority priority = Priority::Normal) const noexcept
    {
        if (!m_handle)
            return std::nullopt;
        uint64_t fence_val = 0;
        std::string path_str(virtual_path);
        int32_t res = gpck_vfs_stream_file_to_d3d12_buffer(m_handle.get(), path_str.c_str(), d3d12_resource,
                                                           dest_offset, static_cast<int32_t>(priority), &fence_val);
        if (res == GPCK_OK)
            return fence_val;
        return std::nullopt;
    }

    /**
     * @brief DirectStorage: Streams a texture directly to a D3D12 Texture2D Resource.
     */
    [[nodiscard]] std::optional<uint64_t> stream_to_d3d12_texture(std::string_view virtual_path, void* d3d12_texture,
                                                                  uint32_t first_subresource = 0,
                                                                  Priority priority = Priority::Normal) const noexcept
    {
        if (!m_handle)
            return std::nullopt;
        uint64_t fence_val = 0;
        std::string path_str(virtual_path);
        int32_t res =
            gpck_vfs_stream_file_to_d3d12_texture(m_handle.get(), path_str.c_str(), d3d12_texture, first_subresource,
                                                  static_cast<int32_t>(priority), &fence_val);
        if (res == GPCK_OK)
            return fence_val;
        return std::nullopt;
    }

    /**
     * @brief DirectStorage: Streams a 64KB sparse tile directly into a D3D12 Tiled Resource.
     */
    [[nodiscard]] std::optional<uint64_t>
    stream_tile_to_d3d12_texture(std::string_view virtual_path, void* d3d12_tiled_texture, uint32_t subresource,
                                 uint32_t tile_x, uint32_t tile_y, uint32_t tile_z = 0,
                                 Priority priority = Priority::Normal) const noexcept
    {
        if (!m_handle)
            return std::nullopt;
        uint64_t fence_val = 0;
        std::string path_str(virtual_path);
        int32_t res =
            gpck_vfs_stream_tile_to_d3d12_texture(m_handle.get(), path_str.c_str(), d3d12_tiled_texture, subresource,
                                                  tile_x, tile_y, tile_z, static_cast<int32_t>(priority), &fence_val);
        if (res == GPCK_OK)
            return fence_val;
        return std::nullopt;
    }

    /**
     * @brief DirectStorage: Waits for a GPU fence value to complete.
     */
    static bool wait_for_d3d12_fence(Priority priority, uint64_t fence_value) noexcept
    {
        return gpck_vfs_wait_for_d3d12_fence(static_cast<int32_t>(priority), fence_value) == GPCK_OK;
    }

private:
    VfsRawPtr m_handle{nullptr};
};

/**
 * @brief Checks if DirectStorage 1.4 is active on the current hardware.
 */
inline bool is_directstorage_supported() noexcept
{
    return gpck_directstorage_is_supported() != 0;
}

} // namespace gpck
