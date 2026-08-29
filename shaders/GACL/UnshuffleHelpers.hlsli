// shaders/GACL/UnshuffleHelpers.hlsli
//--------------------------------------------------------------------------------------
// UnshuffleHelpers.hlsli
//
// Advanced Technology Group (ATG) & GPCK High-Performance Variant
// Copyright (C) Microsoft Corporation / GPCK Contributors. All rights reserved.
//--------------------------------------------------------------------------------------

#include "SharedDefinitions.hlsli"

// Fast Vectorized Byte Unpacker: Loads up to 16 bytes in a single aligned Load4 transaction
inline void loadChunkBytes12(uint byteAddress, in ByteAddressBuffer buffer, out uint b[12], uint count)
{
    uint base = byteAddress & ~3u;
    uint subShift = (byteAddress & 3u);
    uint4 dw = buffer.Load4(base);
    uint w[4] = { dw.x, dw.y, dw.z, dw.w };

    [unroll]
    for (uint i = 0; i < count; ++i)
    {
        uint totalByte = subShift + i;
        b[i] = (w[totalByte >> 2u] >> ((totalByte & 3u) << 3u)) & 0xFFu;
    }
}

inline void loadChunkBytes16(uint byteAddress, in ByteAddressBuffer buffer, out uint b[16], uint count)
{
    uint base = byteAddress & ~3u;
    uint subShift = (byteAddress & 3u);
    uint4 dw0 = buffer.Load4(base);
    uint4 dw1 = (subShift + count > 16u) ? buffer.Load4(base + 16u) : (uint4)0;
    uint w[8] = { dw0.x, dw0.y, dw0.z, dw0.w, dw1.x, dw1.y, dw1.z, dw1.w };

    [unroll]
    for (uint i = 0; i < count; ++i)
    {
        uint totalByte = subShift + i;
        b[i] = (w[totalByte >> 2u] >> ((totalByte & 3u) << 3u)) & 0xFFu;
    }
}

void unshuffleMode0(uint colorAddress, uint miscAddress, uint ScrapData, uint modePattern, in ByteAddressBuffer buffer, inout uint4 raw)
{
    uint colorBytes[12];
    loadChunkBytes12(colorAddress, buffer, colorBytes, 9);

    uint miscBytes[12];
    loadChunkBytes12(miscAddress, buffer, miscBytes, 6);

    uint colorByte0 = colorBytes[0];
    uint colorByte1 = colorBytes[1];
    uint colorByte2 = colorBytes[2];
    uint colorByte3 = colorBytes[3];
    uint colorByte4 = colorBytes[4];
    uint colorByte5 = colorBytes[5];
    uint colorByte6 = colorBytes[6];
    uint colorByte7 = colorBytes[7];
    uint colorByte8 = colorBytes[8];

    uint miscByte0 = miscBytes[0];
    uint miscByte1 = miscBytes[1];
    uint miscByte2 = miscBytes[2];
    uint miscByte3 = miscBytes[3];
    uint miscByte4 = miscBytes[4];
    uint miscByte5 = miscBytes[5];

    WriteBitsToDword(1u, 0, raw[0]);
    WriteBitsToDword(miscByte0 & 0xFu, 1, raw[0]);

    if (modePattern == EndpointPair4bit)
    {
        WriteBitsToDword(colorByte0 & 0xFu,         5,  raw[0]);
        WriteBitsToDword(colorByte0 >> 4u,          9,  raw[0]);
        WriteBitsToDword(colorByte3 & 0xFu,         13, raw[0]);
        WriteBitsToDword(colorByte3 >> 4u,          17, raw[0]);
        WriteBitsToDword(colorByte6 & 0xFu,         21, raw[0]);
        WriteBitsToDword(colorByte6 >> 4u,          25, raw[0]);

        WriteBitsToDword(colorByte1 & 0x7u,         29, raw[0]);
        WriteBitsToDword((colorByte1 >> 3) & 0x1u,  0,  raw[1]);
        WriteBitsToDword(colorByte1 >> 4u,          1,  raw[1]);
        WriteBitsToDword(colorByte4 & 0xFu,         5,  raw[1]);
        WriteBitsToDword(colorByte4 >> 4u,          9,  raw[1]);
        WriteBitsToDword(colorByte7 & 0xFu,         13, raw[1]);
        WriteBitsToDword(colorByte7 >> 4u,          17, raw[1]);

        WriteBitsToDword(colorByte2 & 0xFu,         21, raw[1]);
        WriteBitsToDword(colorByte2 >> 4u,          25, raw[1]);
        WriteBitsToDword(colorByte5 & 0x7u,         29, raw[1]);
        WriteBitsToDword((colorByte5 >> 3) & 0x1u,  0,  raw[2]);
        WriteBitsToDword(colorByte5 >> 4u,          1,  raw[2]);
        WriteBitsToDword(colorByte8 & 0xFu,         5,  raw[2]);
        WriteBitsToDword(colorByte8 >> 4u,          9,  raw[2]);
    }
    else if (modePattern == ColorPlane4bit)
    {
        WriteBitsToDword(colorByte0 & 0xFu,         5,  raw[0]);
        WriteBitsToDword(colorByte0 >> 4u,          9,  raw[0]);
        WriteBitsToDword(colorByte1 & 0xFu,         13, raw[0]);
        WriteBitsToDword(colorByte1 >> 4u,          17, raw[0]);
        WriteBitsToDword(colorByte2 & 0xFu,         21, raw[0]);
        WriteBitsToDword(colorByte2 >> 4u,          25, raw[0]);

        WriteBitsToDword(colorByte3 & 0x7u,         29, raw[0]);
        WriteBitsToDword((colorByte3 >> 3) & 0x1u,  0,  raw[1]);
        WriteBitsToDword(colorByte3 >> 4u,          1,  raw[1]);
        WriteBitsToDword(colorByte4 & 0xFu,         5,  raw[1]);
        WriteBitsToDword(colorByte4 >> 4u,          9,  raw[1]);
        WriteBitsToDword(colorByte5 & 0xFu,         13, raw[1]);
        WriteBitsToDword(colorByte5 >> 4u,          17, raw[1]);

        WriteBitsToDword(colorByte6 & 0xFu,         21, raw[1]);
        WriteBitsToDword(colorByte6 >> 4u,          25, raw[1]);
        WriteBitsToDword(colorByte7 & 0x7u,         29, raw[1]);
        WriteBitsToDword((colorByte7 >> 3) & 0x1u,  0,  raw[2]);
        WriteBitsToDword(colorByte7 >> 4u,          1,  raw[2]);
        WriteBitsToDword(colorByte8 & 0xFu,         5,  raw[2]);
        WriteBitsToDword(colorByte8 >> 4,           9,  raw[2]);
    }
    else if (modePattern == EndpointPairSignificantBitInderleaved)
    {
        WriteBitsToDword(colorByte0 >> 0 & 0x1, 5,  raw[0]);
        WriteBitsToDword(colorByte0 >> 6 & 0x1, 6,  raw[0]);
        WriteBitsToDword(colorByte1 >> 4 & 0x1, 7,  raw[0]);
        WriteBitsToDword(colorByte2 >> 2 & 0x1, 8,  raw[0]);
        WriteBitsToDword(colorByte0 >> 1 & 0x1, 9,  raw[0]);
        WriteBitsToDword(colorByte0 >> 7 & 0x1, 10, raw[0]);
        WriteBitsToDword(colorByte1 >> 5 & 0x1, 11, raw[0]);
        WriteBitsToDword(colorByte2 >> 3 & 0x1, 12, raw[0]);

        WriteBitsToDword(colorByte3 >> 0 & 0x1, 13, raw[0]);
        WriteBitsToDword(colorByte3 >> 6 & 0x1, 14, raw[0]);
        WriteBitsToDword(colorByte4 >> 4 & 0x1, 15, raw[0]);
        WriteBitsToDword(colorByte5 >> 2 & 0x1, 16, raw[0]);
        WriteBitsToDword(colorByte3 >> 1 & 0x1, 17, raw[0]);
        WriteBitsToDword(colorByte3 >> 7 & 0x1, 18, raw[0]);
        WriteBitsToDword(colorByte4 >> 5 & 0x1, 19, raw[0]);
        WriteBitsToDword(colorByte5 >> 3 & 0x1, 20, raw[0]);

        WriteBitsToDword(colorByte6 >> 0 & 0x1, 21, raw[0]);
        WriteBitsToDword(colorByte6 >> 6 & 0x1, 22, raw[0]);
        WriteBitsToDword(colorByte7 >> 4 & 0x1, 23, raw[0]);
        WriteBitsToDword(colorByte8 >> 2 & 0x1, 24, raw[0]);
        WriteBitsToDword(colorByte6 >> 1 & 0x1, 25, raw[0]);
        WriteBitsToDword(colorByte6 >> 7 & 0x1, 26, raw[0]);
        WriteBitsToDword(colorByte7 >> 5 & 0x1, 27, raw[0]);
        WriteBitsToDword(colorByte8 >> 3 & 0x1, 28, raw[0]);

        WriteBitsToDword(colorByte0 >> 2 & 0x1, 29, raw[0]);
        WriteBitsToDword(colorByte1 >> 0 & 0x1, 30, raw[0]);
        WriteBitsToDword(colorByte1 >> 6 & 0x1, 31, raw[0]);
        WriteBitsToDword(colorByte2 >> 4 & 0x1, 0,  raw[1]);
        WriteBitsToDword(colorByte0 >> 3 & 0x1, 1,  raw[1]);
        WriteBitsToDword(colorByte1 >> 1 & 0x1, 2,  raw[1]);
        WriteBitsToDword(colorByte1 >> 7 & 0x1, 3,  raw[1]);
        WriteBitsToDword(colorByte2 >> 5 & 0x1, 4,  raw[1]);

        WriteBitsToDword(colorByte3 >> 2 & 0x1, 5,  raw[1]);
        WriteBitsToDword(colorByte4 >> 0 & 0x1, 6,  raw[1]);
        WriteBitsToDword(colorByte4 >> 6 & 0x1, 7,  raw[1]);
        WriteBitsToDword(colorByte5 >> 4 & 0x1, 8,  raw[1]);
        WriteBitsToDword(colorByte3 >> 3 & 0x1, 9,  raw[1]);
        WriteBitsToDword(colorByte4 >> 1 & 0x1, 10, raw[1]);
        WriteBitsToDword(colorByte4 >> 7 & 0x1, 11, raw[1]);
        WriteBitsToDword(colorByte5 >> 5 & 0x1, 12, raw[1]);

        WriteBitsToDword(colorByte6 >> 2 & 0x1, 13, raw[1]);
        WriteBitsToDword(colorByte7 >> 0 & 0x1, 14, raw[1]);
        WriteBitsToDword(colorByte7 >> 6 & 0x1, 15, raw[1]);
        WriteBitsToDword(colorByte8 >> 4 & 0x1, 16, raw[1]);
        WriteBitsToDword(colorByte6 >> 3 & 0x1, 17, raw[1]);
        WriteBitsToDword(colorByte7 >> 1 & 0x1, 18, raw[1]);
        WriteBitsToDword(colorByte7 >> 7 & 0x1, 19, raw[1]);
        WriteBitsToDword(colorByte8 >> 5 & 0x1, 20, raw[1]);

        WriteBitsToDword(colorByte0 >> 4 & 0x1, 21, raw[1]);
        WriteBitsToDword(colorByte1 >> 2 & 0x1, 22, raw[1]);
        WriteBitsToDword(colorByte2 >> 0 & 0x1, 23, raw[1]);
        WriteBitsToDword(colorByte2 >> 6 & 0x1, 24, raw[1]);
        WriteBitsToDword(colorByte0 >> 5 & 0x1, 25, raw[1]);
        WriteBitsToDword(colorByte1 >> 3 & 0x1, 26, raw[1]);
        WriteBitsToDword(colorByte2 >> 1 & 0x1, 27, raw[1]);
        WriteBitsToDword(colorByte2 >> 7 & 0x1, 28, raw[1]);

        WriteBitsToDword(colorByte3 >> 4 & 0x1, 29, raw[1]);
        WriteBitsToDword(colorByte4 >> 2 & 0x1, 30, raw[1]);
        WriteBitsToDword(colorByte5 >> 0 & 0x1, 31, raw[1]);
        WriteBitsToDword(colorByte5 >> 6 & 0x1, 0,  raw[2]);
        WriteBitsToDword(colorByte3 >> 5 & 0x1, 1,  raw[2]);
        WriteBitsToDword(colorByte4 >> 3 & 0x1, 2,  raw[2]);
        WriteBitsToDword(colorByte5 >> 1 & 0x1, 3,  raw[2]);
        WriteBitsToDword(colorByte5 >> 7 & 0x1, 4,  raw[2]);

        WriteBitsToDword(colorByte6 >> 4 & 0x1, 5,  raw[2]);
        WriteBitsToDword(colorByte7 >> 2 & 0x1, 6,  raw[2]);
        WriteBitsToDword(colorByte8 >> 0 & 0x1, 7,  raw[2]);
        WriteBitsToDword(colorByte8 >> 6 & 0x1, 8,  raw[2]);
        WriteBitsToDword(colorByte6 >> 5 & 0x1, 9,  raw[2]);
        WriteBitsToDword(colorByte7 >> 3 & 0x1, 10, raw[2]);
        WriteBitsToDword(colorByte8 >> 1 & 0x1, 11, raw[2]);
        WriteBitsToDword(colorByte8 >> 7 & 0x1, 12, raw[2]);
    }

    uint PBits = (miscByte0 >> 4) | ((miscByte1 & 0x3u) << 4);
    WriteBitsToDword(PBits,             13, raw[2]);
    WriteBitsToDword(miscByte1 >> 2,    19, raw[2]);
    WriteBitsToDword(miscByte2 & 0x7Fu, 25, raw[2]);
    WriteBitsToDword(miscByte2 >> 7,    0,  raw[3]);
    WriteBitsToDword(miscByte3,         1,  raw[3]);
    WriteBitsToDword(miscByte4,         9,  raw[3]);
    WriteBitsToDword(miscByte5,         17, raw[3]);
    WriteBitsToDword(ScrapData,         25, raw[3]);
}

