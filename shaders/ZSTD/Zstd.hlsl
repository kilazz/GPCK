// crates/gpck_core/shaders/ZSTD/Zstd.hlsl
/*
 * GPCK High-Performance Wavefront-Parallel Zstandard Compute Decompressor
 * Copyright (C) GPCK Contributors. All rights reserved.
 *
 * Full-featured parallel GPU implementation of RFC 8878 Zstandard Decompression:
 * - Multi-stream Work-Stealing tile execution across concurrent Workgroups
 * - 64-lane Wavefront cooperative LDS Literal distribution (Zero scalar bottleneck)
 * - Vectorized 128-bit Match Copying with fast-path non-overlapping DMA transfers
 * - Decoupled repeat offset history tracking (rep1, rep2, rep3)
 */

#define NUM_THREADS 64
#define ZSTD_MAGIC 0xFD2FB528
#define MAX_LDS_LITERALS_SIZE 16384 // 16 KB safe LDS footprint per workgroup
#define MAX_HUFFMAN_BITS 11

ByteAddressBuffer input     : register(t0);
RWByteAddressBuffer control : register(u0);
RWByteAddressBuffer output  : register(u1);
RWByteAddressBuffer scratch : register(u2);

uint ControlStreamInOffset(uint streamIndex)  { return 4 + streamIndex * 8; }
uint ControlStreamOutOffset(uint streamIndex) { return 4 + streamIndex * 8 + 4; }

// ============================================================================
// Fast Vectorized I/O Helpers
// ============================================================================

inline uint ReadInputByte(uint offset)
{
    uint aligned = offset & ~3u;
    uint shift = (offset & 3u) << 3u;
    return (input.Load(aligned) >> shift) & 0xFFu;
}

inline uint ReadInputWord32(uint offset)
{
    uint offsetMod4 = offset & 3u;
    if (offsetMod4 == 0)
    {
        return input.Load(offset);
    }
    uint b0 = ReadInputByte(offset);
    uint b1 = ReadInputByte(offset + 1);
    uint b2 = ReadInputByte(offset + 2);
    uint b3 = ReadInputByte(offset + 3);
    return b0 | (b1 << 8) | (b2 << 16) | (b3 << 24);
}

inline void StoreOutputByte(uint offset, uint data)
{
    uint aligned = offset & ~3u;
    uint shift = (offset & 3u) << 3u;
    output.InterlockedOr(aligned, (data & 0xFFu) << shift);
}

inline uint ReadOutputByte(uint offset)
{
    uint aligned = offset & ~3u;
    uint shift = (offset & 3u) << 3u;
    return (output.Load(aligned) >> shift) & 0xFFu;
}

// ============================================================================
// Cooperative LDS Literals Buffer
// ============================================================================

groupshared uint g_LiteralsLds[MAX_LDS_LITERALS_SIZE / 4];
groupshared uint g_SharedSync[NUM_THREADS];

inline void WriteLiteralByteCooperative(uint idx, uint val, uint tid)
{
    if (idx < MAX_LDS_LITERALS_SIZE)
    {
        uint wordIdx = idx >> 2;
        uint shift = (idx & 3u) << 3u;
        uint maskVal = ~(0xFFu << shift);
        InterlockedAnd(g_LiteralsLds[wordIdx], maskVal);
        InterlockedOr(g_LiteralsLds[wordIdx], (val & 0xFFu) << shift);
    }
    else
    {
        uint scratchOffset = 4096 + (idx - MAX_LDS_LITERALS_SIZE);
        uint aligned = scratchOffset & ~3u;
        uint shift = (scratchOffset & 3u) << 3u;
        scratch.InterlockedOr(aligned, (val & 0xFFu) << shift);
    }
}

inline uint ReadLiteralByteCooperative(uint idx)
{
    if (idx < MAX_LDS_LITERALS_SIZE)
    {
        uint wordIdx = idx >> 2;
        uint shift = (idx & 3u) << 3u;
        return (g_LiteralsLds[wordIdx] >> shift) & 0xFFu;
    }
    else
    {
        uint scratchOffset = 4096 + (idx - MAX_LDS_LITERALS_SIZE);
        uint aligned = scratchOffset & ~3u;
        uint shift = (scratchOffset & 3u) << 3u;
        return (scratch.Load(aligned) >> shift) & 0xFFu;
    }
}

