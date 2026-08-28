// crates/gpck_core/tests/test_gacl_transforms.rs

use gpck_core::gacl::{Gacl, GaclTransform};
use gpck_core::graphics::dxgi_format::dxgi;

/// Generates valid, spec-compliant BC1-BC7 block bitfields
fn generate_valid_bcn_block(format: u32, block_idx: usize) -> Vec<u8> {
    let mut block = vec![
        0u8;
        if format == dxgi::BC1_UNORM || format == dxgi::BC4_UNORM {
            8
        } else {
            16
        }
    ];

    match format {
        dxgi::BC1_UNORM => {
            // Endpoints: RGB 5:6:5 (using wrapping arithmetic to avoid debug overflow)
            let ep0 = (0x5A32 ^ (block_idx as u16).wrapping_mul(17)) | 0x0020;
            let ep1 = (0xA2B4 ^ (block_idx as u16).wrapping_mul(31)) | 0x0010;
            block[0..2].copy_from_slice(&ep0.to_le_bytes());
            block[2..4].copy_from_slice(&ep1.to_le_bytes());
            // 16 2-bit pixel indices
            let indices = (0x9E3779B9u32 ^ (block_idx as u32).wrapping_mul(101)).to_le_bytes();
            block[4..8].copy_from_slice(&indices);
        }
        dxgi::BC2_UNORM => {
            // 16 4-bit explicit alpha values
            for i in 0..8 {
                block[i] = ((i.wrapping_mul(31) + block_idx) & 0xFF) as u8;
            }
            // BC1 color payload
            let bc1 = generate_valid_bcn_block(dxgi::BC1_UNORM, block_idx);
            block[8..16].copy_from_slice(&bc1);
        }
        dxgi::BC3_UNORM => {
            // 8-bit interpolated alpha endpoints + 16 3-bit indices (6 bytes)
            block[0] = 240;
            block[1] = 15;
            for i in 2..8 {
                block[i] = ((i.wrapping_mul(47) + block_idx) & 0xFF) as u8;
            }
            // BC1 color payload
            let bc1 = generate_valid_bcn_block(dxgi::BC1_UNORM, block_idx);
            block[8..16].copy_from_slice(&bc1);
        }
        dxgi::BC4_UNORM => {
            // Single channel (Alpha/Height) 8-byte block
            block[0] = 200;
            block[1] = 20;
            for i in 2..8 {
                block[i] = ((i.wrapping_mul(53) + block_idx) & 0xFF) as u8;
            }
        }
        dxgi::BC5_UNORM => {
            // Two independent BC4 blocks (R and G channels)
            let bc4_r = generate_valid_bcn_block(dxgi::BC4_UNORM, block_idx * 2);
            let bc4_g = generate_valid_bcn_block(dxgi::BC4_UNORM, block_idx * 2 + 1);
            block[0..8].copy_from_slice(&bc4_r);
            block[8..16].copy_from_slice(&bc4_g);
        }
        dxgi::BC6H_UF16 => {
            let valid_modes = [
                0x00u8, 0x01, 0x02, 0x06, 0x0A, 0x0E, 0x12, 0x16, 0x1A, 0x1E, 0x03, 0x07, 0x0B,
                0x0F,
            ];
            let mode_header = valid_modes[block_idx % valid_modes.len()];
            for i in 0..16 {
                block[i] = ((i.wrapping_mul(61) + block_idx) & 0xFF) as u8;
            }
            block[0] = (block[0] & 0xE0) | (mode_header & 0x1F);
        }
        dxgi::BC7_UNORM => {
            let mode_bits = [0x01u8, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80];
            let selected_mode = mode_bits[block_idx % mode_bits.len()];
            for i in 0..16 {
                block[i] = ((i.wrapping_mul(73) + block_idx) & 0xFF) as u8;
            }
            block[0] = selected_mode | (block[0] & (selected_mode.wrapping_sub(1)));
        }
        _ => {
            for (i, b) in block.iter_mut().enumerate() {
                *b = (i & 0xFF) as u8;
            }
        }
    }
    block
}

#[test]
fn test_all_gacl_transforms_lossless_roundtrip() {
    let width = 512usize;
    let height = 512usize;

    let transforms = [
        (dxgi::BC1_UNORM, GaclTransform::Bc1Linear, 8),
        (dxgi::BC1_UNORM, GaclTransform::Bc1LinearSpaceCurve, 8),
        (dxgi::BC1_UNORM, GaclTransform::Bc1V2BitInterleaved, 8),
        (dxgi::BC1_UNORM, GaclTransform::Bc1V2SpaceCurve, 8),
        (dxgi::BC2_UNORM, GaclTransform::Bc2AlphaNibble, 16),
        (dxgi::BC3_UNORM, GaclTransform::Bc3Linear, 16),
        (dxgi::BC3_UNORM, GaclTransform::Bc3LinearSpaceCurve, 16),
        (dxgi::BC3_UNORM, GaclTransform::Bc3V2BitInterleaved, 16),
        (dxgi::BC3_UNORM, GaclTransform::Bc3V2SpaceCurve, 16),
        (dxgi::BC4_UNORM, GaclTransform::Bc4Linear, 8),
        (dxgi::BC4_UNORM, GaclTransform::Bc4LinearSpaceCurve, 8),
        (dxgi::BC5_UNORM, GaclTransform::Bc5DualChannel, 16),
        (dxgi::BC5_UNORM, GaclTransform::Bc5SpaceCurve, 16),
        (dxgi::BC6H_UF16, GaclTransform::Bc6hHeaderJoin, 16),
        (dxgi::BC7_UNORM, GaclTransform::Bc7ModeSplit, 16),
        (dxgi::BC7_UNORM, GaclTransform::Bc7ModeJoin, 16),
    ];

    for &(dxgi_fmt, transform, block_size) in &transforms {
        let num_blocks = (width / 4) * (height / 4);
        let raw_size = num_blocks * block_size;

        let mut original = Vec::with_capacity(raw_size);
        for blk_idx in 0..num_blocks {
            let blk = generate_valid_bcn_block(dxgi_fmt, blk_idx);
            original.extend_from_slice(&blk);
        }

        let shuffled =
            Gacl::apply_exact_transform(&original, transform.to_u32(), block_size, width)
                .unwrap_or_else(|e| panic!("Forward transform failed for {:?}: {}", transform, e));

        assert_eq!(shuffled.len(), original.len());

        let restored = Gacl::unshuffle(transform.to_u32(), &shuffled, raw_size, width)
            .unwrap_or_else(|e| panic!("Reverse unshuffle failed for {:?}: {}", transform, e));

        assert_eq!(
            restored.len(),
            original.len(),
            "Size mismatch for {:?}",
            transform
        );
        assert_eq!(
            restored, original,
            "Lossless roundtrip failed for GACL transform {:?}",
            transform
        );
    }
}
