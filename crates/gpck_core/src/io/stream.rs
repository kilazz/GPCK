// crates/gpck_core/src/io/stream.rs
//! # Archive Stream Reader
//!
//! Implements standard `Read` and `Seek` traits over chunked, compressed archive payloads.

use crate::compression::codecs::{Codec, CompressionMethod};
use crate::format::archive::{ChunkInfo, GameArchive};
use std::io::{Error, ErrorKind, Read, Result as IoResult, Seek, SeekFrom};
use std::sync::Arc;

pub struct ArchiveStream {
    archive: Arc<GameArchive>,
    chunks: Vec<ChunkInfo>,
    original_size: u64,
    flags: u32,
    position: u64,

    current_chunk_idx: Option<usize>,
    current_chunk_data: Vec<u8>,
}

impl ArchiveStream {
    pub fn new(
        archive: Arc<GameArchive>,
        chunks: Vec<ChunkInfo>,
        original_size: u64,
        flags: u32,
    ) -> Self {
        Self {
            archive,
            chunks,
            original_size,
            flags,
            position: 0,
            current_chunk_idx: None,
            current_chunk_data: Vec::new(),
        }
    }

    fn load_chunk(&mut self, index: usize) -> IoResult<()> {
        if Some(index) == self.current_chunk_idx {
            return Ok(());
        }

        if index >= self.chunks.len() {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                "Requested chunk index out of bounds",
            ));
        }

        let chunk = &self.chunks[index];

        let decompressed_data = if chunk.offset == -1 {
            let chunk_hash = chunk.hash;
            self.archive.resolve_base_chunk(chunk_hash).ok_or_else(|| {
                Error::new(
                    ErrorKind::NotFound,
                    format!("Base chunk {:016X} not found for delta patch", chunk_hash),
                )
            })?
        } else {
            let method = CompressionMethod::from_flags(self.flags);
            let raw_chunk = self
                .archive
                .read_raw_chunk(chunk)
                .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;

            if chunk.compressed_size == chunk.original_size
                || method == CompressionMethod::Store
                || method == CompressionMethod::Auto
            {
                raw_chunk
            } else {
                Codec::decompress(&raw_chunk, chunk.original_size as usize, method)
                    .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?
            }
        };

        self.current_chunk_data = decompressed_data;
        self.current_chunk_idx = Some(index);
        Ok(())
    }

    fn get_chunk_for_pos(&self, pos: u64) -> (usize, u64) {
        let mut accumulated_bytes = 0u64;
        for (idx, chunk) in self.chunks.iter().enumerate() {
            let chunk_orig_size = chunk.original_size as u64;
            if pos < accumulated_bytes + chunk_orig_size {
                return (idx, pos - accumulated_bytes);
            }
            accumulated_bytes += chunk_orig_size;
        }

        if self.chunks.is_empty() {
            (0, 0)
        } else {
            (self.chunks.len() - 1, 0)
        }
    }
}

impl Read for ArchiveStream {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        if self.position >= self.original_size || buf.is_empty() {
            return Ok(0);
        }

        let mut total_bytes_read = 0;
        let target_read_count = (buf.len() as u64).min(self.original_size - self.position) as usize;

        while total_bytes_read < target_read_count {
            let (chunk_idx, offset_in_chunk) = self.get_chunk_for_pos(self.position);
            self.load_chunk(chunk_idx)?;

            let chunk_orig_size = self.chunks[chunk_idx].original_size as usize;
            let available_in_chunk = chunk_orig_size.saturating_sub(offset_in_chunk as usize);
            if available_in_chunk == 0 {
                break;
            }

            let copy_count = (target_read_count - total_bytes_read).min(available_in_chunk);
            let src_start = offset_in_chunk as usize;
            let src_end = src_start + copy_count;

            buf[total_bytes_read..total_bytes_read + copy_count]
                .copy_from_slice(&self.current_chunk_data[src_start..src_end]);

            self.position += copy_count as u64;
            total_bytes_read += copy_count;
        }

        Ok(total_bytes_read)
    }
}

impl Seek for ArchiveStream {
    fn seek(&mut self, pos: SeekFrom) -> IoResult<u64> {
        let new_position = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(offset) => {
                self.position.checked_add_signed(offset).ok_or_else(|| {
                    Error::new(
                        ErrorKind::InvalidInput,
                        "Seek position calculation overflow",
                    )
                })?
            }
            SeekFrom::End(offset) => {
                self.original_size
                    .checked_add_signed(offset)
                    .ok_or_else(|| {
                        Error::new(
                            ErrorKind::InvalidInput,
                            "Seek position calculation overflow",
                        )
                    })?
            }
        };

        self.position = new_position.min(self.original_size);
        Ok(self.position)
    }
}