void unshuffleMode1(uint colorAddress, uint miscAddress, uint ScrapData, uint modePattern, in ByteAddressBuffer buffer, inout uint4 raw)
{
    uint colorBytes[12];
    loadChunkBytes12(colorAddress, buffer, colorBytes, 10);

    uint miscBytes[12];
    loadChunkBytes12(miscAddress, buffer, miscBytes, 6);

    uint colorByte0 = colorBytes[0];
    uint colorByte1 = colorBytes[1];
    uint colorByte2 = colorBytes[2];
    uint colorByte3 = colorBytes[3];
    uint colorByte4 = colorBytes[4];
    uint colorByte5 = colorBytes[5];
    uint colorByte6 = colorBytes[6];
    uint colorByte7 = colorBytes[7];
    uint colorByte8 = colorBytes[8];
    uint colorByte9 = colorBytes[9];

    uint miscByte0 = miscBytes[0];
    uint miscByte1 = miscBytes[1];
    uint miscByte2 = miscBytes[2];
    uint miscByte3 = miscBytes[3];
    uint miscByte4 = miscBytes[4];
    uint miscByte5 = miscBytes[5];

    WriteBitsToDword(2u, 0, raw[0]);

    if (modePattern == EndpointPairSignificantBitInderleaved)
    {
        WriteBitsToDword(colorByte0 & 0xF, 2, raw[0]);
        WriteBitsToDword(colorByte5 & 0x3, 6, raw[0]);

        WriteBitsToDword(colorByte0 >> 4 & 0x1, 8,  raw[0]);
        WriteBitsToDword(colorByte1 >> 2 & 0x1, 9,  raw[0]);
        WriteBitsToDword(colorByte2 >> 0 & 0x1, 10, raw[0]);
        WriteBitsToDword(colorByte2 >> 6 & 0x1, 11, raw[0]);
        WriteBitsToDword(colorByte3 >> 4 & 0x1, 12, raw[0]);
        WriteBitsToDword(colorByte4 >> 2 & 0x1, 13, raw[0]);
        WriteBitsToDword(colorByte0 >> 5 & 0x1, 14, raw[0]);
        WriteBitsToDword(colorByte1 >> 3 & 0x1, 15, raw[0]);
        WriteBitsToDword(colorByte2 >> 1 & 0x1, 16, raw[0]);
        WriteBitsToDword(colorByte2 >> 7 & 0x1, 17, raw[0]);
        WriteBitsToDword(colorByte3 >> 5 & 0x1, 18, raw[0]);
        WriteBitsToDword(colorByte4 >> 3 & 0x1, 19, raw[0]);

        WriteBitsToDword(colorByte5 >> 4 & 0x1, 20, raw[0]);
        WriteBitsToDword(colorByte6 >> 2 & 0x1, 21, raw[0]);
        WriteBitsToDword(colorByte7 >> 0 & 0x1, 22, raw[0]);
        WriteBitsToDword(colorByte7 >> 6 & 0x1, 23, raw[0]);
        WriteBitsToDword(colorByte8 >> 4 & 0x1, 24, raw[0]);
        WriteBitsToDword(colorByte9 >> 2 & 0x1, 25, raw[0]);
        WriteBitsToDword(colorByte5 >> 5 & 0x1, 26, raw[0]);
        WriteBitsToDword(colorByte6 >> 3 & 0x1, 27, raw[0]);
        WriteBitsToDword(colorByte7 >> 1 & 0x1, 28, raw[0]);
        WriteBitsToDword(colorByte7 >> 7 & 0x1, 29, raw[0]);
        WriteBitsToDword(colorByte8 >> 5 & 0x1, 30, raw[0]);
        WriteBitsToDword(colorByte9 >> 3 & 0x1, 31, raw[0]);

        WriteBitsToDword(colorByte0 >> 6 & 0x1, 0,  raw[1]);
        WriteBitsToDword(colorByte1 >> 4 & 0x1, 1,  raw[1]);
        WriteBitsToDword(colorByte2 >> 2 & 0x1, 2,  raw[1]);
        WriteBitsToDword(colorByte3 >> 0 & 0x1, 3,  raw[1]);
        WriteBitsToDword(colorByte3 >> 6 & 0x1, 4,  raw[1]);
        WriteBitsToDword(colorByte4 >> 4 & 0x1, 5,  raw[1]);
        WriteBitsToDword(colorByte0 >> 7 & 0x1, 6,  raw[1]);
        WriteBitsToDword(colorByte1 >> 5 & 0x1, 7,  raw[1]);
        WriteBitsToDword(colorByte2 >> 3 & 0x1, 8,  raw[1]);
        WriteBitsToDword(colorByte3 >> 1 & 0x1, 9,  raw[1]);
        WriteBitsToDword(colorByte3 >> 7 & 0x1, 10, raw[1]);
        WriteBitsToDword(colorByte4 >> 5 & 0x1, 11, raw[1]);

        WriteBitsToDword(colorByte5 >> 6 & 0x1, 12, raw[1]);
        WriteBitsToDword(colorByte6 >> 4 & 0x1, 13, raw[1]);
        WriteBitsToDword(colorByte7 >> 2 & 0x1, 14, raw[1]);
        WriteBitsToDword(colorByte8 >> 0 & 0x1, 15, raw[1]);
        WriteBitsToDword(colorByte8 >> 6 & 0x1, 16, raw[1]);
        WriteBitsToDword(colorByte9 >> 4 & 0x1, 17, raw[1]);
        WriteBitsToDword(colorByte5 >> 7 & 0x1, 18, raw[1]);
        WriteBitsToDword(colorByte6 >> 5 & 0x1, 19, raw[1]);
        WriteBitsToDword(colorByte7 >> 3 & 0x1, 20, raw[1]);
        WriteBitsToDword(colorByte8 >> 1 & 0x1, 21, raw[1]);
        WriteBitsToDword(colorByte8 >> 7 & 0x1, 22, raw[1]);
        WriteBitsToDword(colorByte9 >> 5 & 0x1, 23, raw[1]);

        WriteBitsToDword(colorByte1 >> 0 & 0x1, 24, raw[1]);
        WriteBitsToDword(colorByte1 >> 6 & 0x1, 25, raw[1]);
        WriteBitsToDword(colorByte2 >> 4 & 0x1, 26, raw[1]);
        WriteBitsToDword(colorByte3 >> 2 & 0x1, 27, raw[1]);
        WriteBitsToDword(colorByte4 >> 0 & 0x1, 28, raw[1]);
        WriteBitsToDword(colorByte4 >> 6 & 0x1, 29, raw[1]);
        WriteBitsToDword(colorByte1 >> 1 & 0x1, 30, raw[1]);
        WriteBitsToDword(colorByte1 >> 7 & 0x1, 31, raw[1]);
        WriteBitsToDword(colorByte2 >> 5 & 0x1, 0,  raw[2]);
        WriteBitsToDword(colorByte3 >> 3 & 0x1, 1,  raw[2]);
        WriteBitsToDword(colorByte4 >> 1 & 0x1, 2,  raw[2]);
        WriteBitsToDword(colorByte4 >> 7 & 0x1, 3,  raw[2]);

        WriteBitsToDword(colorByte6 >> 0 & 0x1, 4,  raw[2]);
        WriteBitsToDword(colorByte6 >> 6 & 0x1, 5,  raw[2]);
        WriteBitsToDword(colorByte7 >> 4 & 0x1, 6,  raw[2]);
        WriteBitsToDword(colorByte8 >> 2 & 0x1, 7,  raw[2]);
        WriteBitsToDword(colorByte9 >> 0 & 0x1, 8,  raw[2]);
        WriteBitsToDword(colorByte9 >> 6 & 0x1, 9,  raw[2]);
        WriteBitsToDword(colorByte6 >> 1 & 0x1, 10, raw[2]);
        WriteBitsToDword(colorByte6 >> 7 & 0x1, 11, raw[2]);
        WriteBitsToDword(colorByte7 >> 5 & 0x1, 12, raw[2]);
        WriteBitsToDword(colorByte8 >> 3 & 0x1, 13, raw[2]);
        WriteBitsToDword(colorByte9 >> 1 & 0x1, 14, raw[2]);
        WriteBitsToDword(colorByte9 >> 7 & 0x1, 15, raw[2]);

        WriteBitsToDword(colorByte5 >> 2 & 0x3, 16, raw[2]);
        WriteBitsToDword(miscByte0,             18, raw[2]);
        WriteBitsToDword(miscByte1 & 0x3Fu,     26, raw[2]);
        WriteBitsToDword(miscByte1 >> 6,        0,  raw[3]);
        WriteBitsToDword(miscByte2,             2,  raw[3]);
        WriteBitsToDword(miscByte3,             10, raw[3]);
        WriteBitsToDword(miscByte4,             18, raw[3]);
    }
    else if (modePattern == EndpointQuadSignificantBitInderleaved)
    {
        WriteBitsToDword(miscByte0 & 0x3F, 2, raw[0]);

        WriteBitsToDword(colorByte0 >> 0 & 0x1, 8,  raw[0]);
        WriteBitsToDword(colorByte0 >> 6 & 0x1, 9,  raw[0]);
        WriteBitsToDword(colorByte1 >> 4 & 0x1, 10, raw[0]);
        WriteBitsToDword(colorByte2 >> 2 & 0x1, 11, raw[0]);
        WriteBitsToDword(colorByte3 >> 0 & 0x1, 12, raw[0]);
        WriteBitsToDword(colorByte3 >> 6 & 0x1, 13, raw[0]);
        WriteBitsToDword(colorByte0 >> 1 & 0x1, 14, raw[0]);
        WriteBitsToDword(colorByte0 >> 7 & 0x1, 15, raw[0]);
        WriteBitsToDword(colorByte1 >> 5 & 0x1, 16, raw[0]);
        WriteBitsToDword(colorByte2 >> 3 & 0x1, 17, raw[0]);
        WriteBitsToDword(colorByte3 >> 1 & 0x1, 18, raw[0]);
        WriteBitsToDword(colorByte3 >> 7 & 0x1, 19, raw[0]);

        WriteBitsToDword(colorByte8 >> 6 & 0x1, 20, raw[0]);
        WriteBitsToDword(colorByte8 >> 0 & 0x1, 21, raw[0]);
        WriteBitsToDword(colorByte7 >> 2 & 0x1, 22, raw[0]);
        WriteBitsToDword(colorByte6 >> 4 & 0x1, 23, raw[0]);
        WriteBitsToDword(colorByte5 >> 6 & 0x1, 24, raw[0]);
        WriteBitsToDword(colorByte5 >> 0 & 0x1, 25, raw[0]);
        WriteBitsToDword(colorByte8 >> 7 & 0x1, 26, raw[0]);
        WriteBitsToDword(colorByte8 >> 1 & 0x1, 27, raw[0]);
        WriteBitsToDword(colorByte7 >> 3 & 0x1, 28, raw[0]);
        WriteBitsToDword(colorByte6 >> 5 & 0x1, 29, raw[0]);
        WriteBitsToDword(colorByte5 >> 7 & 0x1, 30, raw[0]);
        WriteBitsToDword(colorByte5 >> 1 & 0x1, 31, raw[0]);

        WriteBitsToDword(colorByte0 >> 2 & 0x1, 0,  raw[1]);
        WriteBitsToDword(colorByte1 >> 0 & 0x1, 1,  raw[1]);
        WriteBitsToDword(colorByte1 >> 6 & 0x1, 2,  raw[1]);
        WriteBitsToDword(colorByte2 >> 4 & 0x1, 3,  raw[1]);
        WriteBitsToDword(colorByte3 >> 2 & 0x1, 4,  raw[1]);
        WriteBitsToDword(colorByte4 >> 0 & 0x1, 5,  raw[1]);
        WriteBitsToDword(colorByte0 >> 3 & 0x1, 6,  raw[1]);
        WriteBitsToDword(colorByte1 >> 1 & 0x1, 7,  raw[1]);
        WriteBitsToDword(colorByte1 >> 7 & 0x1, 8,  raw[1]);
        WriteBitsToDword(colorByte2 >> 5 & 0x1, 9,  raw[1]);
        WriteBitsToDword(colorByte3 >> 3 & 0x1, 10, raw[1]);
        WriteBitsToDword(colorByte4 >> 1 & 0x1, 11, raw[1]);

        WriteBitsToDword(colorByte8 >> 4 & 0x1, 12, raw[1]);
        WriteBitsToDword(colorByte7 >> 6 & 0x1, 13, raw[1]);
        WriteBitsToDword(colorByte7 >> 0 & 0x1, 14, raw[1]);
        WriteBitsToDword(colorByte6 >> 2 & 0x1, 15, raw[1]);
        WriteBitsToDword(colorByte5 >> 4 & 0x1, 16, raw[1]);
        WriteBitsToDword(colorByte4 >> 6 & 0x1, 17, raw[1]);
        WriteBitsToDword(colorByte8 >> 5 & 0x1, 18, raw[1]);
        WriteBitsToDword(colorByte7 >> 7 & 0x1, 19, raw[1]);
        WriteBitsToDword(colorByte7 >> 1 & 0x1, 20, raw[1]);
        WriteBitsToDword(colorByte6 >> 3 & 0x1, 21, raw[1]);
        WriteBitsToDword(colorByte5 >> 5 & 0x1, 22, raw[1]);
        WriteBitsToDword(colorByte4 >> 7 & 0x1, 23, raw[1]);

        WriteBitsToDword(colorByte0 >> 4 & 0x1, 24, raw[1]);
        WriteBitsToDword(colorByte1 >> 2 & 0x1, 25, raw[1]);
        WriteBitsToDword(colorByte2 >> 0 & 0x1, 26, raw[1]);
        WriteBitsToDword(colorByte2 >> 6 & 0x1, 27, raw[1]);
        WriteBitsToDword(colorByte3 >> 4 & 0x1, 28, raw[1]);
        WriteBitsToDword(colorByte4 >> 2 & 0x1, 29, raw[1]);
        WriteBitsToDword(colorByte0 >> 5 & 0x1, 30, raw[1]);
        WriteBitsToDword(colorByte1 >> 3 & 0x1, 31, raw[1]);
        WriteBitsToDword(colorByte2 >> 1 & 0x1, 0,  raw[2]);
        WriteBitsToDword(colorByte2 >> 7 & 0x1, 1,  raw[2]);
        WriteBitsToDword(colorByte3 >> 5 & 0x1, 2,  raw[2]);
        WriteBitsToDword(colorByte4 >> 3 & 0x1, 3,  raw[2]);

        WriteBitsToDword(colorByte8 >> 2 & 0x1, 4,  raw[2]);
        WriteBitsToDword(colorByte7 >> 4 & 0x1, 5,  raw[2]);
        WriteBitsToDword(colorByte6 >> 6 & 0x1, 6,  raw[2]);
        WriteBitsToDword(colorByte6 >> 0 & 0x1, 7,  raw[2]);
        WriteBitsToDword(colorByte5 >> 2 & 0x1, 8,  raw[2]);
        WriteBitsToDword(colorByte4 >> 4 & 0x1, 9,  raw[2]);
        WriteBitsToDword(colorByte8 >> 3 & 0x1, 10, raw[2]);
        WriteBitsToDword(colorByte7 >> 5 & 0x1, 11, raw[2]);
        WriteBitsToDword(colorByte6 >> 7 & 0x1, 12, raw[2]);
        WriteBitsToDword(colorByte6 >> 1 & 0x1, 13, raw[2]);
        WriteBitsToDword(colorByte5 >> 3 & 0x1, 14, raw[2]);
        WriteBitsToDword(colorByte4 >> 5 & 0x1, 15, raw[2]);

        WriteBitsToDword(miscByte0 >> 6,    16, raw[2]);
        WriteBitsToDword(miscByte1,         18, raw[2]);
        WriteBitsToDword(miscByte2 & 0x3Fu, 26, raw[2]);
        WriteBitsToDword(miscByte2 >> 6,    0,  raw[3]);
        WriteBitsToDword(miscByte3,         2,  raw[3]);
        WriteBitsToDword(miscByte4,         10, raw[3]);
        WriteBitsToDword(miscByte5,         18, raw[3]);
    }

    WriteBitsToDword(ScrapData, 26, raw[3]);
}

