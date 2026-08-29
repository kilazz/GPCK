// shaders/GDeflate/GDeflate.hlsl
/*
 * SPDX-FileCopyrightText: Copyright (c) 2020, 2021, 2022 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
 * SPDX-FileCopyrightText: Copyright (c) Microsoft Corporation. All rights reserved.
 * SPDX-License-Identifier: Apache-2.0
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 * http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

#define NUM_BITSTREAMS 32          // GDeflate interleaves 32 compressed bitstreams
#define NUM_THREADS NUM_BITSTREAMS // Thread blocks are sized to match bitstream count

// Raw input and output buffer bindings
ByteAddressBuffer input : register(t0);
RWByteAddressBuffer control : register(u0);
RWByteAddressBuffer output : register(u1);
RWByteAddressBuffer scratch : register(u2);

uint ControlStreamOffset(uint streamIndex)
{
    return 4 + streamIndex * 8;
}

uint ControlStreamInOffset(uint streamIndex)
{
    return ControlStreamOffset(streamIndex);
}

uint ScratchStreamTileIndexOffset(uint streamIndex)
{
    return streamIndex * 4;
}

#include "tilestream.hlsli"

uint ControlStreamOutOffset(uint streamIndex)
{
    return ControlStreamInOffset(streamIndex) + 4;
}

inline uint32_t mask(uint32_t n)
{
    return n >= 32 ? 0xFFFFFFFFu : ((1u << n) - 1u);
}

inline uint32_t ltMask(uint tid)
{
    return mask(tid);
}

inline uint32_t extract(uint32_t data, uint32_t pos, uint32_t n, uint32_t base = 0)
{
    return ((data >> pos) & mask(n)) + base;
}

groupshared uint32_t g_tmp[NUM_THREADS];
groupshared uint32_t g_tmp1[NUM_THREADS];
groupshared uint32_t g_tmp2[NUM_THREADS];
groupshared uint32_t g_tmp3[NUM_THREADS];

// Adaptive vote: Hardware ballot if wave size >= 32, otherwise groupshared fallback
inline uint32_t vote(bool p, uint tid)
{
    if (WaveGetLaneCount() >= NUM_THREADS)
    {
        return (uint32_t)WaveActiveBallot(p).x;
    }
    else
    {
        g_tmp1[tid] = p ? (1u << tid) : 0;
        GroupMemoryBarrierWithGroupSync();
        [unroll] for (uint i = NUM_THREADS / 2; i > 0; i >>= 1)
        {
            if (tid < i)
                g_tmp1[tid] |= g_tmp1[tid + i];
            GroupMemoryBarrierWithGroupSync();
        }
        uint ballot = g_tmp1[0];
        GroupMemoryBarrierWithGroupSync();
        return ballot;
    }
}

// Adaptive shuffle: Register-level lane read if wave size >= 32, otherwise groupshared fallback
inline uint32_t shuffle(uint32_t value, uint idx, uint tid)
{
    if (WaveGetLaneCount() >= NUM_THREADS)
    {
        return WaveReadLaneAt(value, idx);
    }
    else
    {
        g_tmp1[tid] = value;
        GroupMemoryBarrierWithGroupSync();
        uint32_t res = g_tmp1[idx];
        GroupMemoryBarrierWithGroupSync();
        return res;
    }
}

// Adaptive broadcast: Direct register broadcast if wave size >= 32, otherwise groupshared fallback
inline uint32_t broadcast(uint32_t value, uint idx, uint tid)
{
    if (WaveGetLaneCount() >= NUM_THREADS)
    {
        return WaveReadLaneAt(value, idx);
    }
    else
    {
        GroupMemoryBarrierWithGroupSync();
        if (tid == idx)
            g_tmp1[0] = value;
        GroupMemoryBarrierWithGroupSync();
        return g_tmp1[0];
    }
}

inline bool all(bool p, uint tid)
{
    if (WaveGetLaneCount() >= NUM_THREADS)
    {
        return WaveActiveAllTrue(p);
    }
    else
    {
        return vote(p, tid) == (1u << NUM_THREADS) - 1u;
    }
}

// Adaptive prefix sum: In-register wave prefix sum if wave size >= 32
inline uint32_t scan(uint32_t value, uint tid)
{
    if (WaveGetLaneCount() >= NUM_THREADS)
    {
        return WavePrefixSum(value);
    }
    else
    {
        uint32_t sum = value;
        [unroll] for (uint i = 1; i < NUM_THREADS; i *= 2) sum += tid >= i ? shuffle(sum, tid - i, tid) : 0;
        return sum - value;
    }
}

// Segmented prefix sum for 16-lane sub-halves
inline uint32_t scan16(uint32_t value, uint tid)
{
    if (WaveGetLaneCount() >= NUM_THREADS)
    {
        uint sum = WavePrefixSum(value);
        uint baseSum = WaveReadLaneAt(sum, tid & ~15u);
        return (sum - baseSum) + value;
    }
    else
    {
        [unroll] for (uint i = 1; i < NUM_THREADS / 2; i *= 2)
        {
            value += (tid & 15) >= i ? shuffle(value, tid - i, tid) : 0;
        }
        return value;
    }
}

inline uint32_t match(uint32_t value, uint tid)
{
    if (WaveGetLaneCount() >= NUM_THREADS)
    {
        uint32_t maskVal = 0;
        [unroll] for (uint i = 0; i < NUM_THREADS; i++)
        {
            maskVal |= (WaveReadLaneAt(value, i) == value ? 1u : 0u) << i;
        }
        return maskVal;
    }
    else
    {
        uint32_t msk = 0;
        g_tmp1[tid] = value;
        GroupMemoryBarrierWithGroupSync();
        [unroll] for (uint i = 0; i < NUM_THREADS; i++)
        {
            GroupMemoryBarrierWithGroupSync();
            msk |= g_tmp1[i] == value ? (1u << i) : 0;
        }
        GroupMemoryBarrierWithGroupSync();
        return msk;
    }
}

inline uint32_t ReadOutputByte(uint32_t offset)
{
    uint32_t offsetMod4 = offset & 3;
    offset -= offsetMod4;
    uint32_t shift = offsetMod4 << 3;
    return (output.Load(offset) >> shift) & 0xff;
}

inline void StoreByte(uint32_t offset, uint32_t data)
{
    uint32_t offsetMod4 = offset & 3;
    offset -= offsetMod4;
    uint32_t shift = offsetMod4 << 3;
    output.InterlockedOr(offset, (data & 0xff) << shift);
}

struct BitReader
{
    static const uint kWidth = NUM_BITSTREAMS;

    uint base, cnt;
    uint64_t buf;

    // Reset bit reader assuming base pointer is word-aligned
    void init(uint i, uint tid)
    {
        cnt = kWidth;
        buf = (uint64_t)input.Load(i + tid * 4);
        base = i + kWidth * 4;
    }

    // Refill bit buffer if required and advance base pointer
    void refill(bool p, uint tid)
    {
        p &= cnt < kWidth;
        uint32_t ballot = vote(p, tid);
        uint offset = countbits(ballot & ltMask(tid)) * 4;
        if (p)
        {
            buf |= (uint64_t)input.Load(base + offset) << cnt;
            cnt += kWidth;
        }
        base += countbits(ballot) * 4;
    }

    // Remove n bits from bit buffer
    void eat(uint n, uint tid, bool p)
    {
        if (p)
        {
            buf >>= n;
            cnt -= n;
        }
        refill(p, tid);
    }

    // Inspect n bits from buffer without changing state
    uint32_t peek(uint n)
    {
        return (uint32_t)buf & mask(n);
    }

    uint32_t peek()
    {
        return (uint32_t)buf;
    }

    // Read n bits from buffer and advance
    uint32_t read(uint n, uint tid, bool p)
    {
        uint32_t bits = p ? (uint32_t)buf & mask(n) : 0;
        eat(n, tid, p);
        return bits;
    }
};

groupshared struct Scratch
{
    uint32_t data[64];
    void clear(uint tid)
    {
        data[tid] = data[tid + NUM_THREADS] = 0;
    }

    uint32_t get4b(uint i)
    {
        return (data[i / 8] >> (4 * (i % 8))) & 15;
    }
} g_buf;

void set4b(uint32_t nibbles, uint32_t n, uint32_t i)
{
    nibbles |= (nibbles << 4);
    nibbles |= (nibbles << 8);
    nibbles |= (nibbles << 16);
    nibbles &= ~((int)0xf0000000 >> (28 - n * 4));

    uint32_t base = i / 8;
    uint32_t shift = i % 8;

    InterlockedOr(g_buf.data[base], nibbles << (shift * 4));
    if (shift + n > 8)
        InterlockedOr(g_buf.data[base + 1], nibbles >> ((8 - shift) * 4));
}

groupshared struct SymbolTable
{
    static const uint32_t kMaxSymbols = 288 + 32;
    static const uint32_t kDistanceCodesBase = 288;

    uint symbols[kMaxSymbols];

    uint32_t scatter(uint sym, uint len, uint offset, uint tid)
    {
        uint32_t msk = match(len, tid);
        if (len != 0)
            symbols[offset + countbits(msk & ltMask(tid))] = sym;
        return msk;
    }

    void init(uint hlit, uint offsets, uint tid)
    {
        if (tid != 15 && tid != 31)
            g_tmp[tid + 1] = offsets;

        if (WaveGetLaneCount() < NUM_THREADS)
            GroupMemoryBarrierWithGroupSync();

        [unroll] for (uint32_t i = 0; i < 256 / NUM_THREADS; i++)
        {
            uint32_t sym = i * NUM_THREADS + tid;
            uint32_t len = g_buf.get4b(sym);
            uint32_t msk = scatter(sym, len, g_tmp[len], tid);
            if (tid == firstbitlow(msk))
                g_tmp[len] += countbits(msk);

            if (WaveGetLaneCount() < NUM_THREADS)
                GroupMemoryBarrierWithGroupSync();
        }

        uint32_t sym = 8 * NUM_THREADS + tid;
        uint32_t len = sym < hlit ? g_buf.get4b(sym) : 0;
        scatter(sym, len, g_tmp[len], tid);

        len = g_buf.get4b(tid + hlit);
        scatter(tid, len, kDistanceCodesBase + g_tmp[16 + len], tid);
    }
} g_lut;

struct DecoderPair
{
    static const uint kMaxCodeLen = 15;
    uint32_t baseCodes[NUM_THREADS];
    uint offsets[NUM_THREADS];

    uint offset(uint i)
    {
        return offsets[i];
    }

    void init(uint counts, uint maxlen, uint tid)
    {
        offsets[tid] = scan16(counts, tid);

        if (WaveGetLaneCount() < NUM_THREADS)
        {
            g_tmp1[tid] = counts;
            GroupMemoryBarrierWithGroupSync();
        }

        uint32_t baseCode = 0;
        [unroll] for (uint32_t i = 1; i < maxlen; i++)
        {
            uint lane = tid & 15;
            uint count = (WaveGetLaneCount() >= NUM_THREADS)
                ? WaveReadLaneAt(counts, (tid & 16) + i)
                : g_tmp1[(tid & 16) + i];

            if (lane >= i)
                baseCode += count << (lane - i);
        }

        uint lane = tid & 15;
        uint tmp = baseCode << (32 - lane);
        baseCodes[tid] = tmp < baseCode || (lane >= maxlen) ? 0xffffffff : tmp;
    }

    uint len4code(uint32_t code, uint base = 0)
    {
        uint len = 1;
        if (code >= baseCodes[7 + base])
            len = 8;
        if (code >= baseCodes[len + 3 + base])
            len += 4;
        if (code >= baseCodes[len + 1 + base])
            len += 2;
        if (code >= baseCodes[len + base])
            len += 1;
        return len;
    }

    uint id4code(uint32_t code, uint len, uint base = 0)
    {
        uint i = len + base - 1;
        return offsets[i] + ((code - baseCodes[i]) >> (32 - len));
    }

    uint decode(uint32_t bits, out uint len, bool isdist = false)
    {
        uint32_t code = reversebits(bits);
        len = len4code(code, isdist ? 16 : 0);
        return g_lut.symbols[id4code(code, len, isdist ? 16 : 0) + (isdist ? 288 : 0)];
    }
};

groupshared DecoderPair dec;

uint32_t GetHistogram(uint32_t cnt, uint32_t len, uint32_t maxlen, uint tid)
{
    g_tmp[tid] = 0;
    if (WaveGetLaneCount() < NUM_THREADS)
        GroupMemoryBarrierWithGroupSync();

    if (len != 0 && tid < cnt)
        InterlockedAdd(g_tmp[len], 1);

    if (WaveGetLaneCount() < NUM_THREADS)
        GroupMemoryBarrierWithGroupSync();

    return g_tmp[tid & 15];
}

uint ReadLenCodes(inout BitReader br, uint hclen, uint tid)
{
    static const uint lane4id[32] = {3, 17, 15, 13, 11, 9, 7, 5, 4, 6, 8, 10, 12, 14, 16, 18,
                                     0,  1,  2,  0,  0, 0, 0, 0, 0, 0, 0,  0,  0,  0,  0,  0};

    uint len = br.read(3, tid, tid < hclen);
    len = shuffle(len, lane4id[tid], tid);
    len &= tid < 19 ? 0xf : 0;
    return len;
}

void UpdateHistograms(uint32_t len, int i, int n, int hlit)
{
    uint32_t cnt = max(min(hlit - i, n), 0);
    if (cnt != 0)
        InterlockedAdd(g_tmp[len], cnt);

    cnt = max(min(i + n - hlit, n), 0);
    if (cnt != 0)
        InterlockedAdd(g_tmp[16 + len], cnt);
}

uint UnpackCodeLengths(inout BitReader br, uint hlit, uint hdist, uint hclen, uint tid, uint dst)
{
    uint len = ReadLenCodes(br, hclen, tid);

    uint cnts = GetHistogram(19, len, 7, tid);
    dec.init(cnts, 7, tid);
    g_lut.scatter(tid, len, dec.offset(len - 1), tid);

    uint32_t count = hlit + hdist;
    uint32_t baseOffset = 0;
    uint32_t lastlen = ~0;

    g_buf.clear(tid);
    g_tmp[tid] = 0;

    if (WaveGetLaneCount() < NUM_THREADS)
        GroupMemoryBarrierWithGroupSync();

    do
    {
        uint len;
        uint32_t bits = br.peek(7 + 7);
        uint sym = dec.decode(bits, len);
        uint idx = sym <= 15 ? 0 : (sym - 15);

        static const uint base[4] = {1, 3, 3, 11};
        static const uint xlen[4] = {0, 2, 3, 7};

        uint n = base[idx] + ((bits >> len) & mask(xlen[idx]));

        uint lane = firstbithigh(vote(sym != 16, tid) & ltMask(tid));

        uint codelen = sym;
        if (sym > 16)
            codelen = 0;
        uint prevlen = shuffle(codelen, lane, tid);

        if (sym == 16)
            codelen = lane == ~0 ? lastlen : prevlen;

        lastlen = broadcast(codelen, NUM_THREADS - 1, tid);

        if (WaveGetLaneCount() < NUM_THREADS)
            GroupMemoryBarrierWithGroupSync();

        baseOffset = scan(n, tid) + baseOffset;

        if (baseOffset < count && codelen != 0)
        {
            UpdateHistograms(codelen, baseOffset, n, hlit);
            set4b(codelen, n, baseOffset);
        }

        br.eat(len + xlen[idx], tid, baseOffset < count);

        baseOffset = broadcast(baseOffset + n, NUM_THREADS - 1, tid);

        if (WaveGetLaneCount() < NUM_THREADS)
            GroupMemoryBarrierWithGroupSync();

    } while (all(baseOffset < count));

    if (WaveGetLaneCount() < NUM_THREADS)
        GroupMemoryBarrierWithGroupSync();

    return g_tmp[tid];
}

// Optimized Coalesced Output Writer (Vectorized & Fast Match Copy with Zero-Division Guard)
void WriteOutput(uint32_t dst, uint32_t offset, uint32_t dist, uint32_t length, uint32_t byte, bool iscopy, uint tid)
{
    dst += offset;
    if (!iscopy && length != 0)
        StoreByte(dst, byte);

    uint32_t maskVal = vote(iscopy, tid);

    while (maskVal)
    {
        uint32_t lane = firstbitlow(maskVal);

        uint32_t off, len, outputPtr;

        if (WaveGetLaneCount() >= NUM_THREADS)
        {
            off = WaveReadLaneAt(dist, lane);
            len = WaveReadLaneAt(length, lane);
            outputPtr = WaveReadLaneAt(dst, lane);
        }
        else
        {
            g_tmp1[tid] = dist;
            g_tmp2[tid] = length;
            g_tmp3[tid] = dst;

            GroupMemoryBarrierWithGroupSync();

            off = g_tmp1[lane];
            len = g_tmp2[lane];
            outputPtr = g_tmp3[lane];
        }

        // Zero-check guard prevents GPU division-by-zero and hardware TDR freezes on malformed payloads
        if (off == 0 || len == 0)
        {
            maskVal &= maskVal - 1;
            continue;
        }

        // Fast path for non-overlapping dword copies (off >= 4)
        if (off >= 4 && len >= 16)
        {
            for (uint32_t i = tid * 4; i + 3 < len; i += NUM_THREADS * 4)
            {
                uint32_t srcOff = outputPtr + (i % off) - off;
                uint32_t d0 = ReadOutputByte(srcOff);
                uint32_t d1 = ReadOutputByte(srcOff + 1);
                uint32_t d2 = ReadOutputByte(srcOff + 2);
                uint32_t d3 = ReadOutputByte(srcOff + 3);
                uint32_t packedWord = d0 | (d1 << 8) | (d2 << 16) | (d3 << 24);

                uint32_t writeTarget = outputPtr + i;
                if ((writeTarget & 3) == 0)
                {
                    output.Store(writeTarget, packedWord);
                }
                else
                {
                    StoreByte(writeTarget, d0);
                    StoreByte(writeTarget + 1, d1);
                    StoreByte(writeTarget + 2, d2);
                    StoreByte(writeTarget + 3, d3);
                }
            }
            uint32_t tailStart = (len / (NUM_THREADS * 4)) * (NUM_THREADS * 4);
            for (uint32_t j = tailStart + tid; j < len; j += NUM_THREADS)
            {
                uint32_t data = ReadOutputByte(outputPtr + j % off - off);
                StoreByte(j + outputPtr, data);
            }
        }
        else
        {
            for (uint32_t i = tid; i < len; i += NUM_THREADS)
            {
                uint32_t data = ReadOutputByte(outputPtr + i % off - off);
                StoreByte(i + outputPtr, data);
            }
        }

        maskVal &= maskVal - 1;
    }
}

uint TranslateSymbol(inout BitReader br, int sym, uint len, uint32_t bits, bool isdist, uint tid, bool p)
{
    static const uint32_t baseDist[] =
    {    1,    2,    3,     4,     5,     7,     9,    13,
        17,   25,   33,    49,    65,    97,   129,   193,
       257,  385,  513,   769,  1025,  1537,  2049,  3073,
      4097, 6145, 8193, 12289, 16385, 24577, 32769, 49153 };

    static const uint32_t baseLength[] =
    {  0,   3,   4,   5,   6,  7,  8,  9,
      10,  11,  13,  15,  17, 19, 23, 27,
      31,  35,  43,  51,  59, 67, 83, 99,
     115, 131, 163, 195, 227,  3,  0 };

    static const uint32_t extraDist[] =
    { 0,  0,  0,  0,  1,  1,  2,  2,
      3,  3,  4,  4,  5,  5,  6,  6,
      7,  7,  8,  8,  9,  9, 10, 10,
     11, 11, 12, 12, 13, 13, 14, 14 };

    static const uint32_t extraLength[] =
    {0, 0, 0, 0, 0,  0, 0, 0,
     0, 1, 1, 1, 1,  2, 2, 2,
     2, 3, 3, 3, 3,  4, 4, 4,
     4, 5, 5, 5, 5, 16, 0 };

    uint32_t base = isdist ? baseDist[sym] : (sym >= 256 ? baseLength[sym - 256] : 1);
    uint32_t n = isdist ? extraDist[sym] : (sym >= 256 ? extraLength[sym - 256] : 0);

    br.eat(len + n, tid, isdist || p);

    return base + ((bits >> len) & mask(n));
}

uint CompressedBlock(inout BitReader br, uint hlit, uint counts, uint dst, uint tid)
{
    dec.init(counts, 15, tid);
    g_lut.init(hlit, dec.offsets[tid], tid);

    uint32_t len;
    uint32_t sym = dec.decode(br.peek(15 + 16), len, false);

    uint32_t eob = vote(sym == 256, tid);
    bool oob = (eob & ltMask(tid)) != 0;

    uint32_t value = TranslateSymbol(br, sym, len, br.peek(), false, tid, !oob);

    uint32_t length = oob ? 0 : value;
    uint32_t offset = scan(length, tid);

    bool iscopy = sym > 256;
    uint32_t byte = sym;

    while (eob == 0)
    {
        sym = dec.decode(br.peek(15 + 16), len, iscopy);

        eob = vote(sym == 256, tid);
        oob = (eob & ltMask(tid)) != 0;

        value = TranslateSymbol(br, sym, len, br.peek(), iscopy, tid, !oob);

        WriteOutput(dst, offset, value, length, byte, iscopy, tid);

        dst += broadcast(offset + length, NUM_THREADS - 1, tid);

        if (WaveGetLaneCount() < NUM_THREADS)
            GroupMemoryBarrierWithGroupSync();

        length = iscopy || oob ? 0 : value;
        offset = scan(length, tid);

        iscopy = sym > 256;
        byte = sym;
    }

    sym = dec.decode(br.peek(15 + 16), len, true);
    iscopy &= !oob;
    uint32_t dist = TranslateSymbol(br, sym, len, br.peek(), iscopy, tid, false);
    WriteOutput(dst, offset, dist, length, byte, iscopy, tid);

    uint res = dst + broadcast(offset + length, NUM_THREADS - 1, tid);

    if (WaveGetLaneCount() < NUM_THREADS)
        GroupMemoryBarrierWithGroupSync();

    return res;
}

// Vectorized Uncompressed Block Writer (Direct 128-bit Store4 without per-byte atomics)
uint32_t UncompressedBlock(inout BitReader br, uint32_t dst, uint32_t size, uint tid)
{
    uint32_t nrounds = size / NUM_THREADS;

    while (nrounds--)
    {
        uint32_t rawByte = br.read(8, tid, true);

        // Vectorized Register Exchange: Pack 32 lane bytes into 8 dwords across lanes 0..7
        if (WaveGetLaneCount() >= NUM_THREADS)
        {
            uint32_t b0 = WaveReadLaneAt(rawByte, (tid & 7) * 4 + 0);
            uint32_t b1 = WaveReadLaneAt(rawByte, (tid & 7) * 4 + 1);
            uint32_t b2 = WaveReadLaneAt(rawByte, (tid & 7) * 4 + 2);
            uint32_t b3 = WaveReadLaneAt(rawByte, (tid & 7) * 4 + 3);
            uint32_t packedWord = b0 | (b1 << 8) | (b2 << 16) | (b3 << 24);

            if (tid < 8)
            {
                output.Store(dst + tid * 4, packedWord);
            }
        }
        else
        {
            g_tmp1[tid] = rawByte;
            GroupMemoryBarrierWithGroupSync();
            if (tid < 8)
            {
                uint32_t b0 = g_tmp1[tid * 4 + 0];
                uint32_t b1 = g_tmp1[tid * 4 + 1];
                uint32_t b2 = g_tmp1[tid * 4 + 2];
                uint32_t b3 = g_tmp1[tid * 4 + 3];
                output.Store(dst + tid * 4, b0 | (b1 << 8) | (b2 << 16) | (b3 << 24));
            }
            GroupMemoryBarrierWithGroupSync();
        }
        dst += NUM_THREADS;
    }

    uint32_t rem = size % NUM_THREADS;
    if (rem != 0)
    {
        uint32_t byte = br.read(8, tid, tid < rem);
        if (tid < rem)
            StoreByte(dst + tid, byte);
        dst += rem;
    }

    return dst;
}

uint FixedCodeLengths(uint tid)
{
    g_buf.data[tid] = tid < 18 ? 0x88888888 : 0x99999999;
    g_buf.data[tid + 32] = tid < 3 ? 0x77777777 : (tid < 4 ? 0x88888888 : 0x55555555);
    return tid == 7 ? 24 : (tid == 8 ? 152 : (tid == 9 ? 112 : tid == 16 + 5 ? 32 : 0));
}

void DecompressTile(in TileParams params, uint tid)
{
    BitReader br;
    br.init(params.inPos, tid);

    bool done;
    uint32_t dst = params.outPos;

    for (uint32_t i = tid; i < (params.outSize + 3) / 4; i += NUM_THREADS)
        output.Store(dst + i * 4, 0);

    do
    {
        uint32_t header = broadcast(br.peek(), 0, tid);

        if (WaveGetLaneCount() < NUM_THREADS)
            GroupMemoryBarrierWithGroupSync();

        done = extract(header, 0, 1) != 0;

        uint32_t btype = extract(header, 1, 2);
        br.eat(3, tid, tid == 0);

        uint counts, size, hlit, hdist;

        switch (btype)
        {
        case 2:
            hlit = extract(header, 3, 5, 257);
            hdist = extract(header, 8, 5, 1);
            br.eat(14, tid, tid == 0);
            counts = UnpackCodeLengths(br, hlit, hdist, extract(header, 13, 4, 4), tid, dst);
        case 1:
            if (btype == 1)
                counts = FixedCodeLengths(tid);

            dst = CompressedBlock(br, btype == 1 ? 288 : hlit, counts, dst, tid);
            break;

        case 0:
            size = broadcast(br.read(16, tid, tid == 0), 0, tid);
            if (WaveGetLaneCount() < NUM_THREADS)
                GroupMemoryBarrierWithGroupSync();

            dst = UncompressedBlock(br, dst, size, tid);
            break;

        default:;
        }

    } while (!done);
}

// Compute shader entry point - work-stealing tile decompression kernel
[numthreads(NUM_THREADS, 1, 1)]
void main(uint tid : SV_GroupThreadID)
{
    int numStreamsLeft = 0;
    if (tid == 0)
        numStreamsLeft = control.Load(0);

    numStreamsLeft = broadcast(numStreamsLeft, 0, tid);

    if (WaveGetLaneCount() < NUM_THREADS)
        GroupMemoryBarrierWithGroupSync();

    [allow_uav_condition] while (numStreamsLeft > 0)
    {
        uint streamIdx = numStreamsLeft - 1;
        const uint streamInPos = control.Load(ControlStreamInOffset(streamIdx));
        uint streamOutPos = control.Load(ControlStreamOutOffset(streamIdx));
        const TileStream tileStream = TileStream::construct(streamInPos, input);

        [allow_uav_condition] while (true)
        {
            uint tileIdx = ~0;

            if (tid == 0)
            {
                scratch.InterlockedAdd(ScratchStreamTileIndexOffset(streamIdx), 1u, tileIdx);
            }

            tileIdx = broadcast(tileIdx, 0, tid);

            if (WaveGetLaneCount() < NUM_THREADS)
                GroupMemoryBarrierWithGroupSync();

            if (tileIdx >= tileStream.GetNumTiles())
                break;

            TileParams params = tileStream.GetTileParams(streamInPos, streamOutPos, tileIdx, input);
            DecompressTile(params, tid);
        }

        if (tid == 0)
        {
            int prevNumStreamsLeft;
            control.InterlockedCompareExchange(0, numStreamsLeft, numStreamsLeft - 1, prevNumStreamsLeft);

            if (prevNumStreamsLeft == numStreamsLeft)
                --numStreamsLeft;
            else
                numStreamsLeft = prevNumStreamsLeft;
        }

        numStreamsLeft = broadcast(numStreamsLeft, 0, tid);

        if (WaveGetLaneCount() < NUM_THREADS)
            GroupMemoryBarrierWithGroupSync();
    }
}