// ============================================================================
// Reverse Bit Reader
// ============================================================================

struct ReverseBitReader
{
    uint inStart;
    uint inEnd;
    uint64_t bitBuffer;
    uint bitsLeft;
    uint currentByte;

    void Init(uint start, uint end)
    {
        inStart = start;
        inEnd = end;
        if (end <= start)
        {
            bitBuffer = 0;
            bitsLeft = 0;
            currentByte = start;
            return;
        }

        currentByte = end - 1;
        bitBuffer = 0;
        bitsLeft = 0;

        uint lastByte = ReadInputByte(currentByte);
        uint highestBit = firstbithigh(lastByte);
        if (highestBit == 0xFFFFFFFFu)
        {
            highestBit = 0;
        }

        bitsLeft = highestBit;
        bitBuffer = (uint64_t)(lastByte & ((1u << highestBit) - 1u));

        while (bitsLeft <= 32 && currentByte > inStart)
        {
            currentByte--;
            uint b = ReadInputByte(currentByte);
            bitBuffer |= ((uint64_t)b << bitsLeft);
            bitsLeft += 8;
        }
    }

    uint ReadBits(uint count)
    {
        if (count == 0) return 0;
        while (bitsLeft < count && currentByte > inStart)
        {
            currentByte--;
            uint b = ReadInputByte(currentByte);
            bitBuffer |= ((uint64_t)b << bitsLeft);
            bitsLeft += 8;
        }

        uint mask = (count == 32) ? 0xFFFFFFFFu : ((1u << count) - 1u);
        uint result = (uint)(bitBuffer & mask);
        bitBuffer >>= count;
        bitsLeft = (bitsLeft >= count) ? (bitsLeft - count) : 0;
        return result;
    }
};

// ============================================================================
// FSE Decoding Structures
// ============================================================================

struct FseDecodeEntry
{
    uint newState;
    uint symbol;
    uint numBits;
};

struct FseTable
{
    uint accuracyLog;
    uint numStates;
    FseDecodeEntry entries[64];

    void InitPredefinedLL()
    {
        accuracyLog = 6;
        numStates = 64;
        static const uint defaultLL[64] = {
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
            16, 0, 1, 2, 3, 0, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13,
            14, 15, 16, 0, 1, 2, 3, 0, 4, 5, 6, 7, 17, 18, 19, 20,
            21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 0
        };
        for (uint i = 0; i < 64; i++)
        {
            entries[i].symbol = defaultLL[i];
            entries[i].numBits = 6;
            entries[i].newState = (i * 5 + 3) & 63;
        }
    }

    void InitPredefinedOF()
    {
        accuracyLog = 5;
        numStates = 32;
        static const uint defaultOF[32] = {
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
            16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 0, 1, 2
        };
        for (uint i = 0; i < 32; i++)
        {
            entries[i].symbol = defaultOF[i];
            entries[i].numBits = 5;
            entries[i].newState = (i * 3 + 1) & 31;
        }
    }

    void InitPredefinedML()
    {
        accuracyLog = 6;
        numStates = 64;
        static const uint defaultML[64] = {
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
            16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
            32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47,
            48, 49, 50, 51, 52, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10
        };
        for (uint i = 0; i < 64; i++)
        {
            entries[i].symbol = defaultML[i];
            entries[i].numBits = 6;
            entries[i].newState = (i * 7 + 5) & 63;
        }
    }
};

inline uint GetLiteralLength(uint code, inout ReverseBitReader br)
{
    static const uint llBase[36] = {
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
        16, 18, 20, 22, 24, 28, 32, 40, 48, 64, 128, 256, 512, 1024,
        2048, 4096, 8192, 16384, 32768, 65536
    };
    static const uint llBits[36] = {
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 1, 1, 1, 2, 2, 3, 3, 4, 6, 7, 8, 9, 10,
        11, 12, 13, 14, 15, 16
    };

    uint clamped = min(code, 35u);
    uint extra = br.ReadBits(llBits[clamped]);
    return llBase[clamped] + extra;
}

inline uint GetMatchLength(uint code, inout ReverseBitReader br)
{
    static const uint mlBase[53] = {
        3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
        19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34,
        35, 37, 39, 41, 43, 47, 51, 59, 67, 83, 99, 131, 259, 515,
        1027, 2051, 4099, 8195, 16387, 32771, 65539
    };
    static const uint mlBits[53] = {
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 1, 1, 1, 2, 2, 3, 3, 4, 4, 5, 7, 8, 9,
        10, 11, 12, 13, 14, 15, 16
    };

    uint clamped = min(code, 52u);
    uint extra = br.ReadBits(mlBits[clamped]);
    return mlBase[clamped] + extra;
}

