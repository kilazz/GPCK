// crates/gpck_core/src/geometry/dmm.rs
//! # Displaced Micro-Meshes (DMM) & Barycentric Domain Subdivision
//!
//! Implements procedural geometry amplification for Mesh Shaders. Stores a coarse base
//! meshlet with micro-displacement values, dynamically generating millions of micro-triangles
//! directly in GPU register memory (VGPR) / Shared Memory (LDS) with zero disk-streaming latency.

use super::meshlet::{MeshletBuilder, RawVertex};
use crate::core::error::{GpckError, GpckResult};
use bytemuck::{Pod, Zeroable};

/// FourCC magic identifier for GPCK Displaced Micro-Mesh containers ("DMM1" = 0x444D4D31).
pub const DMM_MAGIC: u32 = 0x444D4D31;

/// Maximum procedural subdivision level per base triangle (Level 3 = 64 micro-triangles, Level 4 = 256).
pub const MAX_DMM_SUBDIV_LEVEL: u32 = 3;

/// Cache-line aligned 64-byte `.gdmm` container header.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub struct DmmContainerHeader {
    pub magic: u32,                 // "DMM1" (0x444D4D31)
    pub version: u32,               // 1
    pub base_meshlet_count: u32,    // Number of coarse base meshlets
    pub total_base_vertices: u32,   // Coarse base vertices
    pub total_base_triangles: u32,  // Coarse base triangles
    pub total_micro_triangles: u32, // Total procedural micro-triangles generated at max LOD
    pub max_subdiv_level: u32,      // Default target subdiv level (e.g. 3)
    pub global_disp_scale: f32,     // Global displacement height scale
    pub global_disp_bias: f32,      // Global displacement height bias
    pub global_min: [f32; 3],       // Displaced world AABB minimum
    pub global_max: [f32; 3],       // Displaced world AABB maximum
    pub _reserved: [u32; 1],        // Padding to exactly 64 bytes
}

/// 48-byte Micro-Mesh Descriptor for Task/Mesh Shader procedural amplification.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub struct MicroMeshDescriptor {
    pub base_triangle_idx: u32,  // Index of the base coarse triangle
    pub subdiv_level: u8,        // Subdivision level (0..4)
    pub disp_format: u8,         // 0 = unorm8 (1B), 1 = unorm16 (2B), 2 = float16 (2B)
    pub _pad0: [u8; 2],          // Alignment padding
    pub disp_byte_offset: u32,   // Byte offset in micro-displacement payload buffer
    pub micro_vertex_count: u32, // Number of micro-vertices in the barycentric domain
    pub disp_scale: f32,         // Local cluster displacement scale multiplier
    pub disp_bias: f32,          // Local cluster displacement bias
    pub bounds_min: [f32; 3],    // Displaced AABB min (for Frustum Culling in Task Shader)
    pub bounds_max: [f32; 3],    // Displaced AABB max
}

const _: () = assert!(std::mem::size_of::<DmmContainerHeader>() == 64);
const _: () = assert!(std::mem::size_of::<MicroMeshDescriptor>() == 48);

/// Computes the number of micro-vertices in a triangle domain for subdivision level L (S = 2^L).
#[inline(always)]
pub fn get_micro_vertex_count(subdiv_level: u32) -> usize {
    let s = 1 << subdiv_level;
    ((s + 1) * (s + 2)) / 2
}

/// Computes the number of micro-triangles in a triangle domain for subdivision level L (S = 2^L).
#[inline(always)]
pub fn get_micro_triangle_count(subdiv_level: u32) -> usize {
    let s = 1 << subdiv_level;
    s * s
}

/// Maps 2D barycentric grid coordinates (i, j) to a linear 1D micro-vertex index.
#[inline(always)]
pub fn barycentric_to_linear_index(i: usize, j: usize, s: usize) -> usize {
    j + (i * (2 * s + 3 - i)) / 2
}

pub struct DmmBuilder;

