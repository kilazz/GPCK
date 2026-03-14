#include "../../DirectStorage/GDeflate/GDeflate/GDeflate.h"

// Define the export macro directly here for the DLL generation
#define GDEFLATE_EXPORT extern "C" __declspec(dllexport)

GDEFLATE_EXPORT size_t GDeflateCompressBound(size_t size)
{
    return GDeflate::CompressBound(size);
}

GDEFLATE_EXPORT bool GDeflateCompress(uint8_t* output, size_t* outputSize, const uint8_t* in, size_t inSize, uint32_t level, uint32_t flags)
{
    return GDeflate::Compress(output, outputSize, in, inSize, level, flags);
}

GDEFLATE_EXPORT bool GDeflateDecompress(uint8_t* output, size_t outputSize, const uint8_t* in, size_t inSize, uint32_t numWorkers)
{
    return GDeflate::Decompress(output, outputSize, in, inSize, numWorkers);
}
