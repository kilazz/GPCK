// crates/gpck_core/src_cpp/brotlig_wrapper.cpp
/**
 * @file brotlig_wrapper.cpp
 * @brief Clean, thread-safe C-ABI wrapper for AMD Brotli-G SDK.
 *
 * SPDX-FileCopyrightText: Copyright (c) 2022 - 2024 Advanced Micro Devices, Inc.
 * SPDX-License-Identifier: MIT
 */

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <exception>
#include <vector>

#if defined(_WIN32)
#define NOMINMAX
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#endif

#if defined(BROTLIG_SDK_AVAILABLE)
#include "BrotliG.h"
#include "BrotligDecoder.h"
#include "BrotligEncoder.h"
#include "DataStream.h"
#endif

extern "C" {

/**
 * @brief Calculates the upper-bound buffer size required for Brotli-G compression.
 */
size_t BrotliGCompressBound(size_t in_size)
{
#if defined(BROTLIG_SDK_AVAILABLE)
    try {
        uint32_t bound = BrotliG::MaxCompressedSize(static_cast<uint32_t>(in_size), false, false);
        return static_cast<size_t>(bound) + 131072;
    } catch (...) {
        return in_size + (in_size >> 2) + 131072;
    }
#else
    return in_size + (in_size >> 2) + 131072;
#endif
}

/**
 * @brief Reads the uncompressed size from the Brotli-G stream header.
 */
uint32_t BrotliGGetDecompressedSize(const uint8_t* in_data)
{
#if defined(BROTLIG_SDK_AVAILABLE)
    if (!in_data)
        return 0;
    try {
        return static_cast<uint32_t>(BrotliG::DecompressedSize(const_cast<uint8_t*>(in_data)));
    } catch (...) {
        return 0;
    }
#else
    (void)in_data;
    return 0;
#endif
}

/**
 * @brief Compresses data using AMD Brotli-G.
 */
bool BrotliGCompress(uint8_t* out, size_t* out_size, const uint8_t* in_data, size_t in_size, uint32_t page_size,
                     uint32_t /* level */
)
{
    if (!out || !out_size || !in_data || in_size == 0) {
        return false;
    }

#if defined(BROTLIG_SDK_AVAILABLE)
    uint32_t input_bytes = static_cast<uint32_t>(in_size);
    uint32_t max_out_bytes = static_cast<uint32_t>(*out_size);
    uint32_t actual_out_bytes = max_out_bytes;
    uint32_t target_page_size = (page_size > 0) ? page_size : 65536;

    BrotliG::BrotligDataconditionParams dcParams = {};
    dcParams.precondition = false;

    std::vector<uint8_t> compress_buf(static_cast<size_t>(max_out_bytes) + 262144, 0);
    uint8_t* temp_out = compress_buf.data();

    BROTLIG_ERROR err = BROTLIG_ERROR_CORRUPT_STREAM;

    try {
        err = BrotliG::Encode(input_bytes, in_data, &actual_out_bytes, temp_out, target_page_size, dcParams, nullptr);
    } catch (...) {
        return false;
    }

    if (err == BROTLIG_OK && actual_out_bytes <= max_out_bytes && actual_out_bytes > 0) {
        std::memcpy(out, temp_out, actual_out_bytes);
        *out_size = static_cast<size_t>(actual_out_bytes);
        return true;
    }

    return false;
#else
    (void)out;
    (void)out_size;
    (void)in_data;
    (void)in_size;
    (void)page_size;
    return false;
#endif
}

/**
 * @brief Decompresses Brotli-G stream data.
 */
bool BrotliGDecompress(uint8_t* out, size_t out_size, const uint8_t* in_data, size_t in_size)
{
    if (!out || out_size == 0 || !in_data || in_size < 8) {
        return false;
    }

#if defined(BROTLIG_SDK_AVAILABLE)
    std::vector<uint8_t> safe_in_buf(in_size + 128, 0);
    std::memcpy(safe_in_buf.data(), in_data, in_size);

    uint32_t input_bytes = static_cast<uint32_t>(in_size);
    uint32_t actual_decomp_bytes = static_cast<uint32_t>(out_size);
    BROTLIG_ERROR err = BROTLIG_ERROR_CORRUPT_STREAM;

    try {
        err = BrotliG::DecodeCPU(input_bytes, safe_in_buf.data(), &actual_decomp_bytes, out, nullptr);
    } catch (...) {
        return false;
    }

    return (err == BROTLIG_OK && actual_decomp_bytes == static_cast<uint32_t>(out_size));
#else
    (void)out;
    (void)out_size;
    (void)in_data;
    (void)in_size;
    return false;
#endif
}

} // extern "C"
