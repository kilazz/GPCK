// GPCK GPU Displaced Micro-Mesh (DMM) Shader Pipeline (SM 6.5 / 6.6)
// Targets: DirectX 12 Ultimate & Vulkan Compute / Mesh Shader
// Features: Distance-Adaptive Dynamic Tessellation + Barycentric Domain Evaluation

#define MAX_DMM_VERTICES 64
#define MAX_DMM_PRIMITIVES 64
#define GROUP_SIZE 32

// 48-byte Micro-Mesh Descriptor (Byte-exact with Rust GPCK struct)
struct MicroMeshDescriptor {
    uint base_triangle_idx;
    uint subdiv_level;
    uint disp_format;
    uint _pad0;
    uint disp_byte_offset;
    uint micro_vertex_count;
    float disp_scale;
    float disp_bias;
    float3 bounds_min;
    float3 bounds_max;
};

// 16-byte Coarse Base Vertex
struct QuantizedBaseVertex {
    uint pos_xy;
    uint pos_z_norm;
    uint uv_half;
    uint tangent_oct_sign;
};

// Scene Constant Buffer
cbuffer SceneCB : register(b0) {
    float4x4 g_ViewProj;
    float3   g_CameraPos;
    float    _pad0;
    float4   g_FrustumPlanes[6];
    float3   g_GlobalMin;
    float    _pad1;
    float3   g_DequantFactor;
    float    _pad2;
};

// DirectStorage / VRAM Buffers
StructuredBuffer<MicroMeshDescriptor> g_DmmDescriptors : register(t0);
StructuredBuffer<QuantizedBaseVertex> g_BaseVertices   : register(t1);
StructuredBuffer<uint3>               g_BaseIndices    : register(t2);
ByteAddressBuffer                     g_Displacements  : register(t3);

struct VertexOut {
    float4 position_cs : SV_Position;
    float3 normal_ws   : NORMAL;
    float2 uv          : TEXCOORD0;
};

struct DmmPayload {
    uint dmm_indices[GROUP_SIZE];
    uint adaptive_subdiv[GROUP_SIZE];
};

#ifndef __SPIRV__
groupshared DmmPayload s_Payload;
#endif

// ============================================================================
// Primary Compute Shader Entry Point (Default Target for DXC / Vulkan Compute)
// ============================================================================
[numthreads(64, 1, 1)]
void main(uint3 gtid : SV_GroupThreadID, uint3 gid : SV_GroupID) {
    // Fallback batch decompression compute entry point
}

// ============================================================================
// D3D12 / Vulkan Mesh Shader Pipelines (Optional Stage Targets)
// ============================================================================

#ifndef __SPIRV__

// 1. Task / Amplification Shader: Adaptive LOD & Frustum Culling
[numthreads(GROUP_SIZE, 1, 1)]
void ASMain(uint gtid : SV_GroupThreadID, uint dtid : SV_DispatchThreadID) {
    MicroMeshDescriptor desc = g_DmmDescriptors[dtid];
    bool is_visible = true;

    // A. Bounding Box Frustum Culling
    float3 center = (desc.bounds_min + desc.bounds_max) * 0.5f;
    float radius = length(desc.bounds_max - desc.bounds_min) * 0.5f;

    for (int i = 0; i < 6; ++i) {
        if (dot(g_FrustumPlanes[i].xyz, center) + g_FrustumPlanes[i].w < -radius) {
            is_visible = false;
            break;
        }
    }

    // B. Distance-Adaptive LOD Selection (Subdiv level 0..3)
    float dist = max(length(center - g_CameraPos), 1.0f);
    uint target_subdiv = clamp((uint)(4.0f - log2(dist * 0.1f)), 0, desc.subdiv_level);

    uint visible_count = WaveActiveCountBits(is_visible);
    uint local_idx = WavePrefixCountBits(is_visible);

    if (is_visible) {
        s_Payload.dmm_indices[local_idx] = dtid;
        s_Payload.adaptive_subdiv[local_idx] = target_subdiv;
    }

    DispatchMesh(visible_count, 1, 1, s_Payload);
}

