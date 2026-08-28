// crates/gpck_core/src_cpp/gdeflate_wrapper.cpp
/**
 * @file gdeflate_wrapper.cpp
 * @brief Native C-ABI wrapper for Microsoft DirectStorage GDeflate C++ Core.
 *
 * SPDX-FileCopyrightText: Copyright (c) Microsoft Corporation. All rights reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

#include <cstddef>
#include <cstdint>
#include <cstring>

#include "GDeflate.h"

#if defined(_MSC_VER)
#include <windows.h>
#endif

extern "C" {

/**
 * @brief Calculates the maximum upper-bound buffer size required for GDeflate compression.
 * @param in_size Size of the uncompressed input buffer in bytes.
 * @return Maximum required buffer size in bytes.
 */
size_t GDeflateCompressBound(size_t in_size)
{
    return GDeflate::CompressBound(in_size);
}

/**
 * @brief Compresses an input byte buffer using Microsoft GDeflate.
 *
 * @param out Pointer to destination buffer.
 * @param out_size In: Capacity of destination buffer; Out: Exact compressed bytes written.
 * @param in_data Pointer to the source data buffer.
 * @param in_size Size of the input buffer in bytes.
 * @param level Compression level (1 to 9).
 * @param flags Compression option flags.
 * @return true if compression succeeded, false otherwise.
 */
bool GDeflateCompress(uint8_t* out, size_t* out_size, const uint8_t* in_data, size_t in_size, uint32_t level,
                      uint32_t flags)
{
    if (!out || !out_size || !in_data || in_size == 0) {
        return false;
    }

    uint32_t clamped_level = (level < 1u) ? 1u : ((level > 9u) ? 9u : level);

#if defined(_MSC_VER)
    __try {
        return GDeflate::Compress(out, out_size, in_data, in_size, clamped_level, flags);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
#else
    return GDeflate::Compress(out, out_size, in_data, in_size, clamped_level, flags);
#endif
}

/**
 * @brief Decompresses a GDeflate compressed payload into a destination buffer.
 *
 * @param out Pointer to destination buffer.
 * @param out_size Expected exact uncompressed target size in bytes.
 * @param in_data Pointer to the compressed source byte buffer.
 * @param in_size Size of the compressed source buffer in bytes.
 * @param num_workers Number of worker threads for parallel decompression.
 * @return true if decompression succeeded, false otherwise.
 */
bool GDeflateDecompress(uint8_t* out, size_t out_size, const uint8_t* in_data, size_t in_size, uint32_t num_workers)
{
    // Ultra-compressible flat/black 64KB tiles can compress down to 4-12 bytes
    if (!out || out_size == 0 || !in_data || in_size < 4) {
        return false;
    }

    uint32_t workers = (num_workers > 0u) ? num_workers : 1u;

#if defined(_MSC_VER)
    __try {
        return GDeflate::Decompress(out, out_size, in_data, in_size, workers);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
#else
    return GDeflate::Decompress(out, out_size, in_data, in_size, workers);
#endif
}

} // extern "C"
