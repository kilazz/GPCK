// shaders/GACL/UnshuffleBC4x.hlsl
//--------------------------------------------------------------------------------------
// UnshuffleBC4x.hlsl
//
// Advanced Technology Group (ATG) & GPCK High-Performance Variant
// Copyright (C) Microsoft Corporation / GPCK Contributors. All rights reserved.
//--------------------------------------------------------------------------------------
#include "Shared.hlsli"
#include "SharedDefinitions.hlsli"

ByteAddressBuffer srcBuffer : register(t0);

RWStructuredBuffer<uint2> dstBuffer : register(u0);

struct BC4Info
{
    uint bufferSizeInBytes;
    uint bufferOffsetInBytes;
    uint transformId;
    uint widthInPixels;
};

#if defined(__spirv__) || defined(__SPIRV__)
[[vk::push_constant]] BC4Info constants;
#else
ConstantBuffer<BC4Info> constants : register(b0);
#endif

#define BC4_BLOCK_SIZE 8
#define BLOCKS_PER_QUAD 4

[numthreads(256, 1, 1)]
[RootSignature(RootSig4c)]
void main(uint3 dtID : SV_DispatchThreadID)
{
    uint totalBlocks = constants.bufferSizeInBytes / BC4_BLOCK_SIZE;
    uint numQuads = totalBlocks / BLOCKS_PER_QUAD;
    uint numStragglers = totalBlocks % BLOCKS_PER_QUAD;

    uint shuffledBufferSizeInBytes = (numQuads * BLOCKS_PER_QUAD * BC4_BLOCK_SIZE);
    uint bufferOffsetInElements = constants.bufferOffsetInBytes / BC4_BLOCK_SIZE;

    uint threadIndex = dtID.x;

    if (threadIndex < numQuads)
    {
        uint firstBlockIndex = (threadIndex * BLOCKS_PER_QUAD);
        uint4 bc4Blocks[2];

        if (constants.transformId == 3 || constants.transformId == 19)
        {
            uint e1Offset       = (shuffledBufferSizeInBytes / 8);
            uint indicesOffset  = (shuffledBufferSizeInBytes / 4);

            uint e0Quad    = srcBuffer.Load(firstBlockIndex);
            uint e1Quad    = srcBuffer.Load(e1Offset + firstBlockIndex);
            uint3 indicesA = srcBuffer.Load3(indicesOffset + firstBlockIndex * 6);
            uint3 indicesB = srcBuffer.Load3(indicesOffset + firstBlockIndex * 6 + 12);

            bc4Blocks[0].x = ((e0Quad >> 0) & 0xFFu) | (((e1Quad >> 0) & 0xFFu) << 8u) | (indicesA.x << 16u);
            bc4Blocks[0].y = (indicesA.x >> 16u) | (indicesA.y << 16u);

            bc4Blocks[0].z = ((e0Quad >> 8) & 0xFFu) | (((e1Quad >> 8) & 0xFFu) << 8u) | (indicesA.y & 0xFFFF0000u);
            bc4Blocks[0].w = indicesA.z;

            bc4Blocks[1].x = ((e0Quad >> 16) & 0xFFu) | (((e1Quad >> 16) & 0xFFu) << 8u) | (indicesB.x << 16u);
            bc4Blocks[1].y = (indicesB.x >> 16u) | (indicesB.y << 16u);

            bc4Blocks[1].z = ((e0Quad >> 24) & 0xFFu) | (((e1Quad >> 24) & 0xFFu) << 8u) | (indicesB.y & 0xFFFF0000u);
            bc4Blocks[1].w = indicesB.z;
        }
        else
        {
            bc4Blocks[0] = srcBuffer.Load4(firstBlockIndex * 8);
            bc4Blocks[1] = srcBuffer.Load4(firstBlockIndex * 8 + 16);
        }

        if (constants.transformId == 19)
        {
            uint finalBlockID = ReverseSpaceCurveFor8ByteBlock(shuffledBufferSizeInBytes, constants.widthInPixels, threadIndex * BLOCKS_PER_QUAD);
            dstBuffer[bufferOffsetInElements + finalBlockID + 0] = bc4Blocks[0].xy;
            dstBuffer[bufferOffsetInElements + finalBlockID + 1] = bc4Blocks[0].zw;
            dstBuffer[bufferOffsetInElements + finalBlockID + 2] = bc4Blocks[1].xy;
            dstBuffer[bufferOffsetInElements + finalBlockID + 3] = bc4Blocks[1].zw;
        }
        else
        {
            dstBuffer[bufferOffsetInElements + threadIndex * BLOCKS_PER_QUAD + 0] = bc4Blocks[0].xy;
            dstBuffer[bufferOffsetInElements + threadIndex * BLOCKS_PER_QUAD + 1] = bc4Blocks[0].zw;
            dstBuffer[bufferOffsetInElements + threadIndex * BLOCKS_PER_QUAD + 2] = bc4Blocks[1].xy;
            dstBuffer[bufferOffsetInElements + threadIndex * BLOCKS_PER_QUAD + 3] = bc4Blocks[1].zw;
        }
    }
    else if (threadIndex < (numQuads + numStragglers))
    {
        uint stragglerIndex = threadIndex - numQuads;
        uint stragglerByteOffset = shuffledBufferSizeInBytes + (stragglerIndex * BC4_BLOCK_SIZE);

        uint2 oneBC4Block = srcBuffer.Load2(stragglerByteOffset);
        dstBuffer[bufferOffsetInElements + numQuads * BLOCKS_PER_QUAD + stragglerIndex] = oneBC4Block;
    }
}
