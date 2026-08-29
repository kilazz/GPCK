// shaders/GACL/UnshuffleBC6h.hlsl
//--------------------------------------------------------------------------------------
// UnshuffleBC6h.hlsl
//
// GPCK GPU Compute Shaders (Optimized 128-bit HDR Recombiner)
// Copyright (C) GPCK Contributors. All rights reserved.
//--------------------------------------------------------------------------------------
#include "Shared.hlsli"
#include "SharedDefinitions.hlsli"

ByteAddressBuffer srcBuffer : register(t0);
RWStructuredBuffer<uint4> dstBuffer : register(u0);

struct BC6HInfo
{
    uint bufferSizeInBytes;
    uint bufferOffsetInBytes;
    uint transformId;
    uint widthInPixels;
};

#if defined(__spirv__) || defined(__SPIRV__)
[[vk::push_constant]] BC6HInfo constants;
#else
ConstantBuffer<BC6HInfo> constants : register(b0);
#endif

#define BC6H_BLOCK_SIZE 16

[numthreads(256, 1, 1)]
[RootSignature(RootSig4c)]
void main(uint3 dtID : SV_DispatchThreadID)
{
    uint totalBlocks = constants.bufferSizeInBytes / BC6H_BLOCK_SIZE;
    uint blockIdx = dtID.x;

    if (blockIdx >= totalBlocks)
    {
        return;
    }

    uint bufferOffsetInElements = constants.bufferOffsetInBytes / BC6H_BLOCK_SIZE;

    // Header stream: 10 bytes per block | Index stream: 6 bytes per block
    uint headerBase = 0;
    uint indexBase  = totalBlocks * 10;

    uint hPos = headerBase + (blockIdx * 10);
    uint iPos = indexBase  + (blockIdx * 6);

    // Fast Aligned Multi-word loads
    uint hBaseAligned = hPos & ~3u;
    uint hShift = (hPos & 3u) << 3u;
    uint3 hRaw = srcBuffer.Load3(hBaseAligned);

    uint64_t h64_A = ((uint64_t)hRaw.y << 32) | hRaw.x;
    uint64_t h64_B = ((uint64_t)hRaw.z << 32) | hRaw.y;

    uint h0 = (uint)(h64_A >> hShift);
    uint h1 = (uint)(h64_B >> hShift);
    uint h2_u16 = (uint)(hRaw.z >> hShift) & 0xFFFFu;

    uint iBaseAligned = iPos & ~3u;
    uint iShift = (iPos & 3u) << 3u;
    uint2 iRaw = srcBuffer.Load2(iBaseAligned);

    uint64_t i64 = ((uint64_t)iRaw.y << 32) | iRaw.x;
    uint iVal = (uint)(i64 >> iShift);

    uint i0_u16 = iVal & 0xFFFFu;
    uint i1 = (uint)(i64 >> (iShift + 16u));

    uint4 block;
    block.x = h0;
    block.y = h1;
    block.z = h2_u16 | (i0_u16 << 16u);
    block.w = i1;

    dstBuffer[bufferOffsetInElements + blockIdx] = block;
}
