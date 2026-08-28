// crates/gpck_core/src/packer/chunker.rs
//! # Pipeline Stage 3: 64KB Hardware Tile Chunking, Deduplication & Small File Grouping

use crate::compression::codecs::{Codec, CompressionMethod};
use crate::core::asset_id::AssetIdGenerator;
use crate::core::error::{GpckError, GpckResult};
use crate::format::archive::{
    FLAG_ENCRYPTED_META, FLAG_IS_COMPRESSED, FLAG_STREAMING, SHIFT_ALIGNMENT, TYPE_TEXTURE,
};
use crate::format::dds::DdsUtils;
use crate::format::ktx2::Ktx2Utils;
use crate::packer::{PackerOptions, ProcessedChunk, ProcessedFile};
use crossbeam_channel::Sender;
use std::fs;
use std::path::PathBuf;

const GROUP_SUPER_CHUNK_SIZE: usize = 256 * 1024;
const GPU_ALIGNMENT: i64 = 4096;

pub fn compress_to_chunks(
    input: &[u8],
    chunk_size: usize,
    level: i32,
    method: CompressionMethod,
    validate: bool,
    atg_profile: bool,
) -> GpckResult<Vec<ProcessedChunk>> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let actual_chunk_size = if chunk_size == 0 {
        64 * 1024
    } else {
        chunk_size
    };
    let mut chunks = Vec::new();

    for chunk_slice in input.chunks(actual_chunk_size) {
        let hash = twox_hash::XxHash64::oneshot(0, chunk_slice);

        let compressed = match Codec::compress(chunk_slice, method, level, atg_profile) {
            Ok(data) => data,
            Err(_) => chunk_slice.to_vec(),
        };

        let is_uncompressed_fallback =
            compressed.len() >= chunk_slice.len() || compressed == chunk_slice;

        if validate && method != CompressionMethod::Store && !is_uncompressed_fallback {
            match Codec::decompress(&compressed, chunk_slice.len(), method) {
                Ok(decompressed) => {
                    let decomp_hash = twox_hash::XxHash64::oneshot(0, &decompressed);
                    if decomp_hash != hash {
                        // Detailed byte-level difference analysis
                        let mut first_diff_idx = None;
                        let mut diff_count = 0usize;

                        for (i, (&orig, &dec)) in
                            chunk_slice.iter().zip(decompressed.iter()).enumerate()
                        {
                            if orig != dec {
                                if first_diff_idx.is_none() {
                                    first_diff_idx = Some(i);
                                }
                                diff_count += 1;
                            }
                        }

                        let diff_idx = first_diff_idx.unwrap_or(0);
                        let orig_slice =
                            &chunk_slice[diff_idx..(diff_idx + 16).min(chunk_slice.len())];
                        let dec_slice =
                            &decompressed[diff_idx..(diff_idx + 16).min(decompressed.len())];

                        let diag_msg = format!(
                            "\n==================== [BROTLI-G RAW CHUNK MISMATCH] ====================\n\
                             Chunk Hash           : {:016X}\n\
                             Original Size        : {} bytes\n\
                             Compressed Size      : {} bytes (Ratio: {:.1}%)\n\
                             Decompressed Size    : {} bytes\n\
                             Total Mismatched     : {} / {} bytes ({:.2}% corrupted)\n\
                             First Mismatch Offset: Byte {} (0x{:04X})\n\
                             Original Bytes       : {:02X?}\n\
                             Decompressed Bytes   : {:02X?}\n\
                             ========================================================================",
                            hash,
                            chunk_slice.len(),
                            compressed.len(),
                            (compressed.len() as f64 / chunk_slice.len() as f64) * 100.0,
                            decompressed.len(),
                            diff_count,
                            chunk_slice.len(),
                            (diff_count as f64 / chunk_slice.len() as f64) * 100.0,
                            diff_idx,
                            diff_idx,
                            orig_slice,
                            dec_slice
                        );

                        crate::core::logger::log_error(&diag_msg);
                        eprintln!("{}", diag_msg);

                        // Save failure dumps into the centralized log directory for offline analysis
                        let log_dir = crate::core::paths::GpckPaths::get_logs_dir();
                        let _ = std::fs::write(log_dir.join("failed_chunk_orig.bin"), chunk_slice);
                        let _ = std::fs::write(
                            log_dir.join("failed_chunk_compressed.bin"),
                            &compressed,
                        );
                        let _ =
                            std::fs::write(log_dir.join("failed_chunk_decomp.bin"), &decompressed);

                        return Err(GpckError::ChunkValidationFailed {
                            hash,
                            message: format!(
                                "Chunk validation failed: hash mismatch after decompression (diff at 0x{:04X}, {} bytes diff)",
                                diff_idx, diff_count
                            ),
                        });
                    }
                }
                Err(e) => {
                    return Err(GpckError::ChunkValidationFailed {
                        hash,
                        message: format!(
                            "Chunk validation failed: unable to decompress generated chunk: {}",
                            e
                        ),
                    });
                }
            }
        }

        chunks.push(ProcessedChunk {
            compressed_size: compressed.len() as u32,
            original_size: chunk_slice.len() as u32,
            hash,
            data: compressed,
            offset: 0,
        });
    }

    Ok(chunks)
}

