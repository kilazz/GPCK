// crates/gpck_core/tests/test_dgf.rs
//! # AMD Dense Geometry Format (DGF) 128-Byte Codec Tests

use gpck_core::geometry::dgf::{DGF_BLOCK_SIZE, DgfDecoder, DgfEncoder, read_bits, write_bits};
use gpck_core::geometry::meshlet::RawVertex;

#[test]
fn test_dgf_bitwise_read_write() {
    let mut buffer = [0u8; 16];
    write_bits(&mut buffer, 5, 11, 0x5A3);
    let val = read_bits(&buffer, 5, 11);
    assert_eq!(val, 0x5A3);
}

#[test]
fn test_dgf_block_encode_decode_roundtrip() {
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
    let v3 = RawVertex {
        position: [1.0, 1.0, 0.0],
        normal: [0.0, 0.0, 1.0],
        uv: [1.0, 1.0],
        tangent: [1.0, 0.0, 0.0, 1.0],
    };

    let vertices = vec![v0, v1, v2, v3];
    let indices = vec![0, 1, 2, 1, 3, 2]; // 2 connected triangles

    let block = DgfEncoder::encode_block(&vertices, &indices, 0, 127).unwrap();
    assert_eq!(block.len(), DGF_BLOCK_SIZE);

    let (decoded_verts, decoded_indices) = DgfDecoder::decode_block(&block).unwrap();

    assert_eq!(decoded_verts.len(), 4);
    assert_eq!(decoded_indices.len(), 6);

    // Verify position accuracy
    for (orig, dec) in vertices.iter().zip(decoded_verts.iter()) {
        for c in 0..3 {
            assert!((orig.position[c] - dec[c]).abs() < 0.01);
        }
    }
}
