// crates/gpck_core/src/geometry/meshlet.rs
//! # Crack-Free Meshlet Partitioning & Global Grid Quantization
//!
//! Implements the AMD VMV 2024 "Towards Practical Meshlet Compression" algorithm.
//! Uses a global-grid quantization step derived from the largest cluster extents,
//! eliminating T-junction cracks and boundary seam artifacts while compressing
//! vertex positions into local 16-bit integers.

use super::octahedral::{encode_octahedral_normal, encode_octahedral_tangent, f32_to_f16};
use crate::core::error::GpckResult;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;

/// FourCC magic identifier for GPCK meshlet binary containers ("MSHL" = 0x4D53484C).
pub const MESHLET_MAGIC: u32 = 0x4D53484C;

/// Target maximum vertices per cluster (matches AMD RDNA and NVIDIA hardware workgroups).
pub const MAX_MESHLET_VERTICES: usize = 64;

/// Target maximum triangles per cluster.
pub const MAX_MESHLET_TRIANGLES: usize = 124;

/// Cache-line aligned 64-byte `.gmesh` container header.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub struct MeshletContainerHeader {
    pub magic: u32,                // "MSHL" (0x4D53484C) (4 B)
    pub version: u32,              // Version 2 (4 B)
    pub meshlet_count: u32,        // Total number of meshlets (4 B)
    pub total_vertex_count: u32,   // Total quantized vertices (4 B)
    pub total_triangle_count: u32, // Total micro-triangles (4 B)
    pub global_min: [f32; 3],      // Global AABB minimum (12 B)
    pub dequant_factor: [f32; 3],  // Global grid cell scaling factor (12 B)
    pub global_max: [f32; 3],      // Global AABB maximum (12 B)
    pub _reserved: [u32; 2],       // Padding to exactly 64 bytes (8 B)
}

/// Cache-line aligned 64-byte Meshlet Descriptor for Task/Mesh Shader hardware streaming.
/// Exactly matches the binary layout of the HLSL MeshletDescriptor struct.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub struct MeshletDescriptor {
    pub center: [f32; 3], // 12 B (offset 0..12):  Bounding sphere center (Frustum culling)
    pub radius: f32,      // 4 B  (offset 12..16): Bounding sphere radius
    pub quant_offset: [u32; 3], // 12 B (offset 16..28): Integer offset on the global quantization grid
    pub vertex_offset: u32,     // 4 B  (offset 28..32): Index in the global QuantizedVertex buffer
    pub triangle_offset: u32,   // 4 B  (offset 32..36): Index in the global MeshletTriangle buffer
    pub packed_cone: u32, // 4 B  (offset 36..40): byte0=axis.x, byte1=axis.y, byte2=axis.z, byte3=cutoff
    pub vertex_count: u32, // 4 B  (offset 40..44): Number of vertices (<= 64)
    pub triangle_count: u32, // 4 B  (offset 44..48): Number of triangles (<= 124)
    pub _pad: [u32; 4],   // 16 B (offset 48..64): Padding to exactly 64 bytes
}

/// 16-byte Packed Quantized Vertex (128-bit GPU vector load friendly).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq, Eq, Default)]
pub struct QuantizedVertex {
    pub position_quant: [u16; 3],  // 6 B: Local grid coordinates [0..65535]
    pub normal_oct: [i8; 2],       // 2 B: Octahedral unit normal
    pub uv_half: [u16; 2],         // 4 B: Texture coordinates in IEEE-754 f16
    pub tangent_oct_sign: [i8; 2], // 2 B: Octahedral tangent + bitangent sign
    pub _pad: [u8; 2],             // 2 B: Align to 16 bytes
}

/// 4-byte Micro-Triangle containing 8-bit cluster-local vertex indices.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq, Eq, Default)]
pub struct MeshletTriangle {
    pub i0: u8,
    pub i1: u8,
    pub i2: u8,
    pub _pad: u8, // 4-byte dword alignment
}