void unshuffleMode2(uint colorAddress, uint miscAddress, uint ScrapData, uint modePattern, in ByteAddressBuffer buffer, inout uint4 raw)
{
    uint colorBytes[16];
    loadChunkBytes16(colorAddress, buffer, colorBytes, 12);

    uint miscBytes[12];
    loadChunkBytes12(miscAddress, buffer, miscBytes, 6);

    WriteBitsToDword(4u, 0, raw[0]);

    if (modePattern == ColorPlane4bit || modePattern == EndpointPair4bit)
    {
        WriteBitsToDword(miscBytes[2] >> 2, 3, raw[0]);

        if (modePattern == EndpointPair4bit)
        {
            WriteBitsToDword((miscBytes[0]) & 0x1,         9,  raw[0]);
            WriteBitsToDword(colorBytes[0] & 0xF,          10, raw[0]);
            WriteBitsToDword((miscBytes[0] >> 1) & 0x1,    14, raw[0]);
            WriteBitsToDword(colorBytes[0] >> 4,           15, raw[0]);
            WriteBitsToDword((miscBytes[0] >> 2) & 0x1,    19, raw[0]);
            WriteBitsToDword(colorBytes[3] & 0xF,          20, raw[0]);
            WriteBitsToDword((miscBytes[0] >> 3) & 0x1,    24, raw[0]);
            WriteBitsToDword(colorBytes[3] >> 4,           25, raw[0]);
            WriteBitsToDword((miscBytes[0] >> 4) & 0x1,    29, raw[0]);
            WriteBitsToDword(colorBytes[6] & 0x3,          30, raw[0]);
            WriteBitsToDword((colorBytes[6] >> 2) & 0x3,   0,  raw[1]);
            WriteBitsToDword((miscBytes[0] >> 5) & 0x1,    2,  raw[1]);
            WriteBitsToDword(colorBytes[6] >> 4,           3,  raw[1]);

            WriteBitsToDword((miscBytes[0] >> 6) & 0x1,    7,  raw[1]);
            WriteBitsToDword(colorBytes[1] & 0xF,          8,  raw[1]);
            WriteBitsToDword((miscBytes[0] >> 7) & 0x1,    12, raw[1]);
            WriteBitsToDword(colorBytes[1] >> 4,           13, raw[1]);
            WriteBitsToDword((miscBytes[1]) & 0x1,         17, raw[1]);
            WriteBitsToDword(colorBytes[4] & 0xF,          18, raw[1]);
            WriteBitsToDword((miscBytes[1] >> 1) & 0x1,    22, raw[1]);
            WriteBitsToDword(colorBytes[4] >> 4,           23, raw[1]);
            WriteBitsToDword((miscBytes[1] >> 2) & 0x1,    27, raw[1]);
            WriteBitsToDword(colorBytes[7] & 0xF,          28, raw[1]);
            WriteBitsToDword((miscBytes[1] >> 3) & 0x1,    0,  raw[2]);
            WriteBitsToDword(colorBytes[7] >> 4,           1,  raw[2]);

            WriteBitsToDword((miscBytes[1] >> 4) & 0x1,    5,  raw[2]);
            WriteBitsToDword(colorBytes[2] & 0xF,          6,  raw[2]);
            WriteBitsToDword((miscBytes[1] >> 5) & 0x1,    10, raw[2]);
            WriteBitsToDword(colorBytes[2] >> 4,           11, raw[2]);
            WriteBitsToDword((miscBytes[1] >> 6) & 0x1,    15, raw[2]);
            WriteBitsToDword(colorBytes[5] & 0xF,          16, raw[2]);
            WriteBitsToDword((miscBytes[1] >> 7) & 0x1,    20, raw[2]);
            WriteBitsToDword(colorBytes[5] >> 4,           21, raw[2]);
            WriteBitsToDword((miscBytes[2]) & 0x1,         25, raw[2]);
            WriteBitsToDword(colorBytes[8] & 0xF,          26, raw[2]);
            WriteBitsToDword((miscBytes[2] >> 1) & 0x1,    30, raw[2]);
            WriteBitsToDword((colorBytes[8] >> 4) & 0x1,   31, raw[2]);
            WriteBitsToDword((colorBytes[8] >> 5) & 0xF,   0,  raw[3]);
        }
        else
        {
            WriteBitsToDword((miscBytes[0]) & 0x1,         9,  raw[0]);
            WriteBitsToDword(colorBytes[0] & 0xF,          10, raw[0]);
            WriteBitsToDword((miscBytes[0] >> 1) & 0x1,    14, raw[0]);
            WriteBitsToDword(colorBytes[0] >> 4,           15, raw[0]);
            WriteBitsToDword((miscBytes[0] >> 2) & 0x1,    19, raw[0]);
            WriteBitsToDword(colorBytes[1] & 0xF,          20, raw[0]);
            WriteBitsToDword((miscBytes[0] >> 3) & 0x1,    24, raw[0]);
            WriteBitsToDword(colorBytes[1] >> 4,           25, raw[0]);
            WriteBitsToDword((miscBytes[0] >> 4) & 0x1,    29, raw[0]);
            WriteBitsToDword(colorBytes[2] & 0x3,          30, raw[0]);
            WriteBitsToDword((colorBytes[2] >> 2) & 0x3,   0,  raw[1]);
            WriteBitsToDword((miscBytes[0] >> 5) & 0x1,    2,  raw[1]);
            WriteBitsToDword(colorBytes[2] >> 4,           3,  raw[1]);

            WriteBitsToDword((miscBytes[0] >> 6) & 0x1,    7,  raw[1]);
            WriteBitsToDword(colorBytes[3] & 0xF,          8,  raw[1]);
            WriteBitsToDword((miscBytes[0] >> 7) & 0x1,    12, raw[1]);
            WriteBitsToDword(colorBytes[3] >> 4,           13, raw[1]);
            WriteBitsToDword((miscBytes[1]) & 0x1,         17, raw[1]);
            WriteBitsToDword(colorBytes[4] & 0xF,          18, raw[1]);
            WriteBitsToDword((miscBytes[1] >> 1) & 0x1,    22, raw[1]);
            WriteBitsToDword(colorBytes[4] >> 4,           23, raw[1]);
            WriteBitsToDword((miscBytes[1] >> 2) & 0x1,    27, raw[1]);
            WriteBitsToDword(colorBytes[5] & 0xF,          28, raw[1]);
            WriteBitsToDword((miscBytes[1] >> 3) & 0x1,    0,  raw[2]);
            WriteBitsToDword(colorBytes[5] >> 4,           1,  raw[2]);

            WriteBitsToDword((miscBytes[1] >> 4) & 0x1,    5,  raw[2]);
            WriteBitsToDword(colorBytes[6] & 0xF,          6,  raw[2]);
            WriteBitsToDword((miscBytes[1] >> 5) & 0x1,    10, raw[2]);
            WriteBitsToDword(colorBytes[6] >> 4,           11, raw[2]);
            WriteBitsToDword((miscBytes[1] >> 6) & 0x1,    15, raw[2]);
            WriteBitsToDword(colorBytes[7] & 0xF,          16, raw[2]);
            WriteBitsToDword((miscBytes[1] >> 7) & 0x1,    20, raw[2]);
            WriteBitsToDword(colorBytes[7] >> 4,           21, raw[2]);
            WriteBitsToDword((miscBytes[2]) & 0x1,         25, raw[2]);
            WriteBitsToDword(colorBytes[8] & 0xF,          26, raw[2]);
            WriteBitsToDword((miscBytes[2] >> 1) & 0x1,    30, raw[2]);
            WriteBitsToDword((colorBytes[8] >> 4) & 0x1,   31, raw[2]);
            WriteBitsToDword((colorBytes[8] >> 5) & 0xF,   0,  raw[3]);
        }

        WriteBitsToDword(miscBytes[3], 3,  raw[3]);
        WriteBitsToDword(miscBytes[4], 11, raw[3]);
        WriteBitsToDword(miscBytes[5], 19, raw[3]);
    }
    else if (modePattern == EndpointPairSignificantBitInderleaved)
    {
        WriteBitsToDword(colorBytes[0] & 0x3, 3, raw[0]);
        WriteBitsToDword(colorBytes[4] & 0x3, 5, raw[0]);
        WriteBitsToDword(colorBytes[8] & 0x3, 7, raw[0]);

        WriteBitsToDword((colorBytes[0] >> 2) & 0x1, 9,  raw[0]);
        WriteBitsToDword((colorBytes[1] >> 0) & 0x1, 10, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 6) & 0x1, 11, raw[0]);
        WriteBitsToDword((colorBytes[2] >> 4) & 0x1, 12, raw[0]);
        WriteBitsToDword((colorBytes[3] >> 2) & 0x1, 13, raw[0]);

        WriteBitsToDword((colorBytes[0] >> 3) & 0x1, 14, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 1) & 0x1, 15, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 7) & 0x1, 16, raw[0]);
        WriteBitsToDword((colorBytes[2] >> 5) & 0x1, 17, raw[0]);
        WriteBitsToDword((colorBytes[3] >> 3) & 0x1, 18, raw[0]);

        WriteBitsToDword((colorBytes[4] >> 2) & 0x1, 19, raw[0]);
        WriteBitsToDword((colorBytes[5] >> 0) & 0x1, 20, raw[0]);
        WriteBitsToDword((colorBytes[5] >> 6) & 0x1, 21, raw[0]);
        WriteBitsToDword((colorBytes[6] >> 4) & 0x1, 22, raw[0]);
        WriteBitsToDword((colorBytes[7] >> 2) & 0x1, 23, raw[0]);

        WriteBitsToDword((colorBytes[4] >> 3) & 0x1, 24, raw[0]);
        WriteBitsToDword((colorBytes[5] >> 1) & 0x1, 25, raw[0]);
        WriteBitsToDword((colorBytes[5] >> 7) & 0x1, 26, raw[0]);
        WriteBitsToDword((colorBytes[6] >> 5) & 0x1, 27, raw[0]);
        WriteBitsToDword((colorBytes[7] >> 3) & 0x1, 28, raw[0]);

        WriteBitsToDword((colorBytes[8] >> 2) & 0x1,  29, raw[0]);
        WriteBitsToDword((colorBytes[9] >> 0) & 0x1,  30, raw[0]);
        WriteBitsToDword((colorBytes[9] >> 6) & 0x1,  31, raw[0]);
        WriteBitsToDword((colorBytes[10] >> 4) & 0x1, 0,  raw[1]);
        WriteBitsToDword((colorBytes[11] >> 2) & 0x1, 1,  raw[1]);

        WriteBitsToDword((colorBytes[8] >> 3) & 0x1,  2, raw[1]);
        WriteBitsToDword((colorBytes[9] >> 1) & 0x1,  3, raw[1]);
        WriteBitsToDword((colorBytes[9] >> 7) & 0x1,  4, raw[1]);
        WriteBitsToDword((colorBytes[10] >> 5) & 0x1, 5, raw[1]);
        WriteBitsToDword((colorBytes[11] >> 3) & 0x1, 6, raw[1]);

        WriteBitsToDword((colorBytes[0] >> 4) & 0x1, 7,  raw[1]);
        WriteBitsToDword((colorBytes[1] >> 2) & 0x1, 8,  raw[1]);
        WriteBitsToDword((colorBytes[2] >> 0) & 0x1, 9,  raw[1]);
        WriteBitsToDword((colorBytes[2] >> 6) & 0x1, 10, raw[1]);
        WriteBitsToDword((colorBytes[3] >> 4) & 0x1, 11, raw[1]);

        WriteBitsToDword((colorBytes[0] >> 5) & 0x1, 12, raw[1]);
        WriteBitsToDword((colorBytes[1] >> 3) & 0x1, 13, raw[1]);
        WriteBitsToDword((colorBytes[2] >> 1) & 0x1, 14, raw[1]);
        WriteBitsToDword((colorBytes[2] >> 7) & 0x1, 15, raw[1]);
        WriteBitsToDword((colorBytes[3] >> 5) & 0x1, 16, raw[1]);

        WriteBitsToDword((colorBytes[4] >> 4) & 0x1, 17, raw[1]);
        WriteBitsToDword((colorBytes[5] >> 2) & 0x1, 18, raw[1]);
        WriteBitsToDword((colorBytes[6] >> 0) & 0x1, 19, raw[1]);
        WriteBitsToDword((colorBytes[6] >> 6) & 0x1, 20, raw[1]);
        WriteBitsToDword((colorBytes[7] >> 4) & 0x1, 21, raw[1]);

        WriteBitsToDword((colorBytes[4] >> 5) & 0x1, 22, raw[1]);
        WriteBitsToDword((colorBytes[5] >> 3) & 0x1, 23, raw[1]);
        WriteBitsToDword((colorBytes[6] >> 1) & 0x1, 24, raw[1]);
        WriteBitsToDword((colorBytes[6] >> 7) & 0x1, 25, raw[1]);
        WriteBitsToDword((colorBytes[7] >> 5) & 0x1, 26, raw[1]);

        WriteBitsToDword((colorBytes[8] >> 4) & 0x1,  27, raw[1]);
        WriteBitsToDword((colorBytes[9] >> 2) & 0x1,  28, raw[1]);
        WriteBitsToDword((colorBytes[10] >> 0) & 0x1, 29, raw[1]);
        WriteBitsToDword((colorBytes[10] >> 6) & 0x1, 30, raw[1]);
        WriteBitsToDword((colorBytes[11] >> 4) & 0x1, 31, raw[1]);

        WriteBitsToDword((colorBytes[8] >> 5) & 0x1,  0, raw[2]);
        WriteBitsToDword((colorBytes[9] >> 3) & 0x1,  1, raw[2]);
        WriteBitsToDword((colorBytes[10] >> 1) & 0x1, 2, raw[2]);
        WriteBitsToDword((colorBytes[10] >> 7) & 0x1, 3, raw[2]);
        WriteBitsToDword((colorBytes[11] >> 5) & 0x1, 4, raw[2]);

        WriteBitsToDword((colorBytes[0] >> 6) & 0x1, 5, raw[2]);
        WriteBitsToDword((colorBytes[1] >> 4) & 0x1, 6, raw[2]);
        WriteBitsToDword((colorBytes[2] >> 2) & 0x1, 7, raw[2]);
        WriteBitsToDword((colorBytes[3] >> 0) & 0x1, 8, raw[2]);
        WriteBitsToDword((colorBytes[3] >> 6) & 0x1, 9, raw[2]);

        WriteBitsToDword((colorBytes[0] >> 7) & 0x1, 10, raw[2]);
        WriteBitsToDword((colorBytes[1] >> 5) & 0x1, 11, raw[2]);
        WriteBitsToDword((colorBytes[2] >> 3) & 0x1, 12, raw[2]);
        WriteBitsToDword((colorBytes[3] >> 1) & 0x1, 13, raw[2]);
        WriteBitsToDword((colorBytes[3] >> 7) & 0x1, 14, raw[2]);

        WriteBitsToDword((colorBytes[4] >> 6) & 0x1, 15, raw[2]);
        WriteBitsToDword((colorBytes[5] >> 4) & 0x1, 16, raw[2]);
        WriteBitsToDword((colorBytes[6] >> 2) & 0x1, 17, raw[2]);
        WriteBitsToDword((colorBytes[7] >> 0) & 0x1, 18, raw[2]);
        WriteBitsToDword((colorBytes[7] >> 6) & 0x1, 19, raw[2]);

        WriteBitsToDword((colorBytes[4] >> 7) & 0x1, 20, raw[2]);
        WriteBitsToDword((colorBytes[5] >> 5) & 0x1, 21, raw[2]);
        WriteBitsToDword((colorBytes[6] >> 3) & 0x1, 22, raw[2]);
        WriteBitsToDword((colorBytes[7] >> 1) & 0x1, 23, raw[2]);
        WriteBitsToDword((colorBytes[7] >> 7) & 0x1, 24, raw[2]);

        WriteBitsToDword((colorBytes[8] >> 6) & 0x1,  25, raw[2]);
        WriteBitsToDword((colorBytes[9] >> 4) & 0x1,  26, raw[2]);
        WriteBitsToDword((colorBytes[10] >> 2) & 0x1, 27, raw[2]);
        WriteBitsToDword((colorBytes[11] >> 0) & 0x1, 28, raw[2]);
        WriteBitsToDword((colorBytes[11] >> 6) & 0x1, 29, raw[2]);

        WriteBitsToDword((colorBytes[8] >> 7) & 0x1,  30, raw[2]);
        WriteBitsToDword((colorBytes[9] >> 5) & 0x1,  31, raw[2]);
        WriteBitsToDword((colorBytes[10] >> 3) & 0x1, 0,  raw[3]);
        WriteBitsToDword((colorBytes[11] >> 1) & 0x1, 1,  raw[3]);
        WriteBitsToDword((colorBytes[11] >> 7) & 0x1, 2,  raw[3]);

        WriteBitsToDword(miscBytes[0] & 0x7, 3,  raw[3]);
        WriteBitsToDword(miscBytes[0] >> 3,  6,  raw[3]);
        WriteBitsToDword(miscBytes[1],       11, raw[3]);
        WriteBitsToDword(miscBytes[2],       19, raw[3]);
    }

    WriteBitsToDword(ScrapData, 27, raw[3]);
}

