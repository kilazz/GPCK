// crates/gpck_core/src/benchmark/meshlet.rs
//! # Part 5: Meshlet Geometry Conditioning & Task/Mesh Shader Telemetry

use crate::compression::codecs::{Codec, CompressionMethod};
use crate::core::error::GpckResult;
use crate::geometry::meshlet::{
    MeshletBuilder, MeshletContainerHeader, MeshletDescriptor, RawVertex,
};
use std::fmt::Write;
use std::time::Instant;

pub fn run_meshlet_suite(out: &mut String) -> GpckResult<()> {
    crate::core::logger::log_info("Profiling Meshlet Geometry Conditioning & Culling...");
    writeln!(
        out,
        "================================================================================"
    )
    .unwrap();
    writeln!(
        out,
        " Part 5: Meshlet Geometry Conditioning & GPU Task/Mesh Shader Telemetry"
    )
    .unwrap();
    writeln!(
        out,
        "================================================================================"
    )
    .unwrap();

    let (vertices, indices) = generate_dense_torus(250, 200, 2.0, 0.7);
    let raw_vertex_bytes = vertices.len() * 60;
    let raw_index_bytes = indices.len() * 4;
    let raw_total_bytes = raw_vertex_bytes + raw_index_bytes;

    writeln!(
        out,
        "[Scenario: High-Density Photogrammetry / Hero Asset Mesh]"
    )
    .unwrap();
    writeln!(
        out,
        "  Raw Input Geometry     : {} vertices, {} triangles",
        vertices.len(),
        indices.len() / 3
    )
    .unwrap();
    writeln!(
        out,
        "  Raw Memory Footprint   : {:.2} MB (Verts: {:.2} MB @ 60B, Indices: {:.2} MB @ 32-bit)",
        raw_total_bytes as f64 / (1024.0 * 1024.0),
        raw_vertex_bytes as f64 / (1024.0 * 1024.0),
        raw_index_bytes as f64 / (1024.0 * 1024.0)
    )
    .unwrap();

    let start_build = Instant::now();
    let gmesh_bytes = MeshletBuilder::build_container(&vertices, &indices)?;
    let build_time_ms = start_build.elapsed().as_secs_f64() * 1000.0;
    let tri_per_sec = (indices.len() / 3) as f64 / (build_time_ms / 1000.0) / 1_000_000.0;

    let header_size = std::mem::size_of::<MeshletContainerHeader>();
    let header: &MeshletContainerHeader = bytemuck::from_bytes(&gmesh_bytes[0..header_size]);

    writeln!(
        out,
        "\n--- GPCK Geometry Conditioning (Crack-Free Global Grid) ---"
    )
    .unwrap();
    writeln!(
        out,
        "  Clustering Performance : {:>6.2} ms ({:.2} Million Triangles/sec)",
        build_time_ms, tri_per_sec
    )
    .unwrap();
    writeln!(
        out,
        "  Generated Clusters     : {} meshlets (Avg {:.1} verts, {:.1} tris/cluster)",
        header.meshlet_count,
        header.total_vertex_count as f64 / header.meshlet_count as f64,
        header.total_triangle_count as f64 / header.meshlet_count as f64
    )
    .unwrap();
    writeln!(
        out,
        "  Quantized .gmesh Size  : {:.2} MB (16B Vertex + 3B Micro-Index + Descriptors)",
        gmesh_bytes.len() as f64 / (1024.0 * 1024.0)
    )
    .unwrap();
    writeln!(
        out,
        "  Uncompressed Memory Win: {:>6.1}x VRAM Reduction ({:.1}% footprint reduction)",
        raw_total_bytes as f64 / gmesh_bytes.len() as f64,
        (1.0 - (gmesh_bytes.len() as f64 / raw_total_bytes as f64)) * 100.0
    )
    .unwrap();

    let comp_zstd = Codec::compress(&gmesh_bytes, CompressionMethod::Zstd, 9, true)?;
    let comp_gdef = Codec::compress(&gmesh_bytes, CompressionMethod::GDeflate, 9, true)
        .unwrap_or_else(|_| comp_zstd.clone());

    writeln!(
        out,
        "\n--- Secondary DirectStorage Compression (.gdat 64KB Tiles) ---"
    )
    .unwrap();
    writeln!(
        out,
        "  Zstandard ATG L9 (Disk): {:.2} MB ({:.1}% ratio)",
        comp_zstd.len() as f64 / (1024.0 * 1024.0),
        (comp_zstd.len() as f64 / raw_total_bytes as f64) * 100.0
    )
    .unwrap();
    writeln!(
        out,
        "  GDeflate Metacommand   : {:.2} MB ({:.1}% ratio)",
        comp_gdef.len() as f64 / (1024.0 * 1024.0),
        (comp_gdef.len() as f64 / raw_total_bytes as f64) * 100.0
    )
    .unwrap();

    let camera_pos = [0.0f32, 5.0, 5.0];
    let desc_size = std::mem::size_of::<MeshletDescriptor>();
    let desc_start = header_size;
    let desc_end = desc_start + header.meshlet_count as usize * desc_size;
    let descriptors: &[MeshletDescriptor] =
        bytemuck::cast_slice(&gmesh_bytes[desc_start..desc_end]);

    let mut backface_culled = 0;
    for d in descriptors {
        let (axis_raw, cutoff_raw) = d.unpack_cone();
        let cone_axis = [
            axis_raw[0] as f32 / 127.0,
            axis_raw[1] as f32 / 127.0,
            axis_raw[2] as f32 / 127.0,
        ];
        let cone_cutoff = cutoff_raw as f32 / 127.0;

        let to_cluster = [
            d.center[0] - camera_pos[0],
            d.center[1] - camera_pos[1],
            d.center[2] - camera_pos[2],
        ];
        let len = (to_cluster[0].powi(2) + to_cluster[1].powi(2) + to_cluster[2].powi(2))
            .sqrt()
            .max(1e-6);
        let view_dir = [
            to_cluster[0] / len,
            to_cluster[1] / len,
            to_cluster[2] / len,
        ];

        let dot =
            view_dir[0] * cone_axis[0] + view_dir[1] * cone_axis[1] + view_dir[2] * cone_axis[2];
        if dot >= cone_cutoff {
            backface_culled += 1;
        }
    }

    let culled_pct = (backface_culled as f64 / header.meshlet_count as f64) * 100.0;
    writeln!(
        out,
        "\n--- Task / Amplification Shader Culling Simulation ---"
    )
    .unwrap();
    writeln!(
        out,
        "  Backface Cone Culling  : {} / {} meshlets rejected ({:.1}% bandwidth saved)",
        backface_culled, header.meshlet_count, culled_pct
    )
    .unwrap();
    writeln!(
        out,
        "  Active Mesh Shaders    : {} workgroups dispatched to GPU rasterizer\n",
        header.meshlet_count - backface_culled
    )
    .unwrap();

    Ok(())
}

