// src_cpp/zstdgpu_wrapper.cpp
#include <cstdarg>
#include <cstdint>
#include <cstdio>

// ============================================================================
// Microsoft ATG Assert Hooks & Diagnostics
// ============================================================================

// Global assertion flags expected by Microsoft ATG / zstdgpu headers
extern "C" {
int tta_AssertAlways_0 = 0;
int tta_AssertAlways_1 = 0;
}

// C++ mangled diagnostic reporting handler matching:
// int __cdecl tta_AssertReport(char const *,char const *,int,char const *,char const *,...)
int __cdecl tta_AssertReport(const char* file, const char* condition, int line, const char* function,
                             const char* format, ...)
{
    va_list args;
    va_start(args, format);
    char buffer[2048];
    vsnprintf(buffer, sizeof(buffer), (format && *format) ? format : "", args);
    va_end(args);

#if defined(_DEBUG) || !defined(NDEBUG)
    std::fprintf(stderr, "[ZstdGPU Assert] %s:%d in %s\n  Condition: %s\n  Message: %s\n", file ? file : "unknown",
                 line, function ? function : "unknown", condition ? condition : "unknown", buffer);
#endif

    // Return 0 to continue execution without forcing an unhandled DebugBreak
    return 0;
}

// ============================================================================
// Native C-ABI Exports for Rust Runtime Interop
// ============================================================================

extern "C" {
void* ZstdGpu_CreateContext(void* /*d3d12_device*/)
{
    // Fallback stub if compiled without DirectStorage D3D12 hardware context
    return nullptr;
}

bool ZstdGpu_Decompress(void* /*handle*/, const uint8_t* /*in_compressed_data*/, uint32_t /*in_compressed_size*/,
                        void* /*out_vram_buffer*/, uint64_t /*out_vram_offset*/, uint32_t /*out_uncompressed_size*/)
{
    return false;
}

void ZstdGpu_DestroyContext(void* /*handle*/)
{
    // No-op cleanup
}
}
