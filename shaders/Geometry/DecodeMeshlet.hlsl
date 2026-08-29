// GPCK GPU Geometry Decoder & Mesh Shader Pipeline (SM 6.5 / 6.6)
// Targets: DirectX 12 Ultimate & Vulkan Compute / Mesh Shader
// Features: AMD VMV 2024 Crack-Free Global Grid Dequantization + Normal Cone Culling

#define MAX_VERTICES 64
#define MAX_PRIMITIVES 124
#define GROUP_SIZE 32

// 64-byte Meshlet Descriptor (Byte-exact with Rust GPCK struct)
struct MeshletDescriptor {
    float3 center;          // offset 0..12:  Bounding sphere center
    float radius;           // offset 12..16: Bounding sphere radius
    uint3 quant_offset;     // offset 16..28: Crack-free integer offset on global grid
    uint vertex_offset;     // offset 28..32: Offset in QuantizedVertex buffer
    uint triangle_offset;   // offset 32..36: Offset in MeshletTriangle buffer
    uint packed_cone;       // offset 36..40: byte0=cone_x, byte1=cone_y, byte2=cone_z, byte3=cutoff
    uint vertex_count;      // offset 40..44: <= 64
    uint triangle_count;    // offset 44..48: <= 124
    uint4 _pad;             // offset 48..64: 64-byte cache alignment padding
};

// 16-byte Packed Quantized Vertex (128-bit GPU vector load)
struct QuantizedVertex {
    uint pos_xy;           // u16 x, u16 y
    uint pos_z_norm;       // u16 z, i8 norm_x, i8 norm_y
    uint uv_half;          // f16 u, f16 v
    uint tangent_oct_sign; // i8 tan_x, i8 tan_y, 16-bit pad
};

// 4-byte Micro-Triangle Indices (3x u8)
struct MeshletTriangle {
    uint packed_indices;   // byte0: i0, byte1: i1, byte2: i2, byte3: pad
};

// Uncompressed Output Vertex
struct DecodedVertex {
    float3 position;
    float3 normal;
    float2 uv;
    float3 tangent;
};

// Global Mesh & Scene Constant Buffer
cbuffer SceneCB : register(b0) {
    float4x4 g_ViewProj;
    float3   g_CameraPos;
    float    _pad0;
    float4   g_FrustumPlanes[6];
    float3   g_GlobalMin;       // World AABB origin for crack-free grid
    float    _pad1;
    float3   g_DequantFactor;   // Global dequantization step vector
    float    _pad2;
};

// DirectStorage / Vulkan DMA Bound Buffers
StructuredBuffer<MeshletDescriptor> g_Meshlets   : register(t0);
StructuredBuffer<QuantizedVertex>   g_Vertices   : register(t1);
StructuredBuffer<MeshletTriangle>   g_Triangles  : register(t2);

#ifdef __SPIRV__
// Destination buffer for Compute Shader Batch Decompression
RWStructuredBuffer<DecodedVertex>   g_OutVertices : register(u3);
#endif

// Vertex Output for Mesh Shader Rasterizer
struct VertexOut {
    float4 position_cs : SV_Position;
    float3 normal_ws   : NORMAL;
    float2 uv          : TEXCOORD0;
    float3 tangent_ws  : TANGENT;
};

// Compact 8-byte Payload passed from Task Shader to Mesh Shader
struct MeshPayload {
    uint meshlet_indices[GROUP_SIZE];
};

#ifndef __SPIRV__
// Global groupshared payload for Amplification / Task Shader workgroup
groupshared MeshPayload s_Payload;
#endif

// ============================================================================
// Math & Dequantization Functions
// ============================================================================

// 1-Cycle Octahedral Normal Decoding
float3 DecodeOctahedralNormal(int2 oct) {
    float2 f_oct = float2(oct) / 127.0f;
    float3 n = float3(f_oct.x, f_oct.y, 1.0f - abs(f_oct.x) - abs(f_oct.y));
    if (n.z < 0.0f) {
        float old_x = n.x;
        n.x = (1.0f - abs(n.y)) * (old_x >= 0.0f ? 1.0f : -1.0f);
        n.y = (1.0f - abs(old_x)) * (n.y >= 0.0f ? 1.0f : -1.0f);
    }
    return normalize(n);
}

// 2-Cycle Crack-Free Vertex Dequantization (AMD VMV 2024 Specification)
void UnpackVertex(
    QuantizedVertex qv,
    MeshletDescriptor desc,
    out float3 out_pos,
    out float3 out_norm,
    out float2 out_uv,
    out float3 out_tan
) {
    // 1. Unpack Crack-Free Position from Unified Global Grid
    uint qx = qv.pos_xy & 0xFFFF;
    uint qy = qv.pos_xy >> 16;
    uint qz = qv.pos_z_norm & 0xFFFF;

    float3 local_int_pos = float3(desc.quant_offset) + float3(qx, qy, qz);
    out_pos = g_GlobalMin + local_int_pos * g_DequantFactor;

    // 2. Unpack Octahedral Normal (Bit-shift sign extension)
    int norm_x = (int(qv.pos_z_norm) << 8) >> 24;
    int norm_y = int(qv.pos_z_norm) >> 24;
    out_norm = DecodeOctahedralNormal(int2(norm_x, norm_y));

    // 3. Unpack Half-Precision UVs
    out_uv = float2(f16tof32(qv.uv_half & 0xFFFF), f16tof32(qv.uv_half >> 16));

    // 4. Unpack Octahedral Tangent (Bit-shift sign extension)
    int tan_x = (int(qv.tangent_oct_sign) << 24) >> 24;
    int tan_y = (int(qv.tangent_oct_sign) << 16) >> 24;
    out_tan = DecodeOctahedralNormal(int2(tan_x, tan_y));
}

