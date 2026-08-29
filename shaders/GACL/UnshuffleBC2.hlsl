// shaders/GACL/UnshuffleBC2.hlsl
//--------------------------------------------------------------------------------------
// UnshuffleBC2.hlsl
//
// GPCK GPU Compute Shaders (Vectorized 128-bit Quad Unshuffler)
// Copyright (C) GPCK Contributors. All rights reserved.
//--------------------------------------------------------------------------------------
#include "Shared.hlsli"
#include "SharedDefinitions.hlsli"

ByteAddressBuffer srcBuffer : register(t0);
RWStructuredBuffer<uint4> dstBuffer : register(u0);

struct BC2Info
{
    uint bufferSizeInBytes;
    uint bufferOffsetInBytes;
    uint transformId;
    uint widthInPixels;
};

#if defined(__spirv__) || defined(__SPIRV__)
[[vk::push_constant]] BC2Info constants;
#else
ConstantBuffer<BC2Info> constants : register(b0);
#endif

#define BC2_BLOCK_SIZE 16
#define BLOCKS_PER_QUAD 4

[numthreads(256, 1, 1)]
[RootSignature(RootSig4c)]
void main(uint3 dtID : SV_DispatchThreadID)
{
    uint totalBlocks = constants.bufferSizeInBytes / BC2_BLOCK_SIZE;
    uint numQuads = totalBlocks / BLOCKS_PER_QUAD;
    uint numStragglers = totalBlocks % BLOCKS_PER_QUAD;

    uint shuffledBufferSizeInBytes = (numQuads * BLOCKS_PER_QUAD * BC2_BLOCK_SIZE);
    uint bufferOffsetInElements = constants.bufferOffsetInBytes / BC2_BLOCK_SIZE;

    uint threadIndex = dtID.x;

    if (threadIndex < numQuads)
    {
        uint firstBlockIndex = (threadIndex * BLOCKS_PER_QUAD);

        uint alphaOffset        = 0;
        uint e0Offset           = (shuffledBufferSizeInBytes / 2);
        uint e1Offset           = (shuffledBufferSizeInBytes / 2) + (shuffledBufferSizeInBytes / 8);
        uint colorIndicesOffset = (shuffledBufferSizeInBytes / 2) + (shuffledBufferSizeInBytes / 4);

        // Vectorized 128-bit loads for 32 bytes alpha nibbles (4 blocks)
        uint4 alphaNibblesA = srcBuffer.Load4(alphaOffset + firstBlockIndex * 8);
        uint4 alphaNibblesB = srcBuffer.Load4(alphaOffset + firstBlockIndex * 8 + 16);

        // 64-bit loads for endpoints
        uint2 e0Quad = srcBuffer.Load2(e0Offset + firstBlockIndex * 2);
        uint2 e1Quad = srcBuffer.Load2(e1Offset + firstBlockIndex * 2);

        // 128-bit load for color indices
        uint4 colorIndices = srcBuffer.Load4(colorIndicesOffset + firstBlockIndex * 4);

        uint4 bc2Blocks[4];

        bc2Blocks[0].xy = alphaNibblesA.xy;
        bc2Blocks[0].z  = (e0Quad.x & 0xFFFFu) | (e1Quad.x << 16u);
        bc2Blocks[0].w  = colorIndices.x;

        bc2Blocks[1].xy = alphaNibblesA.zw;
        bc2Blocks[1].z  = (e0Quad.x >> 16u) | (e1Quad.x & 0xFFFF0000u);
        bc2Blocks[1].w  = colorIndices.y;

        bc2Blocks[2].xy = alphaNibblesB.xy;
        bc2Blocks[2].z  = (e0Quad.y & 0xFFFFu) | (e1Quad.y << 16u);
        bc2Blocks[2].w  = colorIndices.z;

        bc2Blocks[3].xy = alphaNibblesB.zw;
        bc2Blocks[3].z  = (e0Quad.y >> 16u) | (e1Quad.y & 0xFFFF0000u);
        bc2Blocks[3].w  = colorIndices.w;

        dstBuffer[bufferOffsetInElements + threadIndex * 4 + 0] = bc2Blocks[0];
        dstBuffer[bufferOffsetInElements + threadIndex * 4 + 1] = bc2Blocks[1];
        dstBuffer[bufferOffsetInElements + threadIndex * 4 + 2] = bc2Blocks[2];
        dstBuffer[bufferOffsetInElements + threadIndex * 4 + 3] = bc2Blocks[3];
    }
    else if (threadIndex < (numQuads + numStragglers))
    {
        uint stragglerIndex = threadIndex - numQuads;
        uint stragglerByteOffset = shuffledBufferSizeInBytes + (stragglerIndex * BC2_BLOCK_SIZE);

        uint4 oneBC2Block = srcBuffer.Load4(stragglerByteOffset);
        dstBuffer[bufferOffsetInElements + numQuads * BLOCKS_PER_QUAD + stragglerIndex] = oneBC2Block;
    }
}