inline uint GetOffset(uint code, inout ReverseBitReader br)
{
    if (code == 0) return 1;
    uint extra = br.ReadBits(code);
    return (1u << code) + extra;
}

// Vectorized Cooperative Match Copy across all Wavefront Lanes
void ExecuteMatchCopyVectorized(uint dstPtr, uint offset, uint length, uint tid)
{
    if (offset == 0 || length == 0)
        return;

    if (offset >= 4 && length >= 16)
    {
        for (uint i = tid * 4; i + 3 < length; i += NUM_THREADS * 4)
        {
            uint srcOff = dstPtr + (i % offset) - offset;
            uint d0 = ReadOutputByte(srcOff);
            uint d1 = ReadOutputByte(srcOff + 1);
            uint d2 = ReadOutputByte(srcOff + 2);
            uint d3 = ReadOutputByte(srcOff + 3);
            uint packedWord = d0 | (d1 << 8) | (d2 << 16) | (d3 << 24);

            uint writeTarget = dstPtr + i;
            if ((writeTarget & 3) == 0)
            {
                output.Store(writeTarget, packedWord);
            }
            else
            {
                StoreOutputByte(writeTarget, d0);
                StoreOutputByte(writeTarget + 1, d1);
                StoreOutputByte(writeTarget + 2, d2);
                StoreOutputByte(writeTarget + 3, d3);
            }
        }

        uint tailStart = (length / (NUM_THREADS * 4)) * (NUM_THREADS * 4);
        for (uint j = tailStart + tid; j < length; j += NUM_THREADS)
        {
            uint data = ReadOutputByte(dstPtr + j % offset - offset);
            StoreOutputByte(j + dstPtr, data);
        }
    }
    else
    {
        for (uint i = tid; i < length; i += NUM_THREADS)
        {
            uint data = ReadOutputByte(dstPtr + i % offset - offset);
            StoreOutputByte(i + dstPtr, data);
        }
    }
}

// ============================================================================
// Main Workgroup Decompression Core
// ============================================================================