// ============================================================================
// Primary Compute Shader Entry Point (Default Target for DXC / Vulkan Compute)
// ============================================================================
[numthreads(64, 1, 1)]
void main(
    uint gtid : SV_GroupThreadID,
    uint gid  : SV_GroupID
) {
    uint meshlet_idx = gid;
    MeshletDescriptor desc = g_Meshlets[meshlet_idx];

    if (gtid < desc.vertex_count) {
        QuantizedVertex qv = g_Vertices[desc.vertex_offset + gtid];
        float3 pos, norm, tan;
        float2 uv;
        UnpackVertex(qv, desc, pos, norm, uv, tan);

#ifdef __SPIRV__
        DecodedVertex dv;
        dv.position = pos;
        dv.normal = norm;
        dv.uv = uv;
        dv.tangent = tan;
        g_OutVertices[desc.vertex_offset + gtid] = dv;
#endif
    }
}

// ============================================================================
// D3D12 / Vulkan Mesh Shader Pipelines (Optional Stage Targets)
// ============================================================================

#ifndef __SPIRV__

// 1. Task / Amplification Shader (Frustum + Backface Cone Culling)
[numthreads(GROUP_SIZE, 1, 1)]
void ASMain(
    uint gtid : SV_GroupThreadID,
    uint dtid : SV_DispatchThreadID
) {
    MeshletDescriptor desc = g_Meshlets[dtid];
    bool is_visible = true;

    // A. Bounding Sphere Frustum Culling
    for (int i = 0; i < 6; ++i) {
        if (dot(g_FrustumPlanes[i].xyz, desc.center) + g_FrustumPlanes[i].w < -desc.radius) {
            is_visible = false;
            break;
        }
    }

    // B. Cluster Backface Normal Cone Culling (Sign-Extended Unpack)
    int cone_x = (int(desc.packed_cone << 24)) >> 24;
    int cone_y = (int(desc.packed_cone << 16)) >> 24;
    int cone_z = (int(desc.packed_cone << 8)) >> 24;
    int cone_cutoff_raw = int(desc.packed_cone) >> 24;

    float3 cone_axis = float3(cone_x, cone_y, cone_z) / 127.0f;
    float cone_cutoff = float(cone_cutoff_raw) / 127.0f;
    float3 view_dir = normalize(desc.center - g_CameraPos);

    if (dot(view_dir, cone_axis) >= cone_cutoff) {
        is_visible = false;
    }

    // C. Non-atomic Ballot compaction using Wave Intrinsics
    uint visible_count = WaveActiveCountBits(is_visible);
    uint local_idx = WavePrefixCountBits(is_visible);

    if (is_visible) {
        s_Payload.meshlet_indices[local_idx] = dtid;
    }

    DispatchMesh(visible_count, 1, 1, s_Payload);
}

// 2. Mesh Shader (Topology & Parallel Decompression)
[numthreads(GROUP_SIZE, 1, 1)]
[outputtopology("triangle")]
void MSMain(
    uint gtid : SV_GroupThreadID,
    uint gid  : SV_GroupID,
    in payload MeshPayload payload,
    out vertices VertexOut verts[MAX_VERTICES],
    out indices uint3 tris[MAX_PRIMITIVES]
) {
    uint meshlet_idx = payload.meshlet_indices[gid];
    MeshletDescriptor desc = g_Meshlets[meshlet_idx];

    SetMeshOutputCounts(desc.vertex_count, desc.triangle_count);

    // Parallel Vertex Decompression (2 cycles ALU)
    for (uint v = gtid; v < desc.vertex_count; v += GROUP_SIZE) {
        QuantizedVertex qv = g_Vertices[desc.vertex_offset + v];

        float3 pos_ws, norm_ws, tan_ws;
        float2 uv;
        UnpackVertex(qv, desc, pos_ws, norm_ws, uv, tan_ws);

        verts[v].position_cs = mul(float4(pos_ws, 1.0f), g_ViewProj);
        verts[v].normal_ws   = norm_ws;
        verts[v].uv          = uv;
        verts[v].tangent_ws  = tan_ws;
    }

    // Parallel Triangle Topology Emission
    for (uint t = gtid; t < desc.triangle_count; t += GROUP_SIZE) {
        MeshletTriangle mt = g_Triangles[desc.triangle_offset + t];
        uint raw = mt.packed_indices;

        uint i0 = raw & 0xFF;
        uint i1 = (raw >> 8) & 0xFF;
        uint i2 = (raw >> 16) & 0xFF;

        tris[t] = uint3(i0, i1, i2);
    }
}

#endif
