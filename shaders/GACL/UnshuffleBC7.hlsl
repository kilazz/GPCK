// shaders/GACL/UnshuffleBC7.hlsl
//--------------------------------------------------------------------------------------
// UnshuffleBC7.hlsl
//
// GPCK GPU Compute Shaders (Optimized Vectorized Variant)
// Copyright (C) GPCK Contributors. All rights reserved.
//--------------------------------------------------------------------------------------
#include "Shared.hlsli"
#include "SharedDefinitions.hlsli"

ByteAddressBuffer srcBuffer : register(t0);
RWStructuredBuffer<uint4> dstBuffer : register(u0);

struct BC7Info
{
    uint bufferSizeInBytes;
    uint bufferOffsetInBytes;
    uint transformId;
    uint widthInPixels;
};

#if defined(__spirv__) || defined(__SPIRV__)
[[vk::push_constant]] BC7Info constants;
#else
ConstantBuffer<BC7Info> constants : register(b0);
#endif

#define BC7_BLOCK_SIZE 16

[numthreads(256, 1, 1)]
[RootSignature(RootSig4c)]
void main(uint3 dtID : SV_DispatchThreadID)
{
    uint totalBlocks = constants.bufferSizeInBytes / BC7_BLOCK_SIZE;
    uint blockIdx = dtID.x;

    if (blockIdx >= totalBlocks)
    {
        return;
    }

    uint bufferOffsetInElements = constants.bufferOffsetInBytes / BC7_BLOCK_SIZE;

    // Transform 11: Mode Join (12 bytes endpoints + 4 bytes indices)
    if (constants.transformId == 11)
    {
        uint endpointsBase = 0;
        uint indicesBase   = totalBlocks * 12;

        uint3 ep = srcBuffer.Load3(endpointsBase + blockIdx * 12);
        uint idx = srcBuffer.Load(indicesBase + blockIdx * 4);

        dstBuffer[bufferOffsetInElements + blockIdx] = uint4(ep.x, ep.y, ep.z, idx);
    }
    // Transform 10: Mode Split (1 byte mode, 8 bytes color endpoints, 7 bytes indices)
    else if (constants.transformId == 10)
    {
        uint modeBase  = 0;
        uint colorBase = totalBlocks;
        uint indexBase = totalBlocks * 9;

        // Aligned 4-byte read for mode header stream
        uint modeDword = srcBuffer.Load(modeBase + (blockIdx & ~3u));
        uint modeShift = (blockIdx & 3u) << 3u;
        uint headerByte = (modeDword >> modeShift) & 0xFFu;
        uint mode = headerByte & 0x0Fu;

        if (mode >= 8)
        {
            dstBuffer[bufferOffsetInElements + blockIdx] = uint4(0, 0, 0, 0);
            return;
        }

        // Fast Load2 for 8 bytes color endpoints
        uint2 colorEp = srcBuffer.Load2(colorBase + blockIdx * 8);

        // Vectorized 7-byte Index Extraction
        uint idxByteOffset = indexBase + blockIdx * 7;
        uint idxBaseAligned = idxByteOffset & ~3u;
        uint idxSubShift = (idxByteOffset & 3u) << 3u;

        uint3 idxRaw = srcBuffer.Load3(idxBaseAligned);

        uint64_t low64 = ((uint64_t)idxRaw.y << 32) | idxRaw.x;
        uint64_t high64 = ((uint64_t)idxRaw.z << 32) | idxRaw.y;

        uint i0 = (uint)(low64 >> idxSubShift);
        uint i1 = (uint)(high64 >> (idxSubShift + 24u)) & 0xFFFFFFu;

        dstBuffer[bufferOffsetInElements + blockIdx] = uint4(colorEp.x, colorEp.y, i0, i1);
    }
    else
    {
        // Pass-through fallback (128-bit aligned copy)
        dstBuffer[bufferOffsetInElements + blockIdx] = srcBuffer.Load4(blockIdx * 16);
    }
}
