// shaders/GACL/UnshuffleCurveOnly.hlsl
//--------------------------------------------------------------------------------------
// UnshuffleCurveOnly.hlsl
//
// Advanced Technology Group (ATG) & GPCK High-Performance Variant
// Copyright (C) Microsoft Corporation / GPCK Contributors. All rights reserved.
//--------------------------------------------------------------------------------------
#include "Shared.hlsli"
#include "SharedDefinitions.hlsli"

ByteAddressBuffer srcBuffer : register(t0);

RWStructuredBuffer<uint4> dstBuffer : register(u0);

struct TexInfo
{
    uint bufferSizeInBytes;
    uint bufferOffsetInBytes;
    uint transformId;
    uint widthInPixels;
};

#if defined(__spirv__) || defined(__SPIRV__)
[[vk::push_constant]] TexInfo constants;
#else
ConstantBuffer<TexInfo> constants : register(b0);
#endif

[numthreads(32, 1, 1)]
[RootSignature(RootSig4c)]
void main(uint3 dtID : SV_DispatchThreadID)
{
    uint total16ByteBlocks = constants.bufferSizeInBytes / 16;
    uint bufferOffsetInBlocks = constants.bufferOffsetInBytes / 16;

    uint threadIndex = dtID.x;

    uint4 block16B[32];
    uint baseByteOffset = threadIndex * 512;

    [unroll(32)]
    for (int i = 0; i < 32; ++i)
    {
        block16B[i] = srcBuffer.Load4(baseByteOffset + (i * 16));
    }

    if (constants.transformId == 23)
    {
        uint finalBlockID = ReverseSpaceCurveFor16ByteBlock(constants.bufferSizeInBytes, constants.widthInPixels, threadIndex * 32);

        [unroll(32)]
        for (int j = 0; j < 32; ++j)
        {
            dstBuffer[bufferOffsetInBlocks + finalBlockID + j] = block16B[j];
        }
    }
    else
    {
        [unroll(32)]
        for (int k = 0; k < 32; ++k)
        {
            dstBuffer[bufferOffsetInBlocks + (threadIndex * 32) + k] = block16B[k];
        }
    }
}