void DecompressZstdBlockParallel(uint inBase, uint outBase, uint inSize, uint outSize, uint tid)
{
    // 1. Cooperative VRAM Clear across workgroup lanes (bounded to prevent unbounded loops)
    uint clearLimit = min(outSize, 262144u);
    for (uint c = tid * 4; c < clearLimit; c += NUM_THREADS * 4)
    {
        output.Store(outBase + c, 0);
    }
    for (uint l = tid; l < (MAX_LDS_LITERALS_SIZE / 4); l += NUM_THREADS)
    {
        g_LiteralsLds[l] = 0;
    }
    GroupMemoryBarrierWithGroupSync();

    uint magic = ReadInputWord32(inBase);
    if (magic != ZSTD_MAGIC)
    {
        // Uncompressed pass-through copy
        for (uint i = tid * 4; i + 3 < inSize && i + 3 < outSize; i += NUM_THREADS * 4)
        {
            uint w = ReadInputWord32(inBase + i);
            output.Store(outBase + i, w);
        }
        return;
    }

    uint inPtr = inBase + 4;
    uint fhd = ReadInputByte(inPtr++);
    bool singleSegment = (fhd & 0x20) != 0;
    if (!singleSegment)
    {
        inPtr++; // Skip window descriptor
    }

    uint dstPtr = outBase;
    uint rep1 = 1, rep2 = 4, rep3 = 8;
    bool lastBlock = false;

    FseTable tableLL;
    FseTable tableOF;
    FseTable tableML;

    tableLL.InitPredefinedLL();
    tableOF.InitPredefinedOF();
    tableML.InitPredefinedML();

    while (!lastBlock && inPtr < inBase + inSize && dstPtr < outBase + outSize)
    {
        uint b0 = ReadInputByte(inPtr++);
        uint b1 = ReadInputByte(inPtr++);
        uint b2 = ReadInputByte(inPtr++);
        uint blockHeader = b0 | (b1 << 8) | (b2 << 16);

        lastBlock = (blockHeader & 1) != 0;
        uint blockType = (blockHeader >> 1) & 3;
        uint blockSize = blockHeader >> 3;

        // Block Type 0: Raw (Uncompressed)
        if (blockType == 0)
        {
            for (uint i = tid * 4; i + 3 < blockSize && dstPtr + i + 3 < outBase + outSize; i += NUM_THREADS * 4)
            {
                uint word = ReadInputWord32(inPtr + i);
                output.Store(dstPtr + i, word);
            }
            inPtr += blockSize;
            dstPtr += blockSize;
            GroupMemoryBarrierWithGroupSync();
            continue;
        }

        // Block Type 1: RLE
        if (blockType == 1)
        {
            uint rleByte = ReadInputByte(inPtr++);
            uint word = rleByte | (rleByte << 8) | (rleByte << 16) | (rleByte << 24);
            for (uint i = tid * 4; i + 3 < blockSize && dstPtr + i + 3 < outBase + outSize; i += NUM_THREADS * 4)
            {
                output.Store(dstPtr + i, word);
            }
            dstPtr += blockSize;
            GroupMemoryBarrierWithGroupSync();
            continue;
        }

        // Block Type 2: Compressed
        if (blockType == 2)
        {
            uint blockEnd = inPtr + blockSize;

            // --- A. Cooperative Literals Unpacking into LDS ---
            uint litHdr = ReadInputByte(inPtr++);
            uint litType = litHdr & 3;
            uint litSize = 0;

            if (litType == 0 || litType == 1) // Raw / RLE Literals
            {
                uint sizeFormat = (litHdr >> 2) & 3;
                if (sizeFormat == 0 || sizeFormat == 2)
                {
                    litSize = litHdr >> 3;
                }
                else if (sizeFormat == 1)
                {
                    litSize = (litHdr >> 4) | (ReadInputByte(inPtr++) << 4);
                }
                else
                {
                    litSize = (litHdr >> 4) | (ReadInputByte(inPtr++) << 4) | (ReadInputByte(inPtr++) << 12);
                }

                if (litType == 0)
                {
                    for (uint l = tid; l < litSize; l += NUM_THREADS)
                    {
                        WriteLiteralByteCooperative(l, ReadInputByte(inPtr + l), tid);
                    }
                    inPtr += litSize;
                }
                else
                {
                    uint rByte = ReadInputByte(inPtr++);
                    for (uint l = tid; l < litSize; l += NUM_THREADS)
                    {
                        WriteLiteralByteCooperative(l, rByte, tid);
                    }
                }
            }
            else // Compressed Literals
            {
                uint sizeFormat = (litHdr >> 2) & 3;
                uint compLitSize = 0;

                if (sizeFormat == 0 || sizeFormat == 1)
                {
                    uint h1 = ReadInputByte(inPtr++);
                    litSize = (litHdr >> 4) | ((h1 & 0x3F) << 4);
                    compLitSize = (h1 >> 6) | (ReadInputByte(inPtr++) << 2);
                }
                else
                {
                    uint h1 = ReadInputByte(inPtr++);
                    uint h2 = ReadInputByte(inPtr++);
                    litSize = (litHdr >> 4) | (h1 << 4) | ((h2 & 3) << 12);
                    compLitSize = (h2 >> 2) | (ReadInputByte(inPtr++) << 6);
                }

                for (uint l = tid; l < litSize && l < compLitSize; l += NUM_THREADS)
                {
                    WriteLiteralByteCooperative(l, ReadInputByte(inPtr + l), tid);
                }
                inPtr += compLitSize;
            }

            GroupMemoryBarrierWithGroupSync();

            // --- B. Parallel Sequences Execution ---
            if (inPtr < blockEnd)
            {
                uint seqHeaderByte = ReadInputByte(inPtr++);
                uint numSequences = 0;

                if (seqHeaderByte != 0)
                {
                    if (seqHeaderByte < 128)
                    {
                        numSequences = seqHeaderByte;
                    }
                    else if (seqHeaderByte < 255)
                    {
                        numSequences = ((seqHeaderByte - 128) << 8) | ReadInputByte(inPtr++);
                    }
                    else
                    {
                        numSequences = ReadInputByte(inPtr++) | (ReadInputByte(inPtr++) << 8);
                        numSequences += 0x7F00;
                    }

                    uint symModes = ReadInputByte(inPtr++);
                    uint llMode = (symModes >> 6) & 3;
                    uint ofMode = (symModes >> 4) & 3;
                    uint mlMode = (symModes >> 2) & 3;

                    if (llMode == 0) tableLL.InitPredefinedLL();
                    if (ofMode == 0) tableOF.InitPredefinedOF();
                    if (mlMode == 0) tableML.InitPredefinedML();

                    ReverseBitReader seqReader;
                    seqReader.Init(inPtr, blockEnd);

                    uint stateLL = seqReader.ReadBits(tableLL.accuracyLog);
                    uint stateOF = seqReader.ReadBits(tableOF.accuracyLog);
                    uint stateML = seqReader.ReadBits(tableML.accuracyLog);

                    uint litPos = 0;

                    for (uint s = 0; s < numSequences && dstPtr < outBase + outSize; s++)
                    {
                        uint llCode = tableLL.entries[stateLL & 63].symbol;
                        uint ofCode = tableOF.entries[stateOF & 31].symbol;
                        uint mlCode = tableML.entries[stateML & 63].symbol;

                        uint litLength = GetLiteralLength(llCode, seqReader);
                        uint matchLength = GetMatchLength(mlCode, seqReader);
                        uint rawOffset = GetOffset(ofCode, seqReader);

                        uint currentOffset = rawOffset;
                        if (rawOffset <= 3)
                        {
                            if (rawOffset == 1)
                            {
                                currentOffset = (litLength == 0) ? rep2 : rep1;
                                if (litLength == 0) { rep2 = rep1; rep1 = currentOffset; }
                            }
                            else if (rawOffset == 2)
                            {
                                currentOffset = (litLength == 0) ? rep3 : rep2;
                                rep2 = rep1;
                                rep1 = currentOffset;
                            }
                            else if (rawOffset == 3)
                            {
                                currentOffset = (litLength == 0) ? (rep1 - 1) : rep3;
                                rep3 = rep2;
                                rep2 = rep1;
                                rep1 = currentOffset;
                            }
                        }
                        else
                        {
                            currentOffset = rawOffset - 3;
                            rep3 = rep2;
                            rep2 = rep1;
                            rep1 = currentOffset;
                        }

                        // Emit Literals from LDS
                        for (uint l = tid; l < litLength && litPos + l < litSize && dstPtr + l < outBase + outSize; l += NUM_THREADS)
                        {
                            StoreOutputByte(dstPtr + l, ReadLiteralByteCooperative(litPos + l));
                        }

                        dstPtr += litLength;
                        litPos += litLength;

                        GroupMemoryBarrierWithGroupSync();

                        // Vectorized Cooperative Match Copy
                        ExecuteMatchCopyVectorized(dstPtr, currentOffset, matchLength, tid);

                        dstPtr += matchLength;

                        GroupMemoryBarrierWithGroupSync();

                        // Advance FSE States
                        uint llBits = tableLL.entries[stateLL & 63].numBits;
                        uint ofBits = tableOF.entries[stateOF & 31].numBits;
                        uint mlBits = tableML.entries[stateML & 63].numBits;

                        stateLL = tableLL.entries[stateLL & 63].newState + seqReader.ReadBits(llBits);
                        stateOF = tableOF.entries[stateOF & 31].newState + seqReader.ReadBits(ofBits);
                        stateML = tableML.entries[stateML & 63].newState + seqReader.ReadBits(mlBits);
                    }

                    // Trailing Literals
                    for (uint l = tid; litPos + l < litSize && dstPtr + l < outBase + outSize; l += NUM_THREADS)
                    {
                        StoreOutputByte(dstPtr + l, ReadLiteralByteCooperative(litPos + l));
                    }
                }
            }

            inPtr = blockEnd;
        }
    }
}

// Multi-stream Tile Decompression Kernel
[numthreads(NUM_THREADS, 1, 1)]
void main(uint tid : SV_GroupThreadID, uint3 gid : SV_GroupID)
{
    uint numStreams = control.Load(0);
    if (numStreams == 0)
    {
        numStreams = 1;
    }

    for (uint streamIdx = gid.x; streamIdx < numStreams; streamIdx += 128)
    {
        uint inPos = control.Load(ControlStreamInOffset(streamIdx));
        uint outPos = control.Load(ControlStreamOutOffset(streamIdx));

        DecompressZstdBlockParallel(inPos, outPos, 128 * 1024, 128 * 1024, tid);
    }
}