// Compile-time assertions verifying exact GPU memory alignment
const _: () = assert!(std::mem::size_of::<MeshletContainerHeader>() == 64);
const _: () = assert!(std::mem::size_of::<MeshletDescriptor>() == 64);
const _: () = assert!(std::mem::size_of::<QuantizedVertex>() == 16);
const _: () = assert!(std::mem::size_of::<MeshletTriangle>() == 4);

impl MeshletDescriptor {
    /// Packs 3-component normal cone axis and cutoff angle into a single 32-bit word.
    #[inline(always)]
    pub fn pack_cone(axis: [i8; 3], cutoff: i8) -> u32 {
        (axis[0] as u8 as u32)
            | ((axis[1] as u8 as u32) << 8)
            | ((axis[2] as u8 as u32) << 16)
            | ((cutoff as u8 as u32) << 24)
    }

    /// Unpacks the 3-component normal cone axis and cutoff angle from the 32-bit word.
    #[inline(always)]
    pub fn unpack_cone(&self) -> ([i8; 3], i8) {
        let ax = (self.packed_cone & 0xFF) as u8 as i8;
        let ay = ((self.packed_cone >> 8) & 0xFF) as u8 as i8;
        let az = ((self.packed_cone >> 16) & 0xFF) as u8 as i8;
        let cut = ((self.packed_cone >> 24) & 0xFF) as u8 as i8;
        ([ax, ay, az], cut)
    }
}

/// Uncompressed 3D vertex input representation.
#[derive(Debug, Clone, Default)]
pub struct RawVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub tangent: [f32; 4], // xyz = tangent vector, w = bitangent sign (+1.0 / -1.0)
}

struct UnquantizedCluster {
    vertices: Vec<RawVertex>,
    triangles: Vec<MeshletTriangle>,
    aabb_min: [f32; 3],
    aabb_max: [f32; 3],
    center: [f32; 3],
    radius: f32,
    cone_axis: [i8; 3],
    cone_cutoff: i8,
}

pub struct MeshletBuilder;

