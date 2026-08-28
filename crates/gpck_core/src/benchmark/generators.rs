// crates/gpck_core/src/benchmark/generators.rs
//! # Procedural Texture & Realistic Game Asset Generators
//!
//! Generates realistic BC1–BC7 block-compressed textures, PBR materials,
//! and mixed-entropy game assets (compiled bytecode, JSON metadata, audio PCM).

pub fn generate_highly_compressible_data(size: usize) -> Vec<u8> {
    let mut data = vec![0u8; size];
    for (k, byte) in data.iter_mut().enumerate() {
        let pattern = (k as f64 * 0.01).sin() * 120.0;
        *byte = (128.0 + pattern) as u8;
    }
    data
}

/// Generates a realistic mixed-entropy game asset payload (40% PBR Textures, 30% Bytecode, 20% Scene JSON, 10% Audio/Normal Maps).
pub fn generate_realistic_game_corpus(size: usize) -> Vec<u8> {
    let mut data = vec![0u8; size];
    let p1 = (size * 40) / 100;
    let p2 = (size * 70) / 100;
    let p3 = (size * 90) / 100;

    // 1. Realistic 4K BC7 PBR texture data (40%)
    let bc7 = generate_realistic_bc7_orm_texture(2048, 2048);
    for (i, byte) in data[..p1].iter_mut().enumerate() {
        *byte = bc7[i % bc7.len()];
    }

    // 2. Structured compiled shader bytecode & index tables (30%)
    for (i, byte) in data[p1..p2].iter_mut().enumerate() {
        let global_i = p1 + i;
        *byte = (((global_i * 101 + 37) ^ (global_i >> 3)) & 0xFF) as u8;
    }

    // 3. Scene graph JSON & actor metadata (20%)
    let json_sample = b"{\"node_id\":4021,\"transform\":[1.0,0.0,0.0,0.0,1.0,0.0],\"mesh_ref\":\"models/hero_character.gmesh\"},";
    for (i, byte) in data[p2..p3].iter_mut().enumerate() {
        *byte = json_sample[i % json_sample.len()];
    }

    // 4. Tangent normal vectors & audio waveforms (10%)
    for (i, byte) in data[p3..size].iter_mut().enumerate() {
        let global_i = p3 + i;
        *byte = ((global_i as f64 * 0.08).sin() * 80.0 + 128.0) as u8;
    }

    data
}

/// BC1 (8 MB): Albedo RGB 565 with 2D spatial gradients + 2-bit micro-indices.
pub fn generate_realistic_bc1_texture(width: usize, height: usize) -> Vec<u8> {
    let num_blocks_x = width / 4;
    let num_blocks_y = height / 4;
    let total_blocks = num_blocks_x * num_blocks_y;
    let mut data = vec![0u8; total_blocks * 8];

    for by in 0..num_blocks_y {
        for bx in 0..num_blocks_x {
            let block_idx = by * num_blocks_x + bx;
            let offset = block_idx * 8;

            let r0 = ((bx as f64 / num_blocks_x as f64) * 31.0) as u16;
            let g0 = ((by as f64 / num_blocks_y as f64) * 63.0) as u16;
            let b0 = (((bx + by) as f64 / (num_blocks_x + num_blocks_y) as f64) * 31.0) as u16;

            let c0 = (r0 << 11) | (g0 << 5) | b0;
            let c1 = c0.wrapping_add(0x0821);

            data[offset..offset + 2].copy_from_slice(&c0.to_le_bytes());
            data[offset + 2..offset + 4].copy_from_slice(&c1.to_le_bytes());

            let pattern_byte = ((bx ^ by) & 0xFF) as u8;
            data[offset + 4] = pattern_byte;
            data[offset + 5] = pattern_byte.wrapping_add(1);
            data[offset + 6] = pattern_byte.wrapping_add(2);
            data[offset + 7] = pattern_byte.wrapping_add(3);
        }
    }

    data
}