impl DmmBuilder {
    /// Builds a `.gdmm` procedural micro-mesh container from a coarse mesh and a displacement map sampler.
    pub fn build_dmm_container<F>(
        base_vertices: &[RawVertex],
        base_indices: &[u32],
        subdiv_level: u32,
        global_disp_scale: f32,
        global_disp_bias: f32,
        height_sampler: F,
    ) -> GpckResult<Vec<u8>>
    where
        F: Fn([f32; 2]) -> f32,
    {
        if base_vertices.is_empty() || base_indices.is_empty() {
            return Err(GpckError::GeometryError(
                "Cannot build DMM container from empty geometry".to_string(),
            ));
        }

        let level = subdiv_level.clamp(0, MAX_DMM_SUBDIV_LEVEL);
        let s = 1 << level;
        let micro_verts_per_tri = get_micro_vertex_count(level);
        let num_base_triangles = base_indices.len() / 3;

        // Build Base Meshlet Container
        let base_gmesh_bytes = MeshletBuilder::build_container(base_vertices, base_indices)?;
        let base_header: &super::meshlet::MeshletContainerHeader =
            bytemuck::from_bytes(&base_gmesh_bytes[0..64]);

        let mut descriptors = Vec::with_capacity(num_base_triangles);
        let mut displacement_payload: Vec<u8> =
            Vec::with_capacity(num_base_triangles * micro_verts_per_tri);

        let mut global_min = [f32::MAX; 3];
        let mut global_max = [f32::MIN; 3];

        // Generate Barycentric Micro-Displacements per Base Triangle
        for tri_idx in 0..num_base_triangles {
            let i0 = base_indices[tri_idx * 3] as usize;
            let i1 = base_indices[tri_idx * 3 + 1] as usize;
            let i2 = base_indices[tri_idx * 3 + 2] as usize;

            let v0 = &base_vertices[i0];
            let v1 = &base_vertices[i1];
            let v2 = &base_vertices[i2];

            let mut tri_min = [f32::MAX; 3];
            let mut tri_max = [f32::MIN; 3];

            let disp_start_offset = displacement_payload.len() as u32;

            for i in 0..=s {
                for j in 0..=(s - i) {
                    let w = (s - i - j) as f32 / s as f32;
                    let u = i as f32 / s as f32;
                    let v = j as f32 / s as f32;

                    let pos = [
                        v0.position[0] * w + v1.position[0] * u + v2.position[0] * v,
                        v0.position[1] * w + v1.position[1] * u + v2.position[1] * v,
                        v0.position[2] * w + v1.position[2] * u + v2.position[2] * v,
                    ];
                    let norm = [
                        v0.normal[0] * w + v1.normal[0] * u + v2.normal[0] * v,
                        v0.normal[1] * w + v1.normal[1] * u + v2.normal[1] * v,
                        v0.normal[2] * w + v1.normal[2] * u + v2.normal[2] * v,
                    ];
                    let uv = [
                        v0.uv[0] * w + v1.uv[0] * u + v2.uv[0] * v,
                        v0.uv[1] * w + v1.uv[1] * u + v2.uv[1] * v,
                    ];

                    let norm_len = (norm[0].powi(2) + norm[1].powi(2) + norm[2].powi(2))
                        .sqrt()
                        .max(1e-6);
                    let unit_norm = [norm[0] / norm_len, norm[1] / norm_len, norm[2] / norm_len];

                    let raw_height = height_sampler(uv).clamp(0.0, 1.0);
                    let displaced_dist = global_disp_bias + raw_height * global_disp_scale;

                    let displaced_pos = [
                        pos[0] + unit_norm[0] * displaced_dist,
                        pos[1] + unit_norm[1] * displaced_dist,
                        pos[2] + unit_norm[2] * displaced_dist,
                    ];

                    for c in 0..3 {
                        tri_min[c] = tri_min[c].min(displaced_pos[c]);
                        tri_max[c] = tri_max[c].max(displaced_pos[c]);
                        global_min[c] = global_min[c].min(displaced_pos[c]);
                        global_max[c] = global_max[c].max(displaced_pos[c]);
                    }

                    let quant_byte = (raw_height * 255.0).round().clamp(0.0, 255.0) as u8;
                    displacement_payload.push(quant_byte);
                }
            }

            descriptors.push(MicroMeshDescriptor {
                base_triangle_idx: tri_idx as u32,
                subdiv_level: level as u8,
                disp_format: 0,
                _pad0: [0; 2],
                disp_byte_offset: disp_start_offset,
                micro_vertex_count: micro_verts_per_tri as u32,
                disp_scale: global_disp_scale,
                disp_bias: global_disp_bias,
                bounds_min: tri_min,
                bounds_max: tri_max,
            });
        }

        // Serialize `.gdmm` binary container
        let header = DmmContainerHeader {
            magic: DMM_MAGIC,
            version: 1,
            base_meshlet_count: base_header.meshlet_count,
            total_base_vertices: base_vertices.len() as u32,
            total_base_triangles: num_base_triangles as u32,
            total_micro_triangles: (num_base_triangles * get_micro_triangle_count(level)) as u32,
            max_subdiv_level: level,
            global_disp_scale,
            global_disp_bias,
            global_min,
            global_max,
            _reserved: [0; 1],
        };

        let desc_bytes = bytemuck::cast_slice(&descriptors);
        let mut output = Vec::with_capacity(
            64 + base_gmesh_bytes.len() + desc_bytes.len() + displacement_payload.len(),
        );

        output.extend_from_slice(bytemuck::bytes_of(&header));
        output.extend_from_slice(&(base_gmesh_bytes.len() as u32).to_le_bytes());
        output.extend_from_slice(&base_gmesh_bytes);
        output.extend_from_slice(desc_bytes);
        output.extend_from_slice(&displacement_payload);

        Ok(output)
    }
}
