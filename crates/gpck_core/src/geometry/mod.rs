// src/geometry/mod.rs
//! # Geometry Subsystem & Micro-Mesh Processing Engine
//!
//! Provides cluster-based meshlet building, 16-bit crack-free quantization,
//! octahedral normal encoding, Displaced Micro-Mesh (DMM) amplification,
//! and official AMD Dense Geometry Format (DGF) 128-byte block compression.

pub mod dgf;
pub mod dmm;
pub mod meshlet;
pub mod octahedral;

pub use dgf::{
    DGF_BLOCK_SIZE, DGF_EXPONENT_BIAS, DGF_HEADER_SIZE, DGF_MAX_TRIS, DGF_MAX_VERTS, DGF_S24_MAX,
    DGF_S24_MIN, DgfBlockHeader, DgfDecoder, DgfEncoder, TriControlValues,
};
pub use dmm::{
    DMM_MAGIC, DmmBuilder, DmmContainerHeader, MAX_DMM_SUBDIV_LEVEL, MicroMeshDescriptor,
    get_micro_triangle_count, get_micro_vertex_count,
};
pub use meshlet::{
    MAX_MESHLET_TRIANGLES, MAX_MESHLET_VERTICES, MESHLET_MAGIC, MeshletBuilder,
    MeshletContainerHeader, MeshletDescriptor, MeshletTriangle, QuantizedVertex, RawVertex,
};
pub use octahedral::{
    decode_octahedral_normal, encode_octahedral_normal, encode_octahedral_tangent, f16_to_f32,
    f32_to_f16,
};
