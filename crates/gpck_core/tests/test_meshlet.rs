// crates/gpck_core/tests/test_meshlet.rs
//! # Comprehensive Meshlet & Geometry Conditioning Integration Tests

use gpck_core::geometry::dmm::{DMM_MAGIC, DmmBuilder, DmmContainerHeader};
use gpck_core::geometry::meshlet::{
    MAX_MESHLET_TRIANGLES, MAX_MESHLET_VERTICES, MESHLET_MAGIC, MeshletBuilder,
    MeshletContainerHeader, MeshletDescriptor, RawVertex,
};
use gpck_core::geometry::octahedral::{decode_octahedral_normal, encode_octahedral_normal};
use gpck_core::packer::PackerOptions;
use gpck_core::packer::geometry::process_geometry_file;
use std::fs;

#[test]
fn test_octahedral_normal_accuracy() {
    let cardinal_normals = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];

    for &n in &cardinal_normals {
        let enc = encode_octahedral_normal(n);
        let dec = decode_octahedral_normal(enc);
        let dot = n[0] * dec[0] + n[1] * dec[1] + n[2] * dec[2];
        assert!(dot > 0.999, "Failed for normal {:?}: got dot = {}", n, dot);
    }
}

#[test]
fn test_crack_free_global_grid_quantization() {
    let shared_vertex = RawVertex {
        position: [5.0, 5.0, 5.0],
        normal: [0.0, 1.0, 0.0],
        uv: [0.5, 0.5],
        tangent: [1.0, 0.0, 0.0, 1.0],
    };

    let v_a0 = RawVertex {
        position: [0.0, 0.0, 0.0],
        ..shared_vertex.clone()
    };
    let v_a1 = RawVertex {
        position: [5.0, 0.0, 0.0],
        ..shared_vertex.clone()
    };

    let v_b0 = RawVertex {
        position: [10.0, 10.0, 10.0],
        ..shared_vertex.clone()
    };
    let v_b1 = RawVertex {
        position: [5.0, 10.0, 10.0],
        ..shared_vertex.clone()
    };

    let vertices = vec![v_a0, v_a1, shared_vertex.clone(), v_b0, v_b1];
    let indices = vec![0, 1, 2, 2, 3, 4];

    let gmesh = MeshletBuilder::build_container(&vertices, &indices).unwrap();
    let header_size = std::mem::size_of::<MeshletContainerHeader>();
    let header: &MeshletContainerHeader = bytemuck::from_bytes(&gmesh[0..header_size]);

    assert_eq!(header.magic, MESHLET_MAGIC);
    assert_eq!(header.meshlet_count, 1);
    assert!(header.global_min[0] <= 0.0);
    assert!(header.global_max[0] >= 10.0);
}

#[test]
fn test_meshlet_clustering_constraints() {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for y in 0..10 {
        for x in 0..10 {
            vertices.push(RawVertex {
                position: [x as f32, y as f32, 0.0],
                normal: [0.0, 0.0, 1.0],
                uv: [x as f32 / 10.0, y as f32 / 10.0],
                tangent: [1.0, 0.0, 0.0, 1.0],
            });
        }
    }

    for y in 0..9 {
        for x in 0..9 {
            let i0 = (y * 10 + x) as u32;
            let i1 = ((y + 1) * 10 + x) as u32;
            let i2 = (y * 10 + x + 1) as u32;
            let i3 = ((y + 1) * 10 + x + 1) as u32;

            indices.extend_from_slice(&[i0, i1, i2, i2, i1, i3]);
        }
    }

    let gmesh = MeshletBuilder::build_container(&vertices, &indices).unwrap();
    let header_size = std::mem::size_of::<MeshletContainerHeader>();
    let header: &MeshletContainerHeader = bytemuck::from_bytes(&gmesh[0..header_size]);

    let desc_size = std::mem::size_of::<MeshletDescriptor>();
    let desc_start = header_size;
    let desc_end = desc_start + header.meshlet_count as usize * desc_size;
    let descriptors: &[MeshletDescriptor] = bytemuck::cast_slice(&gmesh[desc_start..desc_end]);

    for (i, desc) in descriptors.iter().enumerate() {
        assert!(
            desc.vertex_count as usize <= MAX_MESHLET_VERTICES,
            "Meshlet {} exceeded max vertices: {}",
            i,
            desc.vertex_count
        );
        assert!(
            desc.triangle_count as usize <= MAX_MESHLET_TRIANGLES,
            "Meshlet {} exceeded max triangles: {}",
            i,
            desc.triangle_count
        );
        assert!(desc.radius > 0.0, "Bounding sphere radius must be positive");
    }
}

#[test]
fn test_obj_parser_and_packaging_pipeline() {
    let dummy_obj = r#"
v -0.5 -0.5 0.0
v 0.5 -0.5 0.0
v 0.5 0.5 0.0
v -0.5 0.5 0.0
vn 0.0 0.0 1.0
vt 0.0 0.0
vt 1.0 0.0
vt 1.0 1.0
vt 0.0 1.0
f 1/1/1 2/2/1 3/3/1
f 1/1/1 3/3/1 4/4/1
"#;

    let temp_dir = std::env::temp_dir().join("gpck_mesh_test");
    fs::create_dir_all(&temp_dir).unwrap();
    let obj_path = temp_dir.join("quad.obj");
    fs::write(&obj_path, dummy_obj).unwrap();

    let options = PackerOptions::default();
    let processed_files = process_geometry_file(&obj_path, "models/quad.obj", &options).unwrap();

    assert_eq!(processed_files.len(), 1);
    let proc = &processed_files[0];
    assert_eq!(proc.original_path, "models/quad.obj");
    assert_eq!(proc.alignment, 65536);
    assert!(!proc.chunks.is_empty());

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_dmm_procedural_amplification() {
    let v0 = RawVertex {
        position: [0.0, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        uv: [0.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    };
    let v1 = RawVertex {
        position: [1.0, 0.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        uv: [1.0, 0.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    };
    let v2 = RawVertex {
        position: [0.0, 1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        uv: [0.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    };

    let vertices = vec![v0, v1, v2];
    let indices = vec![0, 1, 2];

    let height_sampler = |uv: [f32; 2]| -> f32 { (uv[0] * 10.0).sin() * 0.5 + 0.5 };

    let gdmm_bytes =
        DmmBuilder::build_dmm_container(&vertices, &indices, 3, 0.5, 0.0, height_sampler).unwrap();
    let header: &DmmContainerHeader = bytemuck::from_bytes(&gdmm_bytes[0..64]);

    assert_eq!(header.magic, DMM_MAGIC);
    assert_eq!(header.total_base_triangles, 1);
    assert_eq!(header.total_micro_triangles, 64);
}