void unshuffleMode3(uint colorAddress, uint miscAddress, uint ScrapData, uint modePattern, in ByteAddressBuffer buffer, inout uint4 raw)
{
    uint colorBytes[16];
    loadChunkBytes16(colorAddress, buffer, colorBytes, 12);

    uint miscBytes[12];
    loadChunkBytes12(miscAddress, buffer, miscBytes, 4);

    const uint kChannels = 6;
    uint dwordIndex = 0;
    uint dwordPosition = 10;

    WriteBitsToDword(8u, 0, raw[0]);

    if (modePattern == EndpointQuadSignificantBitInderleaved)
    {
        WriteBitsToDword(miscBytes[0] & 0x3F, 4, raw[0]);

        uint base = 2;
        for (uint i = 0; i < 2; ++i)
        {
            for (uint j = 0; j < 7; ++j)
            {
                if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
                uint colorTotalIndex = (base + i) + (kChannels * j);
                WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            }
        }

        base = 84;
        for (uint i = 0; i < 2; ++i)
        {
            for (uint j = 0; j < 7; ++j)
            {
                if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
                uint colorTotalIndex = (base + i) - (kChannels * j);
                WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            }
        }

        base = 4;
        for (uint i = 0; i < 2; ++i)
        {
            for (uint j = 0; j < 7; ++j)
            {
                if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
                uint colorTotalIndex = (base + i) + (kChannels * j);
                WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            }
        }

        base = 82;
        for (uint i = 0; i < 2; ++i)
        {
            for (uint j = 0; j < 7; ++j)
            {
                if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
                uint colorTotalIndex = (base + i) - (kChannels * j);
                WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            }
        }

        base = 6;
        for (uint i = 0; i < 2; ++i)
        {
            for (uint j = 0; j < 7; ++j)
            {
                if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
                uint colorTotalIndex = (base + i) + (kChannels * j);
                WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            }
        }

        base = 80;
        for (uint i = 0; i < 2; ++i)
        {
            for (uint j = 0; j < 7; ++j)
            {
                if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
                uint colorTotalIndex = (base + i) - (kChannels * j);
                WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            }
        }

        WriteBitsToDword(colorBytes[0] & 0x3, 30, raw[2]);
        WriteBitsToDword(colorBytes[10] >> 6, 0,  raw[3]);

        WriteBitsToDword(miscBytes[0] >> 6, 2,  raw[3]);
        WriteBitsToDword(miscBytes[1],      4,  raw[3]);
        WriteBitsToDword(miscBytes[2],      12, raw[3]);
        WriteBitsToDword(miscBytes[3],      20, raw[3]);
    }
    else if (modePattern == EndpointPairSignificantBitInderleaved)
    {
        WriteBitsToDword(colorBytes[0] & 0x3F, 4, raw[0]);

        uint base = 6;
        for (uint i = 0; i < 2; ++i)
        {
            for (uint j = 0; j < 7; ++j)
            {
                if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
                uint colorTotalIndex = (base + i) + (kChannels * j);
                WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            }
        }

        base = 54;
        for (uint i = 0; i < 2; ++i)
        {
            for (uint j = 0; j < 7; ++j)
            {
                if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
                uint colorTotalIndex = (base + i) + (kChannels * j);
                WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            }
        }

        base = 8;
        for (uint i = 0; i < 2; ++i)
        {
            for (uint j = 0; j < 7; ++j)
            {
                if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
                uint colorTotalIndex = (base + i) + (kChannels * j);
                WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            }
        }

        base = 56;
        for (uint i = 0; i < 2; ++i)
        {
            for (uint j = 0; j < 7; ++j)
            {
                if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
                uint colorTotalIndex = (base + i) + (kChannels * j);
                WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            }
        }

        base = 10;
        for (uint i = 0; i < 2; ++i)
        {
            for (uint j = 0; j < 7; ++j)
            {
                if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
                uint colorTotalIndex = (base + i) + (kChannels * j);
                WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            }
        }

        base = 58;
        for (uint i = 0; i < 2; ++i)
        {
            for (uint j = 0; j < 7; ++j)
            {
                if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
                uint colorTotalIndex = (base + i) + (kChannels * j);
                WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            }
        }

        WriteBitsToDword(colorBytes[6] & 0x3,        30, raw[2]);
        WriteBitsToDword((colorBytes[6] >> 2) & 0xF, 0,  raw[3]);

        WriteBitsToDword(miscBytes[0], 4,  raw[3]);
        WriteBitsToDword(miscBytes[1], 12, raw[3]);
        WriteBitsToDword(miscBytes[2], 20, raw[3]);
    }

    WriteBitsToDword(ScrapData, 28, raw[3]);
}