impl MeshletBuilder {
    /// Partitions an indexed triangle mesh into crack-free quantized meshlets.
    pub fn build_container(vertices: &[RawVertex], indices: &[u32]) -> GpckResult<Vec<u8>> {
        if vertices.is_empty() || indices.is_empty() {
            return Ok(Vec::new());
        }

        // ====================================================================
        // Phase 1: Compute Global Mesh Bounds
        // ====================================================================
        let mut global_min = [f32::MAX; 3];
        let mut global_max = [f32::MIN; 3];
        for v in vertices {
            for c in 0..3 {
                global_min[c] = global_min[c].min(v.position[c]);
                global_max[c] = global_max[c].max(v.position[c]);
            }
        }

        let mut global_delta = [0.0f32; 3];
        for c in 0..3 {
            global_delta[c] = (global_max[c] - global_min[c]).max(1e-5);
        }

        // ====================================================================
        // Phase 2: Greedy Meshlet Clustering (<= 64 verts, <= 124 tris)
        // ====================================================================
        let mut raw_clusters: Vec<UnquantizedCluster> = Vec::new();
        let num_triangles = indices.len() / 3;
        let mut tri_idx = 0;

        while tri_idx < num_triangles {
            let mut cluster_vertices: Vec<RawVertex> = Vec::with_capacity(MAX_MESHLET_VERTICES);
            let mut cluster_triangles: Vec<MeshletTriangle> =
                Vec::with_capacity(MAX_MESHLET_TRIANGLES);
            let mut global_to_local_map: HashMap<u32, u8> = HashMap::new();

            while tri_idx < num_triangles && cluster_triangles.len() < MAX_MESHLET_TRIANGLES {
                let i0 = indices[tri_idx * 3];
                let i1 = indices[tri_idx * 3 + 1];
                let i2 = indices[tri_idx * 3 + 2];

                let mut needed_new_vertices = 0;
                if !global_to_local_map.contains_key(&i0) {
                    needed_new_vertices += 1;
                }
                if !global_to_local_map.contains_key(&i1) {
                    needed_new_vertices += 1;
                }
                if !global_to_local_map.contains_key(&i2) {
                    needed_new_vertices += 1;
                }

                if cluster_vertices.len() + needed_new_vertices > MAX_MESHLET_VERTICES {
                    break;
                }

                let l0 = *global_to_local_map.entry(i0).or_insert_with(|| {
                    let idx = cluster_vertices.len() as u8;
                    cluster_vertices.push(vertices[i0 as usize].clone());
                    idx
                });
                let l1 = *global_to_local_map.entry(i1).or_insert_with(|| {
                    let idx = cluster_vertices.len() as u8;
                    cluster_vertices.push(vertices[i1 as usize].clone());
                    idx
                });
                let l2 = *global_to_local_map.entry(i2).or_insert_with(|| {
                    let idx = cluster_vertices.len() as u8;
                    cluster_vertices.push(vertices[i2 as usize].clone());
                    idx
                });

                cluster_triangles.push(MeshletTriangle {
                    i0: l0,
                    i1: l1,
                    i2: l2,
                    _pad: 0,
                });
                tri_idx += 1;
            }

            // Cluster Bounding Box, Sphere & Normal Cone
            let mut cluster_min = [f32::MAX; 3];
            let mut cluster_max = [f32::MIN; 3];
            let mut center = [0.0f32; 3];
            let mut avg_normal = [0.0f32; 3];

            for v in &cluster_vertices {
                for c in 0..3 {
                    cluster_min[c] = cluster_min[c].min(v.position[c]);
                    cluster_max[c] = cluster_max[c].max(v.position[c]);
                    center[c] += v.position[c];
                    avg_normal[c] += v.normal[c];
                }
            }

            let vert_count_f = cluster_vertices.len().max(1) as f32;
            for val in &mut center {
                *val /= vert_count_f;
            }

            let mut radius = 0.0f32;
            for v in &cluster_vertices {
                let d2 = (v.position[0] - center[0]).powi(2)
                    + (v.position[1] - center[1]).powi(2)
                    + (v.position[2] - center[2]).powi(2);
                radius = radius.max(d2.sqrt());
            }

            let avg_norm_len =
                (avg_normal[0].powi(2) + avg_normal[1].powi(2) + avg_normal[2].powi(2)).sqrt();

            let (cone_axis, cone_cutoff) = if avg_norm_len < 1e-4 {
                // Degenerate/spherical normal distribution: disable cone culling
                ([0i8, 0, 0], 127i8)
            } else {
                let cone_axis_f = [
                    avg_normal[0] / avg_norm_len,
                    avg_normal[1] / avg_norm_len,
                    avg_normal[2] / avg_norm_len,
                ];

                let mut min_dot = 1.0f32;
                for v in &cluster_vertices {
                    let dot = v.normal[0] * cone_axis_f[0]
                        + v.normal[1] * cone_axis_f[1]
                        + v.normal[2] * cone_axis_f[2];
                    min_dot = min_dot.min(dot);
                }

                let axis = [
                    (cone_axis_f[0] * 127.0).clamp(-127.0, 127.0) as i8,
                    (cone_axis_f[1] * 127.0).clamp(-127.0, 127.0) as i8,
                    (cone_axis_f[2] * 127.0).clamp(-127.0, 127.0) as i8,
                ];
                let cutoff = (min_dot * 127.0).clamp(-127.0, 127.0) as i8;
                (axis, cutoff)
            };

            raw_clusters.push(UnquantizedCluster {
                vertices: cluster_vertices,
                triangles: cluster_triangles,
                aabb_min: cluster_min,
                aabb_max: cluster_max,
                center,
                radius,
                cone_axis,
                cone_cutoff,
            });
        }

        // ====================================================================
        // Phase 3: AMD VMV 2024 Global Grid Computation
        // ====================================================================
        let mut largest_meshlet_delta = [0.0f32; 3];
        for cluster in &raw_clusters {
            for (c, delta_item) in largest_meshlet_delta.iter_mut().enumerate() {
                let delta = cluster.aabb_max[c] - cluster.aabb_min[c];
                *delta_item = delta_item.max(delta);
            }
        }

        let mut quant_factor = [0.0f32; 3];
        let mut dequant_factor = [0.0f32; 3];

        for c in 0..3 {
            let max_delta = largest_meshlet_delta[c].max(1e-5);
            let meshlet_quant_step = max_delta / 65535.0;
            let global_quant_states =
                ((global_delta[c] / meshlet_quant_step).ceil() as u32).max(65536);

            quant_factor[c] = (global_quant_states - 1) as f32 / global_delta[c];
            dequant_factor[c] = global_delta[c] / (global_quant_states - 1) as f32;
        }

        // ====================================================================
        // Phase 4: Crack-Free Quantization & Output Packing
        // ====================================================================
        let mut meshlet_descriptors = Vec::with_capacity(raw_clusters.len());
        let mut all_quantized_vertices = Vec::new();
        let mut all_triangles = Vec::new();

        for cluster in raw_clusters {
            let mut quant_offset = [0u32; 3];
            for c in 0..3 {
                quant_offset[c] =
                    ((cluster.aabb_min[c] - global_min[c]) * quant_factor[c] + 0.5) as u32;
            }

            let vert_start = all_quantized_vertices.len() as u32;
            for v in &cluster.vertices {
                let mut local_pos = [0u16; 3];
                for c in 0..3 {
                    let global_quant_val =
                        ((v.position[c] - global_min[c]) * quant_factor[c] + 0.5) as u32;
                    local_pos[c] =
                        (global_quant_val.saturating_sub(quant_offset[c])).min(65535) as u16;
                }

                let normal_oct = encode_octahedral_normal(v.normal);
                let uv_half = [f32_to_f16(v.uv[0]), f32_to_f16(v.uv[1])];
                let tangent_oct_sign = encode_octahedral_tangent(
                    [v.tangent[0], v.tangent[1], v.tangent[2]],
                    v.tangent[3],
                );

                all_quantized_vertices.push(QuantizedVertex {
                    position_quant: local_pos,
                    normal_oct,
                    uv_half,
                    tangent_oct_sign,
                    _pad: [0; 2],
                });
            }

            let tri_start = all_triangles.len() as u32;
            let tri_cnt = cluster.triangles.len() as u32;
            all_triangles.extend(cluster.triangles);

            meshlet_descriptors.push(MeshletDescriptor {
                center: cluster.center,
                radius: cluster.radius,
                quant_offset,
                vertex_offset: vert_start,
                triangle_offset: tri_start,
                packed_cone: MeshletDescriptor::pack_cone(cluster.cone_axis, cluster.cone_cutoff),
                vertex_count: cluster.vertices.len() as u32,
                triangle_count: tri_cnt,
                _pad: [0; 4],
            });
        }

        // ====================================================================
        // Phase 5: Serialize Container Binary Stream (.gmesh)
        // ====================================================================
        let header = MeshletContainerHeader {
            magic: MESHLET_MAGIC,
            version: 2,
            meshlet_count: meshlet_descriptors.len() as u32,
            total_vertex_count: all_quantized_vertices.len() as u32,
            total_triangle_count: all_triangles.len() as u32,
            global_min,
            dequant_factor,
            global_max,
            _reserved: [0; 2],
        };

        let mut output = Vec::with_capacity(
            64 + meshlet_descriptors.len() * std::mem::size_of::<MeshletDescriptor>()
                + all_quantized_vertices.len() * std::mem::size_of::<QuantizedVertex>()
                + all_triangles.len() * std::mem::size_of::<MeshletTriangle>(),
        );

        output.extend_from_slice(bytemuck::bytes_of(&header));
        output.extend_from_slice(bytemuck::cast_slice(&meshlet_descriptors));
        output.extend_from_slice(bytemuck::cast_slice(&all_quantized_vertices));
        output.extend_from_slice(bytemuck::cast_slice(&all_triangles));

        Ok(output)
    }
}