fn generate_dense_torus(
    radial_segments: usize,
    tubular_segments: usize,
    radius: f32,
    tube_radius: f32,
) -> (Vec<RawVertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for j in 0..=radial_segments {
        let v = j as f32 / radial_segments as f32;
        let phi = v * std::f32::consts::PI * 2.0;

        for i in 0..=tubular_segments {
            let u = i as f32 / tubular_segments as f32;
            let theta = u * std::f32::consts::PI * 2.0;

            let x = (radius + tube_radius * theta.cos()) * phi.cos();
            let y = (radius + tube_radius * theta.cos()) * phi.sin();
            let z = tube_radius * theta.sin();

            let center_x = radius * phi.cos();
            let center_y = radius * phi.sin();
            let nx = x - center_x;
            let ny = y - center_y;
            let nz = z;
            let n_len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);

            vertices.push(RawVertex {
                position: [x, y, z],
                normal: [nx / n_len, ny / n_len, nz / n_len],
                uv: [u, v],
                tangent: [-phi.sin(), phi.cos(), 0.0, 1.0],
            });
        }
    }

    for j in 0..radial_segments {
        for i in 0..tubular_segments {
            let a = (tubular_segments + 1) * j + i;
            let b = (tubular_segments + 1) * (j + 1) + i;
            let c = (tubular_segments + 1) * (j + 1) + i + 1;
            let d = (tubular_segments + 1) * j + i + 1;

            indices.push(a as u32);
            indices.push(b as u32);
            indices.push(d as u32);

            indices.push(b as u32);
            indices.push(c as u32);
            indices.push(d as u32);
        }
    }

    (vertices, indices)
}
