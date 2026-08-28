// crates/gpck_core/src/packer/geometry.rs
//! # Geometry Packaging Pipeline & Wavefront OBJ Ingestion
//!
//! Parses 3D geometry files (`.obj` / `.gmesh`), generates quantized meshlet clusters,
//! and packs them into 64KB tile-aligned archive chunks for GPU DMA streaming.

use crate::compression::codecs::CompressionMethod;
use crate::core::error::{GpckError, GpckResult};
use crate::format::archive::{FLAG_STREAMING, TYPE_MESHLET_CONTAINER};
use crate::geometry::meshlet::{MeshletBuilder, RawVertex};
use crate::packer::chunker;
use crate::packer::texture::{ProcessedFileParams, build_processed_file};
use crate::packer::{PackerOptions, ProcessedFile};
use std::fs;
use std::path::Path;

/// 64 KB D3D12 / Vulkan Sparse Hardware Tile Boundary Alignment
const TILE_HARDWARE_ALIGNMENT: i64 = 65536;

/// Parses a 3D geometry file, runs meshlet clustering, and emits a packaged file record.
pub fn process_geometry_file(
    input_path: &Path,
    rel_path: &str,
    options: &PackerOptions,
) -> GpckResult<Vec<ProcessedFile>> {
    let lower_path = rel_path.to_lowercase();

    let gmesh_payload = if lower_path.ends_with(".obj") {
        let raw_data = fs::read_to_string(input_path)?;
        let (vertices, indices) = parse_obj(&raw_data)?;
        if vertices.is_empty() || indices.is_empty() {
            return Err(GpckError::GeometryError(format!(
                "OBJ file contains no valid geometry: {:?}",
                input_path
            )));
        }
        MeshletBuilder::build_container(&vertices, &indices)?
    } else {
        fs::read(input_path)?
    };

    let method = match options.method {
        CompressionMethod::Auto => CompressionMethod::Zstd,
        m => m,
    };

    let flags = FLAG_STREAMING | TYPE_MESHLET_CONTAINER;

    let chunks = chunker::compress_to_chunks(
        &gmesh_payload,
        options.chunk_size,
        options.level,
        method,
        options.validate_chunks,
        options.atg_profile,
    )?;

    let processed = build_processed_file(ProcessedFileParams {
        rel_path: rel_path.to_string(),
        original_size: gmesh_payload.len() as u32,
        chunks,
        flags,
        tags: options.tags,
        method,
        alignment: TILE_HARDWARE_ALIGNMENT,
        key: options.key.as_ref(),
    });

    Ok(vec![processed])
}

/// Zero-dependency Wavefront OBJ parser supporting Quads & N-Gons via fan triangulation.
fn parse_obj(obj_str: &str) -> GpckResult<(Vec<RawVertex>, Vec<u32>)> {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();

    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for line in obj_str.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("v ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 3 {
                positions.push([
                    parts[0].parse::<f32>().unwrap_or(0.0),
                    parts[1].parse::<f32>().unwrap_or(0.0),
                    parts[2].parse::<f32>().unwrap_or(0.0),
                ]);
            }
        } else if let Some(rest) = trimmed.strip_prefix("vn ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 3 {
                normals.push([
                    parts[0].parse::<f32>().unwrap_or(0.0),
                    parts[1].parse::<f32>().unwrap_or(0.0),
                    parts[2].parse::<f32>().unwrap_or(1.0),
                ]);
            }
        } else if let Some(rest) = trimmed.strip_prefix("vt ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 2 {
                uvs.push([
                    parts[0].parse::<f32>().unwrap_or(0.0),
                    parts[1].parse::<f32>().unwrap_or(0.0),
                ]);
            }
        } else if let Some(rest) = trimmed.strip_prefix("f ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() >= 3 {
                let mut poly_verts = Vec::with_capacity(parts.len());

                for part in parts {
                    let elems: Vec<&str> = part.split('/').collect();
                    let v_idx = elems[0].parse::<usize>().unwrap_or(1).saturating_sub(1);
                    let vt_idx = elems
                        .get(1)
                        .and_then(|s| s.parse::<usize>().ok())
                        .map(|i| i.saturating_sub(1));
                    let vn_idx = elems
                        .get(2)
                        .and_then(|s| s.parse::<usize>().ok())
                        .map(|i| i.saturating_sub(1));

                    let pos = positions.get(v_idx).copied().unwrap_or([0.0, 0.0, 0.0]);
                    let uv = vt_idx
                        .and_then(|i| uvs.get(i).copied())
                        .unwrap_or([0.0, 0.0]);
                    let norm = vn_idx
                        .and_then(|i| normals.get(i).copied())
                        .unwrap_or([0.0, 0.0, 1.0]);

                    let vert_idx = vertices.len() as u32;
                    vertices.push(RawVertex {
                        position: pos,
                        normal: norm,
                        uv,
                        tangent: [1.0, 0.0, 0.0, 1.0],
                    });
                    poly_verts.push(vert_idx);
                }

                // Fan triangulation: (0, 1, 2), (0, 2, 3), (0, 3, 4)...
                for i in 1..(poly_verts.len() - 1) {
                    indices.push(poly_verts[0]);
                    indices.push(poly_verts[i]);
                    indices.push(poly_verts[i + 1]);
                }
            }
        }
    }

    // Compute tangent vectors across triangle faces
    for tri in indices.chunks_exact_mut(3) {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;

        let p0 = vertices[i0].position;
        let p1 = vertices[i1].position;
        let p2 = vertices[i2].position;

        let uv0 = vertices[i0].uv;
        let uv1 = vertices[i1].uv;
        let uv2 = vertices[i2].uv;

        let edge1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let edge2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];

        let delta_uv1 = [uv1[0] - uv0[0], uv1[1] - uv0[1]];
        let delta_uv2 = [uv2[0] - uv0[0], uv2[1] - uv0[1]];

        let det = delta_uv1[0] * delta_uv2[1] - delta_uv2[0] * delta_uv1[1];
        let f = if det.abs() > 1e-6 { 1.0 / det } else { 0.0 };

        let tangent = [
            f * (delta_uv2[1] * edge1[0] - delta_uv1[1] * edge2[0]),
            f * (delta_uv2[1] * edge1[1] - delta_uv1[1] * edge2[1]),
            f * (delta_uv2[1] * edge1[2] - delta_uv1[1] * edge2[2]),
        ];

        let tan_len = (tangent[0].powi(2) + tangent[1].powi(2) + tangent[2].powi(2))
            .sqrt()
            .max(1e-6);
        let normalized_tangent = [
            tangent[0] / tan_len,
            tangent[1] / tan_len,
            tangent[2] / tan_len,
            1.0,
        ];

        vertices[i0].tangent = normalized_tangent;
        vertices[i1].tangent = normalized_tangent;
        vertices[i2].tangent = normalized_tangent;
    }

    Ok((vertices, indices))
}
