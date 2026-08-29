// shaders/GACL/UnshuffleBC1x.hlsl
//--------------------------------------------------------------------------------------
// UnshuffleBC1x.hlsl
//
// Advanced Technology Group (ATG) & GPCK High-Performance Variant
// Copyright (C) Microsoft Corporation / GPCK Contributors. All rights reserved.
//--------------------------------------------------------------------------------------
#include "Shared.hlsli"
#include "SharedDefinitions.hlsli"

ByteAddressBuffer srcBuffer : register(t0);

RWStructuredBuffer<uint2> dstBuffer : register(u0);

struct BC1Info
{
    uint bufferSizeInBytes;
    uint bufferOffsetInBytes;
    uint transformId;
    uint widthInPixels;
};

#if defined(__spirv__) || defined(__SPIRV__)
[[vk::push_constant]] BC1Info constants;
#else
ConstantBuffer<BC1Info> constants : register(b0);
#endif

#define BC1_BLOCK_SIZE 8
#define BLOCKS_PER_PAIR 2

[numthreads(256, 1, 1)]
[RootSignature(RootSig4c)]
void main(uint3 dtID : SV_DispatchThreadID, uint3 gID : SV_GroupID)
{
    uint totalBlocks = constants.bufferSizeInBytes / BC1_BLOCK_SIZE;
    uint numPairs = totalBlocks / BLOCKS_PER_PAIR;
    uint numStragglers = totalBlocks % BLOCKS_PER_PAIR;

    uint shuffledBufferSizeInBytes = (numPairs * BLOCKS_PER_PAIR * BC1_BLOCK_SIZE);
    uint bufferOffsetInElements = constants.bufferOffsetInBytes / BC1_BLOCK_SIZE;

    uint threadIndex = dtID.x;

    if (threadIndex < numPairs)
    {
        uint4 twoBC1Blocks;
        uint firstBlockIndex = threadIndex * BLOCKS_PER_PAIR;

        // Transform 1 & 17: Micro-pattern 1 (Separated e0, e1, indices)
        if (constants.transformId == 1 || constants.transformId == 17)
        {
            uint e1Offset       = (shuffledBufferSizeInBytes / 4);
            uint indicesOffset  = (shuffledBufferSizeInBytes / 2);

            uint e0Pair         = srcBuffer.Load(firstBlockIndex * 2);
            uint e1Pair         = srcBuffer.Load(e1Offset + firstBlockIndex * 2);
            uint2 indicesPair   = srcBuffer.Load2(indicesOffset + firstBlockIndex * 4);

            twoBC1Blocks.x = (e0Pair & 0x0000FFFF) | (e1Pair << 16);
            twoBC1Blocks.y = indicesPair.x;
            twoBC1Blocks.z = (e0Pair >> 16) | (e1Pair & 0xFFFF0000);
            twoBC1Blocks.w = indicesPair.y;
        }
        // Transform 32 & 33: Micro-pattern 2 (5:6:5 High/Low Entropy Split)
        else if (constants.transformId == 32 || constants.transformId == 33)
        {
            uint indicesOffset = (shuffledBufferSizeInBytes / 2);

            uint2 ePairs      = srcBuffer.Load2(firstBlockIndex * 4);
            uint2 indicesPair = srcBuffer.Load2(indicesOffset + firstBlockIndex * 4);

            twoBC1Blocks.x = (((ePairs.x >> 28) & 0xF) << 12) |
                             (((ePairs.x >> 24) & 0xF) << 7)  |
                             (((ePairs.x >> 20) & 0xF) << 1)  |
                             (((ePairs.x >> 16) & 0xF) << 28) |
                             (((ePairs.x >> 12) & 0xF) << 23) |
                             (((ePairs.x >> 8)  & 0xF) << 17) |
                             (((ePairs.x >> 7)  & 0x1) << 11) |
                             (((ePairs.x >> 5)  & 0x3) << 5)  |
                             (((ePairs.x >> 4)  & 0x1) << 0)  |
                             (((ePairs.x >> 3)  & 0x1) << 27) |
                             (((ePairs.x >> 1)  & 0x3) << 21) |
                             (((ePairs.x >> 0)  & 0x1) << 16);

            twoBC1Blocks.y = indicesPair.x;

            twoBC1Blocks.z = (((ePairs.y >> 28) & 0xF) << 12) |
                             (((ePairs.y >> 24) & 0xF) << 7)  |
                             (((ePairs.y >> 20) & 0xF) << 1)  |
                             (((ePairs.y >> 16) & 0xF) << 28) |
                             (((ePairs.y >> 12) & 0xF) << 23) |
                             (((ePairs.y >> 8)  & 0xF) << 17) |
                             (((ePairs.y >> 7)  & 0x1) << 11) |
                             (((ePairs.y >> 5)  & 0x3) << 5)  |
                             (((ePairs.y >> 4)  & 0x1) << 0)  |
                             (((ePairs.y >> 3)  & 0x1) << 27) |
                             (((ePairs.y >> 1)  & 0x3) << 21) |
                             (((ePairs.y >> 0)  & 0x1) << 16);

            twoBC1Blocks.w = indicesPair.y;
        }
        else
        {
            twoBC1Blocks = srcBuffer.Load4(firstBlockIndex * 4);
        }

        // Apply Space Curve Inversion if requested
        if (constants.transformId == 17 || constants.transformId == 33)
        {
            uint finalBlockID = ReverseSpaceCurveFor8ByteBlock(shuffledBufferSizeInBytes, constants.widthInPixels, threadIndex * BLOCKS_PER_PAIR);
            dstBuffer[bufferOffsetInElements + finalBlockID + 0] = twoBC1Blocks.xy;
            dstBuffer[bufferOffsetInElements + finalBlockID + 1] = twoBC1Blocks.zw;
        }
        else
        {
            dstBuffer[bufferOffsetInElements + threadIndex * BLOCKS_PER_PAIR + 0] = twoBC1Blocks.xy;
            dstBuffer[bufferOffsetInElements + threadIndex * BLOCKS_PER_PAIR + 1] = twoBC1Blocks.zw;
        }
    }
    else if (threadIndex < (numPairs + numStragglers))
    {
        uint stragglerIndex = threadIndex - numPairs;
        uint stragglerByteOffset = shuffledBufferSizeInBytes + (stragglerIndex * BC1_BLOCK_SIZE);

        uint2 oneBC1Block = srcBuffer.Load2(stragglerByteOffset);
        dstBuffer[bufferOffsetInElements + numPairs * BLOCKS_PER_PAIR + stragglerIndex] = oneBC1Block;
    }
}