pub fn process_small_file_groups(
    small_files: &[(PathBuf, String)],
    tx: &Sender<ProcessedFile>,
    options: &PackerOptions,
) -> GpckResult<()> {
    let mut group_buf = Vec::with_capacity(GROUP_SUPER_CHUNK_SIZE);
    let mut group_items = Vec::new();

    for (abs_path, rel_path) in small_files {
        let bytes = fs::read(abs_path).map_err(GpckError::Io)?;

        // Extract metadata for small textures/LUTs (DDS / KTX2) so resolution and format are preserved
        let mut meta1 = 0u32;
        let mut meta2 = 0u32;
        let mut is_texture = false;

        let lower = rel_path.to_lowercase();
        if lower.ends_with(".dds") {
            if let Some(h_info) = DdsUtils::get_header_info(&bytes) {
                let (dxgi_fmt, _) = DdsUtils::detect_dxgi_format(&bytes);
                meta1 = ((h_info.width as u32) << 16) | ((h_info.height as u32) & 0xFFFF);
                meta2 = ((h_info.mip_count as u32) << 24) | ((dxgi_fmt & 0xFF) << 16);
                is_texture = true;
            }
        } else if lower.ends_with(".ktx2")
            && let Some(k_info) = Ktx2Utils::get_header_info(&bytes)
        {
            meta1 = ((k_info.width as u32) << 16) | ((k_info.height as u32) & 0xFFFF);
            meta2 = ((k_info.mip_count as u32) << 24) | ((k_info.dxgi_format & 0xFF) << 16);
            is_texture = true;
        }

        if group_buf.len() + bytes.len() > GROUP_SUPER_CHUNK_SIZE && !group_buf.is_empty() {
            flush_file_group(&group_buf, &group_items, tx, options)?;
            group_buf.clear();
            group_items.clear();
        }

        group_items.push((rel_path.clone(), bytes.len(), meta1, meta2, is_texture));
        group_buf.extend_from_slice(&bytes);
    }

    if !group_buf.is_empty() {
        flush_file_group(&group_buf, &group_items, tx, options)?;
    }

    Ok(())
}

pub fn flush_file_group(
    group_buf: &[u8],
    group_items: &[(String, usize, u32, u32, bool)],
    tx: &Sender<ProcessedFile>,
    options: &PackerOptions,
) -> GpckResult<()> {
    let chunks = compress_to_chunks(
        group_buf,
        options.chunk_size,
        options.level,
        options.method,
        options.validate_chunks,
        options.atg_profile,
    )?;

    let total_compressed_size: usize = chunks.iter().map(|c| c.compressed_size as usize).sum();
    let total_orig_size: usize = group_items.iter().map(|(_, sz, _, _, _)| *sz).sum();
    let mut curr_sub_offset = 0u32;

    for (rel_path, orig_sz, meta1, meta2, is_texture) in group_items {
        let file_comp_size = if total_orig_size > 0 {
            ((*orig_sz as f64 / total_orig_size as f64) * total_compressed_size as f64).round()
                as u32
        } else {
            *orig_sz as u32
        };

        let mut flags = FLAG_STREAMING;
        if *is_texture {
            flags |= TYPE_TEXTURE;
        }
        if options.method != CompressionMethod::Store && file_comp_size < *orig_sz as u32 {
            flags |= FLAG_IS_COMPRESSED;
        }
        flags |= options.method.to_flag_bits();
        if options.key.is_some() {
            flags |= FLAG_ENCRYPTED_META;
        }

        let alignment = GPU_ALIGNMENT;
        let align_power = (alignment as f64).log2() as u32;
        flags |= align_power << SHIFT_ALIGNMENT;

        let processed = ProcessedFile {
            asset_id: AssetIdGenerator::generate(rel_path),
            original_path: rel_path.clone(),
            original_size: *orig_sz as u32,
            compressed_size: file_comp_size,
            flags,
            tags: options.tags,
            partition_id: 0,
            alignment,
            meta1: *meta1,
            meta2: *meta2,
            chunks: chunks.clone(),
            sub_chunk_offset: curr_sub_offset,
            sub_chunk_size: *orig_sz as u32,
        };

        curr_sub_offset += *orig_sz as u32;
        let _ = tx.send(processed);
    }

    Ok(())
}
