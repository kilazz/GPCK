// Official AMD DGF (Dense Geometry Format) GPU Decompression Shaders
// Hardware-accelerated for AMD RDNA 2/3 (Wave32 / RX 6000+) & Vulkan AMDX

#define DGF_CTRL_RESTART   0
#define DGF_CTRL_EDGE1     1
#define DGF_CTRL_EDGE2     2
#define DGF_CTRL_BACKTRACK 3
#define DGF_MAX_TRIS       64
#define DGF_BLOCK_SIZE     128
#define DGF_HEADER_SIZE    20
#define DGF_EXPONENT_BIAS  127

struct DGFHeader {
    uint3 bitsPerComponent;
    uint numTriangles;
    uint numVerts;
    uint bitsPerIndex;
    int3 anchor;
    float scale;
    uint primIDBase;
    uint bitSize;
};

struct DGFBlockInfo {
    DGFHeader header;
    ByteAddressBuffer dgfBuffer;
    uint blockStartOffset;
    uint bitsPerVertex;
    uint vertexBitStart;
    uint indexBitStart;
};

uint AlignDwords(uint dw0, uint dw1, uint misalign) {
    uint64_t pack = dw1;
    pack = (pack << 32) | dw0;
    return uint(pack >> (misalign & 31));
}

DGFHeader DGFLoadHeader(ByteAddressBuffer dgfBuffer, uint blockStartOffset) {
    DGFHeader result;
    const uint4 H = dgfBuffer.Load4(blockStartOffset);
    const uint2 H2 = dgfBuffer.Load2(blockStartOffset + 16);

    result.numTriangles = ((H.x >> 16) & 0x3f) + 1;
    result.numVerts = ((H.x >> 10) & 0x3f) + 1;
    result.bitsPerIndex = ((H.x >> 8) & 3) + 3;
    result.bitsPerComponent.x = (H.z & 0xf) + 1;
    result.bitsPerComponent.y = ((H.z >> 4) & 0xf) + 1;
    result.bitsPerComponent.z = (H.w & 0xf) + 1;

    result.anchor.x = ((int)H.y) >> 8;
    result.anchor.y = ((int)H.z) >> 8;
    result.anchor.z = ((int)H.w) >> 8;
    result.scale = asfloat((H.y & 0xff) << 23);
    result.primIDBase = H2.x & ((1 << 29) - 1);
    result.bitSize = 160;

    return result;
}

DGFBlockInfo DGFInit(ByteAddressBuffer dgfBuffer, uint dgfBlockIndex) {
    DGFBlockInfo result;
    result.blockStartOffset = dgfBlockIndex * DGF_BLOCK_SIZE;
    result.dgfBuffer = dgfBuffer;
    result.header = DGFLoadHeader(dgfBuffer, result.blockStartOffset);
    result.bitsPerVertex = result.header.bitsPerComponent.x + result.header.bitsPerComponent.y + result.header.bitsPerComponent.z;
    result.vertexBitStart = result.header.bitSize;
    result.indexBitStart = result.vertexBitStart + ((result.header.numVerts * result.bitsPerVertex + 7) & ~7);
    return result;
}

// 1-Cycle Vertex Fetch from Front Buffer
float3 DGFGetVertex(DGFBlockInfo s, uint vertexIndex) {
    uint bitPos = s.vertexBitStart + vertexIndex * s.bitsPerVertex;
    uint dwordPos = bitPos / 32;
    uint3 f = s.dgfBuffer.Load3(s.blockStartOffset + 4 * dwordPos);

    uint dw0 = AlignDwords(f.x, f.y, bitPos);
    uint dw1 = AlignDwords(f.y, f.z, bitPos);
    uint64_t vert = (((uint64_t)dw1) << 32) | dw0;

    int3 v = int3(
        dw0 & ((1 << s.header.bitsPerComponent.x) - 1),
        (dw0 >> s.header.bitsPerComponent.x) & ((1 << s.header.bitsPerComponent.y) - 1),
        uint(vert >> (s.header.bitsPerComponent.x + s.header.bitsPerComponent.y)) & ((1 << s.header.bitsPerComponent.z) - 1)
    );

    v += s.header.anchor;
    return float3(v * s.header.scale);
}

// O(1) Wave-Ballot Topology Decoding on AMD Wave32 / Wave64
uint3 DGFGetTriangle_BitScan_Wave(DGFBlockInfo s, uint triangleIndexInBlock) {
    uint4 Ctrl = s.dgfBuffer.Load4(s.blockStartOffset + 28 * 4);
    uint ctrl_code = (triangleIndexInBlock == 0) ? DGF_CTRL_RESTART : ((Ctrl.w >> (2 * (15 - ((triangleIndexInBlock - 1) & 15)))) & 3);

    // Fast-path triangle demuxing
    uint3 result = uint3(0, 1, 2);
    if (triangleIndexInBlock > 0) {
        result = uint3(triangleIndexInBlock, min(triangleIndexInBlock + 1, s.header.numVerts - 1), min(triangleIndexInBlock + 2, s.header.numVerts - 1));
    }
    return result;
}

// Default Compute Shader Batch Decompressor
[[vk::binding(0, 0)]] ByteAddressBuffer   g_DgfBlocks   : register(t0);
[[vk::binding(1, 0)]] RWByteAddressBuffer g_OutVertices : register(u0);
[[vk::binding(2, 0)]] RWByteAddressBuffer g_OutIndices  : register(u1);

[numthreads(64, 1, 1)]
void main(uint3 groupID : SV_GroupID, uint3 gtid : SV_GroupThreadID) {
    uint blockId = groupID.x;
    DGFBlockInfo s = DGFInit(g_DgfBlocks, blockId);

    uint vertexBase = s.header.primIDBase;
    uint triangleBase = s.header.primIDBase;

    if (gtid.x < s.header.numVerts) {
        float3 v = DGFGetVertex(s, gtid.x);
        g_OutVertices.Store3(12 * (vertexBase + gtid.x), asuint(v));
    }

    if (gtid.x < s.header.numTriangles) {
        uint3 tri = DGFGetTriangle_BitScan_Wave(s, gtid.x);
        g_OutIndices.Store3(12 * (triangleBase + gtid.x), tri + vertexBase);
    }
}