/// BC2 (16 MB): Cutout / Decals with 4-bit explicit alpha nibbles + BC1 RGB.
pub fn generate_realistic_bc2_texture(width: usize, height: usize) -> Vec<u8> {
    let num_blocks_x = width / 4;
    let num_blocks_y = height / 4;
    let total_blocks = num_blocks_x * num_blocks_y;
    let mut data = vec![0u8; total_blocks * 16];

    for by in 0..num_blocks_y {
        for bx in 0..num_blocks_x {
            let block_idx = by * num_blocks_x + bx;
            let offset = block_idx * 16;

            for k in 0..8 {
                let alpha_nibble = if (bx / 16 + by / 16 + k) % 3 == 0 {
                    0x00
                } else {
                    0xFF
                };
                data[offset + k] = alpha_nibble;
            }

            let r0 = ((bx as f64 / num_blocks_x as f64) * 31.0) as u16;
            let g0 = ((by as f64 / num_blocks_y as f64) * 63.0) as u16;
            let b0 = (((bx + by) as f64 / (num_blocks_x + num_blocks_y) as f64) * 31.0) as u16;

            let c0 = (r0 << 11) | (g0 << 5) | b0;
            let c1 = c0.wrapping_add(0x0821);

            data[offset + 8..offset + 10].copy_from_slice(&c0.to_le_bytes());
            data[offset + 10..offset + 12].copy_from_slice(&c1.to_le_bytes());

            let pattern_byte = ((bx ^ by) & 0xFF) as u8;
            data[offset + 12] = pattern_byte;
            data[offset + 13] = pattern_byte.wrapping_add(1);
            data[offset + 14] = pattern_byte.wrapping_add(2);
            data[offset + 15] = pattern_byte.wrapping_add(3);
        }
    }

    data
}

/// BC3 (16 MB): Albedo RGB + Smooth 8-bit interpolated alpha channel.
pub fn generate_realistic_bc3_texture(width: usize, height: usize) -> Vec<u8> {
    let num_blocks_x = width / 4;
    let num_blocks_y = height / 4;
    let total_blocks = num_blocks_x * num_blocks_y;
    let mut data = vec![0u8; total_blocks * 16];

    for by in 0..num_blocks_y {
        for bx in 0..num_blocks_x {
            let block_idx = by * num_blocks_x + bx;
            let offset = block_idx * 16;

            let a0 = ((bx as f64 / num_blocks_x as f64) * 255.0) as u8;
            let a1 = a0.wrapping_add(40);
            data[offset] = a0;
            data[offset + 1] = a1;

            for k in 0..6 {
                data[offset + 2 + k] = ((bx ^ by ^ k) & 0xFF) as u8;
            }

            let r0 = ((bx as f64 / num_blocks_x as f64) * 31.0) as u16;
            let g0 = ((by as f64 / num_blocks_y as f64) * 63.0) as u16;
            let b0 = (((bx + by) as f64 / (num_blocks_x + num_blocks_y) as f64) * 31.0) as u16;

            let c0 = (r0 << 11) | (g0 << 5) | b0;
            let c1 = c0.wrapping_add(0x0821);

            data[offset + 8..offset + 10].copy_from_slice(&c0.to_le_bytes());
            data[offset + 10..offset + 12].copy_from_slice(&c1.to_le_bytes());

            let pattern_byte = ((bx ^ by) & 0xFF) as u8;
            data[offset + 12] = pattern_byte;
            data[offset + 13] = pattern_byte.wrapping_add(1);
            data[offset + 14] = pattern_byte.wrapping_add(2);
            data[offset + 15] = pattern_byte.wrapping_add(3);
        }
    }

    data
}

/// BC4 (8 MB): Single-channel 8-bit Grayscale / Height / Roughness map.
pub fn generate_realistic_bc4_texture(width: usize, height: usize) -> Vec<u8> {
    let num_blocks_x = width / 4;
    let num_blocks_y = height / 4;
    let total_blocks = num_blocks_x * num_blocks_y;
    let mut data = vec![0u8; total_blocks * 8];

    for by in 0..num_blocks_y {
        for bx in 0..num_blocks_x {
            let block_idx = by * num_blocks_x + bx;
            let offset = block_idx * 8;

            let r0 = (((bx + by) as f64 / (num_blocks_x + num_blocks_y) as f64) * 255.0) as u8;
            let r1 = r0.wrapping_add(32);
            data[offset] = r0;
            data[offset + 1] = r1;

            for k in 0..6 {
                data[offset + 2 + k] = ((bx ^ by ^ k) & 0xFF) as u8;
            }
        }
    }

    data
}