static uint M4StaticBitStarts[9] =
{
    16, 16, 16, 16, 8, 8, 8, 0, 0
};

static uint M4OtherByteOrder[9][5] =
{
    { 2, 3, 1, 4, 0 },
    { 2, 3, 1, 4, 0 },
    { 2, 3, 1, 4, 0 },
    { 2, 3, 1, 4, 0 },
    { 1, 2, 3, 0, 4 },
    { 1, 2, 3, 4, 0 },
    { 1, 2, 3, 4, 0 },
    { 0, 1, 2, 3, 4 },
    { 0, 1, 2, 3, 4 }
};

void unshuffleMode4(uint colorAddress, uint miscAddress, uint ScrapData, uint modePattern, uint modeStatics, uint lowEntropy, in ByteAddressBuffer buffer, inout uint4 raw)
{
    uint colorBytes[12];
    loadChunkBytes12(colorAddress, buffer, colorBytes, 5);

    uint miscBytes[12];
    loadChunkBytes12(miscAddress, buffer, miscBytes, 10);

    WriteBitsToDword(16u, 0, raw[0]);
    WriteBitsToDword(miscBytes[3] >> 7, 5, raw[0]);
    WriteBitsToDword(ScrapData & 0x1,   6, raw[0]);
    WriteBitsToDword(miscBytes[9] >> 7, 7, raw[0]);

    if (modePattern == StableIsland)
    {
        uint order[8] = { 6, 7, 4, 5, 2, 3, 0, 1 };

        for (uint ep = 0; ep < 8; ++ep)
        {
            if ((modeStatics | lowEntropy) & (1u << ep))
            {
                for (uint ee = 0; ee < 8; ee++)
                {
                    if (order[ee] > order[ep]) order[ee]--;
                }
                order[ep] = 0;
            }
        }

        uint staticFields = countbits(modeStatics | lowEntropy);
        uint staticBits = staticFields * 5;
        uint otherFields = 8 - staticFields;

        uint nextStaticBit = M4StaticBitStarts[staticFields];
        uint dwordIndex = 0;
        uint dwordPosition = 8;

        for (uint ep = 0; ep < 8; ep++)
        {
            if ((modeStatics | lowEntropy) & (1u << ep))
            {
                uint colorByteIndex = nextStaticBit / 8;
                uint colorBitIndex = nextStaticBit % 8;
                uint intermediateEndpoint = 0u;

                if (colorBitIndex > 3)
                {
                    uint lowBits = 8 - colorBitIndex;
                    uint highBits = 5 - lowBits;
                    uint mask = (~0u) >> (32 - lowBits);
                    WriteBitsToDword((colorBytes[colorByteIndex] >> colorBitIndex) & mask, 0, intermediateEndpoint);

                    mask = (~0u) >> (32 - highBits);
                    WriteBitsToDword(colorBytes[colorByteIndex + 1] & mask, lowBits, intermediateEndpoint);
                }
                else
                {
                    WriteBitsToDword((colorBytes[colorByteIndex] >> colorBitIndex) & 0x1Fu, 0, intermediateEndpoint);
                }

                if (32 - dwordPosition < 5)
                {
                    uint bitsToRaw0 = 32 - dwordPosition;
                    uint bitsToRaw1 = 5 - bitsToRaw0;
                    uint mask = (~0u) >> (32 - bitsToRaw0);
                    WriteBitsToDword(intermediateEndpoint & mask, dwordPosition, raw[dwordIndex]);
                    dwordIndex += 1;
                    dwordPosition = 0;
                    WriteBitsToDword(intermediateEndpoint >> bitsToRaw0, dwordPosition, raw[dwordIndex]);
                    dwordPosition += bitsToRaw1;
                }
                else
                {
                    WriteBitsToDword(intermediateEndpoint & 0x1Fu, dwordPosition, raw[dwordIndex]);
                    dwordPosition += 5;
                }

                nextStaticBit += 5;
            }
            else
            {
                uint intermediateEndpoint = 0u;
                uint intPosition = 0;
                for (uint b = 0; b < 5; ++b)
                {
                    const uint bi = 4 - b;
                    const uint destLinear = staticBits + order[ep] + (bi * otherFields);
                    const uint destLinearByte = destLinear / 8;
                    const uint destLinearBit = destLinear % 8;
                    const uint destRemappedByte = M4OtherByteOrder[staticFields][destLinearByte];
                    const uint destRemapped = 8 * destRemappedByte + destLinearBit;

                    uint colorByteIndex = destRemapped / 8;
                    uint colorBitIndex = destRemapped % 8;

                    WriteBitsToDword((colorBytes[colorByteIndex] >> colorBitIndex) & 0x1u, intPosition, intermediateEndpoint);
                    intPosition += 1;
                }

                if (32 - dwordPosition < 5)
                {
                    uint firstHalf = 32 - dwordPosition;
                    uint secondHalf = 5 - firstHalf;
                    uint mask = (~0u) >> (32 - firstHalf);
                    WriteBitsToDword(intermediateEndpoint & mask, dwordPosition, raw[dwordIndex]);
                    dwordIndex++;
                    dwordPosition = 0;
                    WriteBitsToDword(intermediateEndpoint >> firstHalf, dwordPosition, raw[dwordIndex]);
                    dwordPosition += secondHalf;
                }
                else
                {
                    WriteBitsToDword(intermediateEndpoint & 0x1Fu, dwordPosition, raw[dwordIndex]);
                    dwordPosition += 5;
                }
            }
        }
    }
    else if (modePattern == EndpointPairSignificantBitInderleaved)
    {
        WriteBitsToDword((colorBytes[0]) & 0x1, 8,  raw[0]);
        WriteBitsToDword((colorBytes[1]) & 0x1, 9,  raw[0]);
        WriteBitsToDword((colorBytes[2]) & 0x1, 10, raw[0]);
        WriteBitsToDword((colorBytes[3]) & 0x1, 11, raw[0]);
        WriteBitsToDword((colorBytes[4]) & 0x1, 12, raw[0]);

        WriteBitsToDword((colorBytes[0] >> 1) & 0x1, 13, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 1) & 0x1, 14, raw[0]);
        WriteBitsToDword((colorBytes[2] >> 1) & 0x1, 15, raw[0]);
        WriteBitsToDword((colorBytes[3] >> 1) & 0x1, 16, raw[0]);
        WriteBitsToDword((colorBytes[4] >> 1) & 0x1, 17, raw[0]);

        WriteBitsToDword((colorBytes[0] >> 2) & 0x1, 18, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 2) & 0x1, 19, raw[0]);
        WriteBitsToDword((colorBytes[2] >> 2) & 0x1, 20, raw[0]);
        WriteBitsToDword((colorBytes[3] >> 2) & 0x1, 21, raw[0]);
        WriteBitsToDword((colorBytes[4] >> 2) & 0x1, 22, raw[0]);

        WriteBitsToDword((colorBytes[0] >> 3) & 0x1, 23, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 3) & 0x1, 24, raw[0]);
        WriteBitsToDword((colorBytes[2] >> 3) & 0x1, 25, raw[0]);
        WriteBitsToDword((colorBytes[3] >> 3) & 0x1, 26, raw[0]);
        WriteBitsToDword((colorBytes[4] >> 3) & 0x1, 27, raw[0]);

        WriteBitsToDword((colorBytes[0] >> 4) & 0x1, 28, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 4) & 0x1, 29, raw[0]);
        WriteBitsToDword((colorBytes[2] >> 4) & 0x1, 30, raw[0]);
        WriteBitsToDword((colorBytes[3] >> 4) & 0x1, 31, raw[0]);
        WriteBitsToDword((colorBytes[4] >> 4) & 0x1, 0,  raw[1]);

        WriteBitsToDword((colorBytes[0] >> 5) & 0x1, 1, raw[1]);
        WriteBitsToDword((colorBytes[1] >> 5) & 0x1, 2, raw[1]);
        WriteBitsToDword((colorBytes[2] >> 5) & 0x1, 3, raw[1]);
        WriteBitsToDword((colorBytes[3] >> 5) & 0x1, 4, raw[1]);
        WriteBitsToDword((colorBytes[4] >> 5) & 0x1, 5, raw[1]);

        WriteBitsToDword((colorBytes[0] >> 6) & 0x1, 6,  raw[1]);
        WriteBitsToDword((colorBytes[1] >> 6) & 0x1, 7,  raw[1]);
        WriteBitsToDword((colorBytes[2] >> 6) & 0x1, 8,  raw[1]);
        WriteBitsToDword((colorBytes[3] >> 6) & 0x1, 9,  raw[1]);
        WriteBitsToDword((colorBytes[4] >> 6) & 0x1, 10, raw[1]);

        WriteBitsToDword((colorBytes[0] >> 7) & 0x1, 11, raw[1]);
        WriteBitsToDword((colorBytes[1] >> 7) & 0x1, 12, raw[1]);
        WriteBitsToDword((colorBytes[2] >> 7) & 0x1, 13, raw[1]);
        WriteBitsToDword((colorBytes[3] >> 7) & 0x1, 14, raw[1]);
        WriteBitsToDword((colorBytes[4] >> 7) & 0x1, 15, raw[1]);
    }

    raw[1] = raw[1] & 0x0003FFFF;

    WriteBitsToDword(miscBytes[0],        18, raw[1]);
    WriteBitsToDword(miscBytes[1] & 0x3F, 26, raw[1]);
    WriteBitsToDword(miscBytes[1] >> 6,   0,  raw[2]);
    WriteBitsToDword(miscBytes[2],        2,  raw[2]);
    WriteBitsToDword(miscBytes[3] & 0x7F, 10, raw[2]);

    WriteBitsToDword(miscBytes[4],        17, raw[2]);
    WriteBitsToDword(miscBytes[5] & 0x7F, 25, raw[2]);
    WriteBitsToDword(miscBytes[5] >> 7,   0,  raw[3]);
    WriteBitsToDword(miscBytes[6],        1,  raw[3]);
    WriteBitsToDword(miscBytes[7],        9,  raw[3]);
    WriteBitsToDword(miscBytes[8],        17, raw[3]);
    WriteBitsToDword(miscBytes[9] & 0x7F, 25, raw[3]);
}

