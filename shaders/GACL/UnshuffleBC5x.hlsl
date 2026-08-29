// shaders/GACL/UnshuffleBC5x.hlsl
//--------------------------------------------------------------------------------------
// UnshuffleBC5x.hlsl
//
// Advanced Technology Group (ATG) & GPCK High-Performance Variant
// Copyright (C) Microsoft Corporation / GPCK Contributors. All rights reserved.
//--------------------------------------------------------------------------------------
#include "Shared.hlsli"
#include "SharedDefinitions.hlsli"

ByteAddressBuffer srcBuffer : register(t0);

RWStructuredBuffer<uint4> dstBuffer : register(u0);

struct BC5Info
{
    uint bufferSizeInBytes;
    uint bufferOffsetInBytes;
    uint transformId;
    uint widthInPixels;
};

#if defined(__spirv__) || defined(__SPIRV__)
[[vk::push_constant]] BC5Info constants;
#else
ConstantBuffer<BC5Info> constants : register(b0);
#endif

#define BC5_BLOCK_SIZE 16
#define BLOCKS_PER_QUAD 4

[numthreads(256, 1, 1)]
[RootSignature(RootSig4c)]
void main(uint3 dtID : SV_DispatchThreadID)
{
    uint totalBlocks = constants.bufferSizeInBytes / BC5_BLOCK_SIZE;
    uint numQuads = totalBlocks / BLOCKS_PER_QUAD;
    uint numStragglers = totalBlocks % BLOCKS_PER_QUAD;

    uint shuffledBufferSizeInBytes = (numQuads * BLOCKS_PER_QUAD * BC5_BLOCK_SIZE);
    uint bufferOffsetInElements = constants.bufferOffsetInBytes / BC5_BLOCK_SIZE;

    uint threadIndex = dtID.x;

    if (threadIndex < numQuads)
    {
        uint firstBlockIndex = (threadIndex * BLOCKS_PER_QUAD);
        uint4 bc5Blocks[4];

        if (constants.transformId == 4 || constants.transformId == 20)
        {
            uint r1Offset           = (shuffledBufferSizeInBytes * 1) / 16;
            uint redIndicesOffset   = (shuffledBufferSizeInBytes * 2) / 16;
            uint g0Offset           = (shuffledBufferSizeInBytes * 8) / 16;
            uint g1Offset           = (shuffledBufferSizeInBytes * 9) / 16;
            uint greenIndicesOffset = (shuffledBufferSizeInBytes * 10) / 16;

            uint r0Quad         = srcBuffer.Load(firstBlockIndex);
            uint r1Quad         = srcBuffer.Load(r1Offset + firstBlockIndex);
            uint3 redIndicesA   = srcBuffer.Load3(redIndicesOffset + firstBlockIndex * 6);
            uint3 redIndicesB   = srcBuffer.Load3(redIndicesOffset + firstBlockIndex * 6 + 12);

            uint g0Quad         = srcBuffer.Load(g0Offset + firstBlockIndex);
            uint g1Quad         = srcBuffer.Load(g1Offset + firstBlockIndex);
            uint3 greenIndicesA = srcBuffer.Load3(greenIndicesOffset + firstBlockIndex * 6);
            uint3 greenIndicesB = srcBuffer.Load3(greenIndicesOffset + firstBlockIndex * 6 + 12);

            bc5Blocks[0].x = ((r0Quad >> 0) & 0xFFu) | (((r1Quad >> 0) & 0xFFu) << 8u) | (redIndicesA.x << 16u);
            bc5Blocks[0].y = (redIndicesA.x >> 16u) | (redIndicesA.y << 16u);
            bc5Blocks[0].z = ((g0Quad >> 0) & 0xFFu) | (((g1Quad >> 0) & 0xFFu) << 8u) | (greenIndicesA.x << 16u);
            bc5Blocks[0].w = (greenIndicesA.x >> 16u) | (greenIndicesA.y << 16u);

            bc5Blocks[1].x = ((r0Quad >> 8) & 0xFFu) | (((r1Quad >> 8) & 0xFFu) << 8u) | (redIndicesA.y & 0xFFFF0000u);
            bc5Blocks[1].y = redIndicesA.z;
            bc5Blocks[1].z = ((g0Quad >> 8) & 0xFFu) | (((g1Quad >> 8) & 0xFFu) << 8u) | (greenIndicesA.y & 0xFFFF0000u);
            bc5Blocks[1].w = greenIndicesA.z;

            bc5Blocks[2].x = ((r0Quad >> 16) & 0xFFu) | (((r1Quad >> 16) & 0xFFu) << 8u) | (redIndicesB.x << 16u);
            bc5Blocks[2].y = (redIndicesB.x >> 16u) | (redIndicesB.y << 16u);
            bc5Blocks[2].z = ((g0Quad >> 16) & 0xFFu) | (((g1Quad >> 16) & 0xFFu) << 8u) | (greenIndicesB.x << 16u);
            bc5Blocks[2].w = (greenIndicesB.x >> 16u) | (greenIndicesB.y << 16u);

            bc5Blocks[3].x = ((r0Quad >> 24) & 0xFFu) | (((r1Quad >> 24) & 0xFFu) << 8u) | (redIndicesB.y & 0xFFFF0000u);
            bc5Blocks[3].y = redIndicesB.z;
            bc5Blocks[3].z = ((g0Quad >> 24) & 0xFFu) | (((g1Quad >> 24) & 0xFFu) << 8u) | (greenIndicesB.y & 0xFFFF0000u);
            bc5Blocks[3].w = greenIndicesB.z;
        }
        else
        {
            bc5Blocks[0] = srcBuffer.Load4(firstBlockIndex * 16);
            bc5Blocks[1] = srcBuffer.Load4(firstBlockIndex * 16 + 16);
            bc5Blocks[2] = srcBuffer.Load4(firstBlockIndex * 16 + 32);
            bc5Blocks[3] = srcBuffer.Load4(firstBlockIndex * 16 + 48);
        }

        if (constants.transformId == 20)
        {
            uint finalBlockID = ReverseSpaceCurveFor16ByteBlock(shuffledBufferSizeInBytes, constants.widthInPixels, threadIndex * BLOCKS_PER_QUAD);
            dstBuffer[bufferOffsetInElements + finalBlockID + 0] = bc5Blocks[0];
            dstBuffer[bufferOffsetInElements + finalBlockID + 1] = bc5Blocks[1];
            dstBuffer[bufferOffsetInElements + finalBlockID + 2] = bc5Blocks[2];
            dstBuffer[bufferOffsetInElements + finalBlockID + 3] = bc5Blocks[3];
        }
        else
        {
            dstBuffer[bufferOffsetInElements + threadIndex * BLOCKS_PER_QUAD + 0] = bc5Blocks[0];
            dstBuffer[bufferOffsetInElements + threadIndex * BLOCKS_PER_QUAD + 1] = bc5Blocks[1];
            dstBuffer[bufferOffsetInElements + threadIndex * BLOCKS_PER_QUAD + 2] = bc5Blocks[2];
            dstBuffer[bufferOffsetInElements + threadIndex * BLOCKS_PER_QUAD + 3] = bc5Blocks[3];
        }
    }
    else if (threadIndex < (numQuads + numStragglers))
    {
        uint stragglerIndex = threadIndex - numQuads;
        uint stragglerByteOffset = shuffledBufferSizeInBytes + (stragglerIndex * BC5_BLOCK_SIZE);

        uint4 oneBC5Block = srcBuffer.Load4(stragglerByteOffset);
        dstBuffer[bufferOffsetInElements + numQuads * BLOCKS_PER_QUAD + stragglerIndex] = oneBC5Block;
    }
}
