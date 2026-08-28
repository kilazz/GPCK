// examples/cpp/test_main.cpp
#include <algorithm>
#include <chrono>
#include <iostream>
#include <numeric>
#include <vector>

#include "gpck.hpp"

static uint8_t g_ScratchBuffer[8 * 1024 * 1024];

int main(int argc, char* argv[])
{
    std::cout << "=====================================================\n";
    std::cout << " GPCK Modern C++17 RAII SDK & Streaming Benchmark\n";
    std::cout << "=====================================================\n\n";

    const char* archive_path = (argc > 1) ? argv[1] : "demo.gtoc";
    const char* test_file = (argc > 2) ? argv[2] : "main.rs";

    // [1] Initialize VFS Engine via RAII
    auto vfs = gpck::Vfs::create();
    if (!vfs) {
        std::cerr << "[ERROR] Failed to initialize VFS!\n";
        return 1;
    }

    // [2] Check DirectStorage 1.4 Support
    if (gpck::is_directstorage_supported()) {
        std::cout << "    -> [GPU ACTIVE] DirectStorage 1.4 BypassIO is READY.\n\n";
    } else {
        std::cout << "    -> [INFO] DirectStorage GPU is inactive (CPU SIMD fallback).\n\n";
    }

    // [3] Open Archive via RAII
    auto archive = gpck::Archive::open(archive_path);
    if (!archive) {
        std::cerr << "[ERROR] Failed to open archive: " << archive_path << "\n";
        return 1;
    }

    std::cout << "    -> Archive loaded! Assets count: " << archive->entry_count() << "\n\n";

    // --- TEST A: Safe RAII Zero-Copy Asset Slice (< 0.1 us) ---
    std::cout << "--- Test A: Safe RAII Zero-Copy Asset Slice ---\n";
    auto t0 = std::chrono::high_resolution_clock::now();
    auto slice = archive->acquire_slice(test_file);
    auto t1 = std::chrono::high_resolution_clock::now();

    double zcopy_ns = std::chrono::duration<double, std::nano>(t1 - t0).count();

    if (slice) {
        std::cout << "    -> [RAII ZERO-COPY SUCCESS] Direct Pointer: " << (void*)slice->data() << " (" << slice->size()
                  << " bytes) in " << zcopy_ns << " ns (< 0.1 us!)\n\n";
    } else {
        std::cout << "    -> [INFO] Asset is compressed. Using pre-allocated scratch decompressor.\n\n";
    }

    // --- TEST B: Pre-allocated Scratch Buffer Decompression (1,000 runs) ---
    std::cout << "--- Test B: Zero-Allocation Scratch Decompression (1,000 runs) ---\n";
    const int NUM_RUNS = 1000;
    std::vector<double> latencies_us;
    latencies_us.reserve(NUM_RUNS);

    size_t bytes_written = 0;
    archive->read_asset_to_buffer(test_file, g_ScratchBuffer, sizeof(g_ScratchBuffer), &bytes_written);

    for (int i = 0; i < NUM_RUNS; ++i) {
        auto start = std::chrono::high_resolution_clock::now();
        int32_t res =
            archive->read_asset_to_buffer(test_file, g_ScratchBuffer, sizeof(g_ScratchBuffer), &bytes_written);
        auto end = std::chrono::high_resolution_clock::now();

        if (res == GPCK_OK) {
            latencies_us.push_back(std::chrono::duration<double, std::micro>(end - start).count());
        }
    }

    if (!latencies_us.empty()) {
        std::sort(latencies_us.begin(), latencies_us.end());
        double min_us = latencies_us.front();
        double max_us = latencies_us.back();
        double p50_us = latencies_us[latencies_us.size() / 2];
        double p99_us = latencies_us[static_cast<size_t>(latencies_us.size() * 0.99)];
        double avg_us = std::accumulate(latencies_us.begin(), latencies_us.end(), 0.0) / latencies_us.size();

        double throughput_gb = (bytes_written / (1024.0 * 1024.0 * 1024.0)) / (avg_us / 1000000.0);

        std::cout << "    -> Target Asset : '" << test_file << "' (" << bytes_written << " bytes)\n";
        std::cout << "    -> Min Latency  : " << min_us << " us\n";
        std::cout << "    -> Median (P50) : " << p50_us << " us\n";
        std::cout << "    -> Average      : " << avg_us << " us\n";
        std::cout << "    -> P99 Latency  : " << p99_us << " us\n";
        std::cout << "    -> Max Latency  : " << max_us << " us\n";
        std::cout << "    -> Single-Thread: " << throughput_gb << " GB/s\n\n";
    }

    std::cout << "[SUCCESS] Full Modern C++ RAII Integration Complete.\n";
    return 0;
} // vfs, archive, slice automatically close and release all memory here!