static uint staticBitStarts[9] =
{
    24, 24, 24, 16, 16, 8, 8, 0, 0
};

static uint otherByteOrder[9][7] =
{
    { 3, 4, 2, 5, 1, 6, 0 },
    { 3, 4, 2, 5, 1, 6, 0 },
    { 3, 4, 2, 5, 1, 6, 0 },
    { 2, 3, 4, 1, 5, 0, 6 },
    { 2, 3, 4, 5, 1, 6, 0 },
    { 1, 2, 3, 4, 5, 0, 6 },
    { 1, 2, 3, 4, 5, 6, 0 },
    { 0, 1, 2, 3, 4, 5, 6 },
    { 0, 1, 2, 3, 4, 5, 6 }
};

void unshuffleMode5(uint colorAddress, uint miscAddress, uint ScrapData, uint modePattern, uint modeStatics, uint lowEntropy, in ByteAddressBuffer buffer, inout uint4 raw)
{
    uint colorBytes[12];
    loadChunkBytes12(colorAddress, buffer, colorBytes, 7);

    uint miscBytes[12];
    loadChunkBytes12(miscAddress, buffer, miscBytes, 8);

    WriteBitsToDword(32u, 0, raw[0]);
    WriteBitsToDword(miscBytes[3] >> 7, 6, raw[0]);
    WriteBitsToDword(miscBytes[7] >> 7, 7, raw[0]);

    if (modePattern == StableIsland)
    {
        uint order[8] = { 6, 7, 4, 5, 2, 3, 0, 1 };

        for (uint ep = 0; ep < 8; ++ep)
        {
            if ((modeStatics | lowEntropy) & (1u << ep))
            {
                for (uint ee = 0; ee < 8; ee++)
                {
                    if (order[ee] > order[ep]) order[ee]--;
                }
                order[ep] = 0;
            }
        }

        uint staticFields = countbits(modeStatics | lowEntropy);
        uint staticBits = staticFields * 7;
        uint otherFields = 8 - staticFields;

        uint nextStaticBit = staticBitStarts[staticFields];
        uint dwordIndex = 0;
        uint dwordPosition = 8;

        for (uint ep = 0; ep < 8; ep++)
        {
            if ((modeStatics | lowEntropy) & (1u << ep))
            {
                uint colorByteIndex = nextStaticBit / 8;
                uint colorBitIndex = nextStaticBit % 8;
                uint intermediateEndpoint = 0u;

                if (colorBitIndex > 1)
                {
                    uint lowBits = 8 - colorBitIndex;
                    uint highBits = 7 - lowBits;
                    uint mask = (~0u) >> (32 - lowBits);
                    WriteBitsToDword((colorBytes[colorByteIndex] >> colorBitIndex) & mask, 0, intermediateEndpoint);

                    mask = (~0u) >> (32 - highBits);
                    WriteBitsToDword(colorBytes[colorByteIndex + 1] & mask, lowBits, intermediateEndpoint);
                }
                else
                {
                    WriteBitsToDword((colorBytes[colorByteIndex] >> colorBitIndex) & 0x7Fu, 0, intermediateEndpoint);
                }

                if (32 - dwordPosition < 7)
                {
                    uint bitsToRaw0 = 32 - dwordPosition;
                    uint bitsToRaw1 = 7 - bitsToRaw0;
                    uint mask = (~0u) >> (32 - bitsToRaw0);
                    WriteBitsToDword(intermediateEndpoint & mask, dwordPosition, raw[dwordIndex]);
                    dwordIndex += 1;
                    dwordPosition = 0;
                    WriteBitsToDword(intermediateEndpoint >> bitsToRaw0, dwordPosition, raw[dwordIndex]);
                    dwordPosition += bitsToRaw1;
                }
                else
                {
                    WriteBitsToDword(intermediateEndpoint & 0x7Fu, dwordPosition, raw[dwordIndex]);
                    dwordPosition += 7;
                }

                nextStaticBit += 7;
            }
            else
            {
                uint intermediateEndpoint = 0u;
                uint intPosition = 0;
                for (uint b = 0; b < 7; ++b)
                {
                    const uint bi = 6 - b;
                    const uint destLinear = staticBits + order[ep] + (bi * otherFields);
                    const uint destLinearByte = destLinear / 8;
                    const uint destLinearBit = destLinear % 8;
                    const uint destRemappedByte = otherByteOrder[staticFields][destLinearByte];
                    const uint destRemapped = 8 * destRemappedByte + destLinearBit;

                    uint colorByteIndex = destRemapped / 8;
                    uint colorBitIndex = destRemapped % 8;

                    WriteBitsToDword((colorBytes[colorByteIndex] >> colorBitIndex) & 0x1u, intPosition, intermediateEndpoint);
                    intPosition += 1;
                }

                if (32 - dwordPosition < 7)
                {
                    uint firstHalf = 32 - dwordPosition;
                    uint secondHalf = 7 - firstHalf;
                    uint mask = (~0u) >> (32 - firstHalf);
                    WriteBitsToDword(intermediateEndpoint & mask, dwordPosition, raw[dwordIndex]);
                    dwordIndex++;
                    dwordPosition = 0;
                    WriteBitsToDword(intermediateEndpoint >> firstHalf, dwordPosition, raw[dwordIndex]);
                    dwordPosition += secondHalf;
                }
                else
                {
                    WriteBitsToDword(intermediateEndpoint & 0x7Fu, dwordPosition, raw[dwordIndex]);
                    dwordPosition += 7;
                }
            }
        }
    }
    else if (modePattern == EndpointPairSignificantBitInderleaved)
    {
        WriteBitsToDword((colorBytes[0]) & 0x1, 8,  raw[0]);
        WriteBitsToDword((colorBytes[1]) & 0x1, 9,  raw[0]);
        WriteBitsToDword((colorBytes[2]) & 0x1, 10, raw[0]);
        WriteBitsToDword((colorBytes[3]) & 0x1, 11, raw[0]);
        WriteBitsToDword((colorBytes[4]) & 0x1, 12, raw[0]);
        WriteBitsToDword((colorBytes[5]) & 0x1, 13, raw[0]);
        WriteBitsToDword((colorBytes[6]) & 0x1, 14, raw[0]);

        WriteBitsToDword((colorBytes[0] >> 1) & 0x1, 15, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 1) & 0x1, 16, raw[0]);
        WriteBitsToDword((colorBytes[2] >> 1) & 0x1, 17, raw[0]);
        WriteBitsToDword((colorBytes[3] >> 1) & 0x1, 18, raw[0]);
        WriteBitsToDword((colorBytes[4] >> 1) & 0x1, 19, raw[0]);
        WriteBitsToDword((colorBytes[5] >> 1) & 0x1, 20, raw[0]);
        WriteBitsToDword((colorBytes[6] >> 1) & 0x1, 21, raw[0]);

        WriteBitsToDword((colorBytes[0] >> 2) & 0x1, 22, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 2) & 0x1, 23, raw[0]);
        WriteBitsToDword((colorBytes[2] >> 2) & 0x1, 24, raw[0]);
        WriteBitsToDword((colorBytes[3] >> 2) & 0x1, 25, raw[0]);
        WriteBitsToDword((colorBytes[4] >> 2) & 0x1, 26, raw[0]);
        WriteBitsToDword((colorBytes[5] >> 2) & 0x1, 27, raw[0]);
        WriteBitsToDword((colorBytes[6] >> 2) & 0x1, 28, raw[0]);

        WriteBitsToDword((colorBytes[0] >> 3) & 0x1, 29, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 3) & 0x1, 30, raw[0]);
        WriteBitsToDword((colorBytes[2] >> 3) & 0x1, 31, raw[0]);
        WriteBitsToDword((colorBytes[3] >> 3) & 0x1, 0,  raw[1]);
        WriteBitsToDword((colorBytes[4] >> 3) & 0x1, 1,  raw[1]);
        WriteBitsToDword((colorBytes[5] >> 3) & 0x1, 2,  raw[1]);
        WriteBitsToDword((colorBytes[6] >> 3) & 0x1, 3,  raw[1]);

        WriteBitsToDword((colorBytes[0] >> 4) & 0x1, 4,  raw[1]);
        WriteBitsToDword((colorBytes[1] >> 4) & 0x1, 5,  raw[1]);
        WriteBitsToDword((colorBytes[2] >> 4) & 0x1, 6,  raw[1]);
        WriteBitsToDword((colorBytes[3] >> 4) & 0x1, 7,  raw[1]);
        WriteBitsToDword((colorBytes[4] >> 4) & 0x1, 8,  raw[1]);
        WriteBitsToDword((colorBytes[5] >> 4) & 0x1, 9,  raw[1]);
        WriteBitsToDword((colorBytes[6] >> 4) & 0x1, 10, raw[1]);

        WriteBitsToDword((colorBytes[0] >> 5) & 0x1, 11, raw[1]);
        WriteBitsToDword((colorBytes[1] >> 5) & 0x1, 12, raw[1]);
        WriteBitsToDword((colorBytes[2] >> 5) & 0x1, 13, raw[1]);
        WriteBitsToDword((colorBytes[3] >> 5) & 0x1, 14, raw[1]);
        WriteBitsToDword((colorBytes[4] >> 5) & 0x1, 15, raw[1]);
        WriteBitsToDword((colorBytes[5] >> 5) & 0x1, 16, raw[1]);
        WriteBitsToDword((colorBytes[6] >> 5) & 0x1, 17, raw[1]);

        WriteBitsToDword((colorBytes[0] >> 6) & 0x1, 18, raw[1]);
        WriteBitsToDword((colorBytes[1] >> 6) & 0x1, 19, raw[1]);
        WriteBitsToDword((colorBytes[2] >> 6) & 0x1, 20, raw[1]);
        WriteBitsToDword((colorBytes[3] >> 6) & 0x1, 21, raw[1]);
        WriteBitsToDword((colorBytes[4] >> 6) & 0x1, 22, raw[1]);
        WriteBitsToDword((colorBytes[5] >> 6) & 0x1, 23, raw[1]);
        WriteBitsToDword((colorBytes[6] >> 6) & 0x1, 24, raw[1]);

        WriteBitsToDword((colorBytes[0] >> 7) & 0x1, 25, raw[1]);
        WriteBitsToDword((colorBytes[1] >> 7) & 0x1, 26, raw[1]);
        WriteBitsToDword((colorBytes[2] >> 7) & 0x1, 27, raw[1]);
        WriteBitsToDword((colorBytes[3] >> 7) & 0x1, 28, raw[1]);
        WriteBitsToDword((colorBytes[4] >> 7) & 0x1, 29, raw[1]);
        WriteBitsToDword((colorBytes[5] >> 7) & 0x1, 30, raw[1]);
        WriteBitsToDword((colorBytes[6] >> 7) & 0x1, 31, raw[1]);
    }

    WriteBitsToDword(miscBytes[0],        2,  raw[2]);
    WriteBitsToDword(miscBytes[1],        10, raw[2]);
    WriteBitsToDword(miscBytes[2],        18, raw[2]);
    WriteBitsToDword(miscBytes[3] & 0x3F, 26, raw[2]);
    WriteBitsToDword((miscBytes[3] >> 6) & 0x1u, 0, raw[3]);

    WriteBitsToDword(miscBytes[4],        1,  raw[3]);
    WriteBitsToDword(miscBytes[5],        9,  raw[3]);
    WriteBitsToDword(miscBytes[6],        17, raw[3]);
    WriteBitsToDword(miscBytes[7] & 0x7F, 25, raw[3]);
}