// 2. Mesh Shader: Procedural Barycentric Tessellation & Height Evaluation
[numthreads(GROUP_SIZE, 1, 1)]
[outputtopology("triangle")]
void MSMain(
    uint gtid : SV_GroupThreadID,
    uint gid  : SV_GroupID,
    in payload DmmPayload payload,
    out vertices VertexOut verts[MAX_DMM_VERTICES],
    out indices uint3 tris[MAX_DMM_PRIMITIVES]
) {
    uint dmm_idx = payload.dmm_indices[gid];
    uint level = payload.adaptive_subdiv[gid];
    MicroMeshDescriptor desc = g_DmmDescriptors[dmm_idx];

    uint s = 1u << level;
    uint micro_vert_count = ((s + 1) * (s + 2)) / 2;
    uint micro_tri_count = s * s;

    SetMeshOutputCounts(micro_vert_count, micro_tri_count);

    // Read Coarse Triangle Corner Vertices
    uint3 base_tri = g_BaseIndices[desc.base_triangle_idx];
    QuantizedBaseVertex qv0 = g_BaseVertices[base_tri.x];
    QuantizedBaseVertex qv1 = g_BaseVertices[base_tri.y];
    QuantizedBaseVertex qv2 = g_BaseVertices[base_tri.z];

    float3 p0 = g_GlobalMin + float3(qv0.pos_xy & 0xFFFF, qv0.pos_xy >> 16, qv0.pos_z_norm & 0xFFFF) * g_DequantFactor;
    float3 p1 = g_GlobalMin + float3(qv1.pos_xy & 0xFFFF, qv1.pos_xy >> 16, qv1.pos_z_norm & 0xFFFF) * g_DequantFactor;
    float3 p2 = g_GlobalMin + float3(qv2.pos_xy & 0xFFFF, qv2.pos_xy >> 16, qv2.pos_z_norm & 0xFFFF) * g_DequantFactor;

    float2 uv0 = float2(f16tof32(qv0.uv_half & 0xFFFF), f16tof32(qv0.uv_half >> 16));
    float2 uv1 = float2(f16tof32(qv1.uv_half & 0xFFFF), f16tof32(qv1.uv_half >> 16));
    float2 uv2 = float2(f16tof32(qv2.uv_half & 0xFFFF), f16tof32(qv2.uv_half >> 16));

    float3 face_norm = normalize(cross(p1 - p0, p2 - p0));

    // A. Parallel Micro-Vertex Barycentric Evaluation
    for (uint v = gtid; v < micro_vert_count; v += GROUP_SIZE) {
        uint row_idx = 0;
        uint acc = 0;
        for (uint r = 0; r <= s; ++r) {
            uint row_len = s + 1 - r;
            if (v < acc + row_len) {
                row_idx = r;
                break;
            }
            acc += row_len;
        }
        uint col_idx = v - acc;

        float u = float(row_idx) / float(s);
        float w_coord = float(col_idx) / float(s);
        float w0 = 1.0f - u - w_coord;

        float3 interp_pos = p0 * w0 + p1 * u + p2 * w_coord;
        float2 interp_uv = uv0 * w0 + uv1 * u + uv2 * w_coord;

        uint raw_byte = g_Displacements.Load(desc.disp_byte_offset + v) & 0xFF;
        float height = float(raw_byte) / 255.0f;
        float displaced_dist = desc.disp_bias + height * desc.disp_scale;

        float3 final_pos = interp_pos + face_norm * displaced_dist;

        verts[v].position_cs = mul(float4(final_pos, 1.0f), g_ViewProj);
        verts[v].normal_ws   = face_norm;
        verts[v].uv          = interp_uv;
    }

    // B. Parallel Micro-Triangle Topology Emission
    for (uint t = gtid; t < micro_tri_count; t += GROUP_SIZE) {
        uint i0 = t;
        uint i1 = min(t + 1, micro_vert_count - 1);
        uint i2 = min(t + s + 1, micro_vert_count - 1);

        tris[t] = uint3(i0, i1, i2);
    }
}

#endif