/// BC5 (16 MB): Dual-channel Tangent-Space Normal Map (Nx in Red, Ny in Green).
pub fn generate_realistic_bc5_texture(width: usize, height: usize) -> Vec<u8> {
    let num_blocks_x = width / 4;
    let num_blocks_y = height / 4;
    let total_blocks = num_blocks_x * num_blocks_y;
    let mut data = vec![0u8; total_blocks * 16];

    for by in 0..num_blocks_y {
        for bx in 0..num_blocks_x {
            let block_idx = by * num_blocks_x + bx;
            let offset = block_idx * 16;

            let nx0 = (128.0 + ((bx as f64 / num_blocks_x as f64) * 60.0 - 30.0)) as u8;
            let nx1 = nx0.wrapping_add(16);
            data[offset] = nx0;
            data[offset + 1] = nx1;
            for k in 0..6 {
                data[offset + 2 + k] = ((bx ^ k) & 0xFF) as u8;
            }

            let ny0 = (128.0 + ((by as f64 / num_blocks_y as f64) * 60.0 - 30.0)) as u8;
            let ny1 = ny0.wrapping_add(16);
            data[offset + 8] = ny0;
            data[offset + 9] = ny1;
            for k in 0..6 {
                data[offset + 10 + k] = ((by ^ k) & 0xFF) as u8;
            }
        }
    }

    data
}

/// BC6H (16 MB): Half-Float HDR Skybox / Radiance Environment Map.
pub fn generate_realistic_bc6h_texture(width: usize, height: usize) -> Vec<u8> {
    let num_blocks_x = width / 4;
    let num_blocks_y = height / 4;
    let total_blocks = num_blocks_x * num_blocks_y;
    let mut data = vec![0u8; total_blocks * 16];

    for by in 0..num_blocks_y {
        for bx in 0..num_blocks_x {
            let block_idx = by * num_blocks_x + bx;
            let offset = block_idx * 16;

            data[offset] = 0x00;
            data[offset + 1] = ((bx * 3) & 0xFF) as u8;
            data[offset + 2] = ((by * 2) & 0xFF) as u8;
            data[offset + 3] = (((bx + by) * 4) & 0xFF) as u8;
            data[offset + 4] = 0x3C;
            data[offset + 5] = 0x40;
            data[offset + 6] = ((bx ^ by) & 0xFF) as u8;
            data[offset + 7] = 0x38;
            data[offset + 8] = ((bx & 0x7F) * 2) as u8;
            data[offset + 9] = ((by & 0x7F) * 2) as u8;

            for k in 0..6 {
                data[offset + 10 + k] = ((bx + by + k) & 0xFF) as u8;
            }
        }
    }

    data
}

/// BC7 (16 MB): Realistic 4K BC7 PBR ORM (Occlusion in R, Roughness in G, Metallic in B).
pub fn generate_realistic_bc7_orm_texture(width: usize, height: usize) -> Vec<u8> {
    let num_blocks_x = width / 4;
    let num_blocks_y = height / 4;
    let total_blocks = num_blocks_x * num_blocks_y;
    let mut data = vec![0u8; total_blocks * 16];

    for by in 0..num_blocks_y {
        for bx in 0..num_blocks_x {
            let block_idx = by * num_blocks_x + bx;
            let offset = block_idx * 16;

            data[offset] = 0x40;

            let ao_val = (255.0 - ((bx % 32) as f64 / 32.0) * 100.0) as u8;
            let rough_val = (((bx ^ by) & 0x7F) * 2) as u8;
            let metal_val = if (bx / 64 + by / 64) % 2 == 0 {
                0u8
            } else {
                255u8
            };

            data[offset + 1] = ao_val;
            data[offset + 2] = rough_val;
            data[offset + 3] = metal_val;
            data[offset + 4] = 255;

            data[offset + 5] = ao_val.wrapping_add(10);
            data[offset + 6] = rough_val.wrapping_add(20);
            data[offset + 7] = metal_val;
            data[offset + 8] = 255;

            for k in 0..7 {
                data[offset + 9 + k] = ((bx + by + k) & 0xFF) as u8;
            }
        }
    }

    data
}

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.2} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