void unshuffleMode6(uint colorAddress, uint miscAddress, uint ScrapData, uint modePattern, uint modeStatics, uint lowEntropy, in ByteAddressBuffer buffer, inout uint4 raw)
{
    uint colorBytes[12];
    loadChunkBytes12(colorAddress, buffer, colorBytes, 7);

    uint miscBytes[12];
    loadChunkBytes12(miscAddress, buffer, miscBytes, 8);

    WriteBitsToDword(64u, 0, raw[0]);

    if (modePattern == StableIsland)
    {
        uint order[8] = { 6, 7, 4, 5, 2, 3, 0, 1 };

        for (uint ep = 0; ep < 8; ++ep)
        {
            if ((modeStatics | lowEntropy) & (1u << ep))
            {
                for (uint ee = 0; ee < 8; ee++)
                {
                    if (order[ee] > order[ep]) order[ee]--;
                }
                order[ep] = 0;
            }
        }

        uint staticFields = countbits(modeStatics | lowEntropy);
        uint staticBits = staticFields * 7;
        uint otherFields = 8 - staticFields;

        uint nextStaticBit = staticBitStarts[staticFields];
        uint dwordIndex = 0;
        uint dwordPosition = 7;

        for (uint ep = 0; ep < 8; ep++)
        {
            if ((modeStatics | lowEntropy) & (1u << ep))
            {
                uint colorByteIndex = nextStaticBit / 8;
                uint colorBitIndex = nextStaticBit % 8;
                uint intermediateEndpoint = 0u;

                if (colorBitIndex > 1)
                {
                    uint lowBits = 8 - colorBitIndex;
                    uint highBits = 7 - lowBits;
                    uint mask = (~0u) >> (32 - lowBits);
                    WriteBitsToDword((colorBytes[colorByteIndex] >> colorBitIndex) & mask, 0, intermediateEndpoint);

                    mask = (~0u) >> (32 - highBits);
                    WriteBitsToDword(colorBytes[colorByteIndex + 1] & mask, lowBits, intermediateEndpoint);
                }
                else
                {
                    WriteBitsToDword((colorBytes[colorByteIndex] >> colorBitIndex) & 0x7Fu, 0, intermediateEndpoint);
                }

                if (32 - dwordPosition < 7)
                {
                    uint bitsToRaw0 = 32 - dwordPosition;
                    uint bitsToRaw1 = 7 - bitsToRaw0;
                    uint mask = (~0u) >> (32 - bitsToRaw0);
                    WriteBitsToDword(intermediateEndpoint & mask, dwordPosition, raw[dwordIndex]);
                    dwordIndex += 1;
                    dwordPosition = 0;
                    WriteBitsToDword(intermediateEndpoint >> bitsToRaw0, dwordPosition, raw[dwordIndex]);
                    dwordPosition += bitsToRaw1;
                }
                else
                {
                    WriteBitsToDword(intermediateEndpoint & 0x7Fu, dwordPosition, raw[dwordIndex]);
                    dwordPosition += 7;
                }

                nextStaticBit += 7;
            }
            else
            {
                uint intermediateEndpoint = 0u;
                uint intPosition = 0;
                for (uint b = 0; b < 7; ++b)
                {
                    const uint bi = 6 - b;
                    const uint destLinear = staticBits + order[ep] + (bi * otherFields);
                    const uint destLinearByte = destLinear / 8;
                    const uint destLinearBit = destLinear % 8;
                    const uint destRemappedByte = otherByteOrder[staticFields][destLinearByte];
                    const uint destRemapped = 8 * destRemappedByte + destLinearBit;

                    uint colorByteIndex = destRemapped / 8;
                    uint colorBitIndex = destRemapped % 8;

                    WriteBitsToDword((colorBytes[colorByteIndex] >> colorBitIndex) & 0x1u, intPosition, intermediateEndpoint);
                    intPosition += 1;
                }

                if (32 - dwordPosition < 7)
                {
                    uint firstHalf = 32 - dwordPosition;
                    uint secondHalf = 7 - firstHalf;
                    uint mask = (~0u) >> (32 - firstHalf);
                    WriteBitsToDword(intermediateEndpoint & mask, dwordPosition, raw[dwordIndex]);
                    dwordIndex++;
                    dwordPosition = 0;
                    WriteBitsToDword(intermediateEndpoint >> firstHalf, dwordPosition, raw[dwordIndex]);
                    dwordPosition += secondHalf;
                }
                else
                {
                    WriteBitsToDword(intermediateEndpoint & 0x7Fu, dwordPosition, raw[dwordIndex]);
                    dwordPosition += 7;
                }
            }
        }
    }
    else if (modePattern == EndpointPairSignificantBitInderleaved)
    {
        WriteBitsToDword((colorBytes[0]) & 0x1, 7,  raw[0]);
        WriteBitsToDword((colorBytes[1]) & 0x1, 8,  raw[0]);
        WriteBitsToDword((colorBytes[2]) & 0x1, 9,  raw[0]);
        WriteBitsToDword((colorBytes[3]) & 0x1, 10, raw[0]);
        WriteBitsToDword((colorBytes[4]) & 0x1, 11, raw[0]);
        WriteBitsToDword((colorBytes[5]) & 0x1, 12, raw[0]);
        WriteBitsToDword((colorBytes[6]) & 0x1, 13, raw[0]);

        WriteBitsToDword((colorBytes[0] >> 1) & 0x1, 14, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 1) & 0x1, 15, raw[0]);
        WriteBitsToDword((colorBytes[2] >> 1) & 0x1, 16, raw[0]);
        WriteBitsToDword((colorBytes[3] >> 1) & 0x1, 17, raw[0]);
        WriteBitsToDword((colorBytes[4] >> 1) & 0x1, 18, raw[0]);
        WriteBitsToDword((colorBytes[5] >> 1) & 0x1, 19, raw[0]);
        WriteBitsToDword((colorBytes[6] >> 1) & 0x1, 20, raw[0]);

        WriteBitsToDword((colorBytes[0] >> 2) & 0x1, 21, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 2) & 0x1, 22, raw[0]);
        WriteBitsToDword((colorBytes[2] >> 2) & 0x1, 23, raw[0]);
        WriteBitsToDword((colorBytes[3] >> 2) & 0x1, 24, raw[0]);
        WriteBitsToDword((colorBytes[4] >> 2) & 0x1, 25, raw[0]);
        WriteBitsToDword((colorBytes[5] >> 2) & 0x1, 26, raw[0]);
        WriteBitsToDword((colorBytes[6] >> 2) & 0x1, 27, raw[0]);

        WriteBitsToDword((colorBytes[0] >> 3) & 0x1, 28, raw[0]);
        WriteBitsToDword((colorBytes[1] >> 3) & 0x1, 29, raw[0]);
        WriteBitsToDword((colorBytes[2] >> 3) & 0x1, 30, raw[0]);
        WriteBitsToDword((colorBytes[3] >> 3) & 0x1, 31, raw[0]);
        WriteBitsToDword((colorBytes[4] >> 3) & 0x1, 0,  raw[1]);
        WriteBitsToDword((colorBytes[5] >> 3) & 0x1, 1,  raw[1]);
        WriteBitsToDword((colorBytes[6] >> 3) & 0x1, 2,  raw[1]);

        WriteBitsToDword((colorBytes[0] >> 4) & 0x1, 3, raw[1]);
        WriteBitsToDword((colorBytes[1] >> 4) & 0x1, 4, raw[1]);
        WriteBitsToDword((colorBytes[2] >> 4) & 0x1, 5, raw[1]);
        WriteBitsToDword((colorBytes[3] >> 4) & 0x1, 6, raw[1]);
        WriteBitsToDword((colorBytes[4] >> 4) & 0x1, 7, raw[1]);
        WriteBitsToDword((colorBytes[5] >> 4) & 0x1, 8, raw[1]);
        WriteBitsToDword((colorBytes[6] >> 4) & 0x1, 9, raw[1]);

        WriteBitsToDword((colorBytes[0] >> 5) & 0x1, 10, raw[1]);
        WriteBitsToDword((colorBytes[1] >> 5) & 0x1, 11, raw[1]);
        WriteBitsToDword((colorBytes[2] >> 5) & 0x1, 12, raw[1]);
        WriteBitsToDword((colorBytes[3] >> 5) & 0x1, 13, raw[1]);
        WriteBitsToDword((colorBytes[4] >> 5) & 0x1, 14, raw[1]);
        WriteBitsToDword((colorBytes[5] >> 5) & 0x1, 15, raw[1]);
        WriteBitsToDword((colorBytes[6] >> 5) & 0x1, 16, raw[1]);

        WriteBitsToDword((colorBytes[0] >> 6) & 0x1, 17, raw[1]);
        WriteBitsToDword((colorBytes[1] >> 6) & 0x1, 18, raw[1]);
        WriteBitsToDword((colorBytes[2] >> 6) & 0x1, 19, raw[1]);
        WriteBitsToDword((colorBytes[3] >> 6) & 0x1, 20, raw[1]);
        WriteBitsToDword((colorBytes[4] >> 6) & 0x1, 21, raw[1]);
        WriteBitsToDword((colorBytes[5] >> 6) & 0x1, 22, raw[1]);
        WriteBitsToDword((colorBytes[6] >> 6) & 0x1, 23, raw[1]);

        WriteBitsToDword((colorBytes[0] >> 7) & 0x1, 24, raw[1]);
        WriteBitsToDword((colorBytes[1] >> 7) & 0x1, 25, raw[1]);
        WriteBitsToDword((colorBytes[2] >> 7) & 0x1, 26, raw[1]);
        WriteBitsToDword((colorBytes[3] >> 7) & 0x1, 27, raw[1]);
        WriteBitsToDword((colorBytes[4] >> 7) & 0x1, 28, raw[1]);
        WriteBitsToDword((colorBytes[5] >> 7) & 0x1, 29, raw[1]);
        WriteBitsToDword((colorBytes[6] >> 7) & 0x1, 30, raw[1]);
    }

    WriteBitsToDword(miscBytes[7] >> 7, 31, raw[1]);
    WriteBitsToDword(ScrapData,         0,  raw[2]);

    WriteBitsToDword(miscBytes[0],        1,  raw[2]);
    WriteBitsToDword(miscBytes[1],        9,  raw[2]);
    WriteBitsToDword(miscBytes[2],        17, raw[2]);
    WriteBitsToDword(miscBytes[3] & 0x7F, 25, raw[2]);
    WriteBitsToDword(miscBytes[3] >> 7,   0,  raw[3]);
    WriteBitsToDword(miscBytes[4],        1,  raw[3]);
    WriteBitsToDword(miscBytes[5],        9,  raw[3]);
    WriteBitsToDword(miscBytes[6],        17, raw[3]);
    WriteBitsToDword(miscBytes[7] & 0x7F, 25, raw[3]);
}

void unshuffleMode7(uint colorAddress, uint miscAddress, uint ScrapData, uint modePattern, in ByteAddressBuffer buffer, inout uint4 raw)
{
    uint colorBytes[16];
    loadChunkBytes16(colorAddress, buffer, colorBytes, 11);

    uint miscBytes[12];
    loadChunkBytes12(miscAddress, buffer, miscBytes, 5);

    WriteBitsToDword(128u, 0, raw[0]);

    if (modePattern == EndpointQuadSignificantBitInderleaved)
    {
        WriteBitsToDword(colorBytes[0] & 0xF,        8,  raw[0]);
        WriteBitsToDword((colorBytes[10] >> 4) & 0x3, 12, raw[0]);

        const uint kChannels = 8;
        uint dwordIndex = 0;
        uint dwordPosition = 14;

        uint base = 4;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 82;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
        }

        base = 6;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 80;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 8;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
        }

        base = 78;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 10;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 76;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        WriteBitsToDword(colorBytes[10] >> 6, 30, raw[2]);

        WriteBitsToDword(miscBytes[0], 0,  raw[3]);
        WriteBitsToDword(miscBytes[1], 8,  raw[3]);
        WriteBitsToDword(miscBytes[2], 16, raw[3]);
        WriteBitsToDword(miscBytes[3], 24, raw[3]);
    }
    else if (modePattern == EndpointQuadSignificantBitInderleavedAlt)
    {
        WriteBitsToDword(miscBytes[0] & 0x3F, 8, raw[0]);

        const uint kChannels = 8;
        uint dwordIndex = 0;
        uint dwordPosition = 14;

        uint base = 0;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 78;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
        }

        base = 2;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 76;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 4;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
        }

        base = 74;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 6;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 72;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) - (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        WriteBitsToDword(miscBytes[0] >> 6, 30, raw[2]);
        WriteBitsToDword(miscBytes[1],      0,  raw[3]);
        WriteBitsToDword(miscBytes[2],      8,  raw[3]);
        WriteBitsToDword(miscBytes[3],      16, raw[3]);
        WriteBitsToDword(miscBytes[4],      24, raw[3]);
    }
    else if (modePattern == EndpointPairSignificantBitInderleaved)
    {
        WriteBitsToDword(colorBytes[10] & 0x3F, 8, raw[0]);

        const uint kChannels = 8;
        uint dwordIndex = 0;
        uint dwordPosition = 14;

        uint base = 0;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 40;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
        }

        base = 2;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 42;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 4;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
            if (dwordPosition == 32) { dwordPosition = 0; dwordIndex++; }
        }

        base = 44;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 6;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        base = 46;
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }
        for (uint j = 0; j < 5; ++j)
        {
            uint colorTotalIndex = (base + 1) + (kChannels * j);
            WriteBitsToDword((colorBytes[colorTotalIndex / 8] >> (colorTotalIndex % 8)) & 0x1, dwordPosition++, raw[dwordIndex]);
        }

        WriteBitsToDword(colorBytes[10] >> 6, 30, raw[2]);

        WriteBitsToDword(miscBytes[0], 0,  raw[3]);
        WriteBitsToDword(miscBytes[1], 8,  raw[3]);
        WriteBitsToDword(miscBytes[2], 16, raw[3]);
        WriteBitsToDword(miscBytes[3], 24, raw[3]);
    }
}

void unshuffleMode8(inout uint4 raw)
{
    raw = uint4(0u, 0u, 0u, 0u);
}
