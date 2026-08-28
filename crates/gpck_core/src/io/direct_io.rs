// crates/gpck_core/src/io/direct_io.rs
//! # Linux io_uring & O_DIRECT High-Throughput NVMe Subsystem
//!
//! Provides kernel-bypass Direct I/O for Linux and SteamOS. Submits aligned NVMe read
//! requests directly into page-aligned Host memory via io_uring submission queues,
//! completely bypassing the Linux page cache and context-switch syscall latency.

use crate::core::error::{GpckError, GpckResult};
use std::alloc::{Layout, alloc, dealloc};
use std::path::Path;

#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;
#[cfg(target_os = "linux")]
use std::sync::Mutex;

pub const DIRECT_IO_SECTOR_ALIGNMENT: usize = 4096;

pub struct AlignedDirectBuffer {
    ptr: *mut u8,
    layout: Layout,
    capacity: usize,
}

unsafe impl Send for AlignedDirectBuffer {}
unsafe impl Sync for AlignedDirectBuffer {}

impl AlignedDirectBuffer {
    pub fn new(size: usize) -> GpckResult<Self> {
        let aligned_size =
            (size + DIRECT_IO_SECTOR_ALIGNMENT - 1) & !(DIRECT_IO_SECTOR_ALIGNMENT - 1);
        let layout =
            Layout::from_size_align(aligned_size, DIRECT_IO_SECTOR_ALIGNMENT).map_err(|e| {
                GpckError::DirectIoError(format!("Invalid aligned memory layout: {}", e))
            })?;

        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            return Err(GpckError::BufferAllocationFailed(aligned_size));
        }

        Ok(Self {
            ptr,
            layout,
            capacity: aligned_size,
        })
    }

    #[inline(always)]
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }

    #[inline(always)]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr
    }

    #[inline(always)]
    pub fn as_slice(&self, len: usize) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, len.min(self.capacity)) }
    }

    #[inline(always)]
    pub fn as_mut_slice(&mut self, len: usize) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, len.min(self.capacity)) }
    }
}

impl Drop for AlignedDirectBuffer {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                dealloc(self.ptr, self.layout);
            }
        }
    }
}

pub struct LinuxDirectIoReader {
    #[cfg(target_os = "linux")]
    ring: Mutex<io_uring::IoUring>,
    #[cfg(target_os = "linux")]
    direct_file: File,
    is_direct_supported: bool,
}

impl LinuxDirectIoReader {
    pub fn open<P: AsRef<Path>>(path: P) -> GpckResult<Self> {
        #[cfg(target_os = "linux")]
        {
            let direct_file_res = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECT | libc::O_NOATIME | libc::O_CLOEXEC)
                .open(&path);

            let (direct_file, is_direct_supported) = match direct_file_res {
                Ok(f) => (f, true),
                Err(_) => {
                    let standard_file = File::open(&path).map_err(GpckError::Io)?;
                    (standard_file, false)
                }
            };

            let ring = io_uring::IoUring::builder()
                .setup_coop_taskrun()
                .setup_single_issuer()
                .build(256)
                .map_err(|e| {
                    GpckError::DirectIoError(format!(
                        "Failed to initialize Linux io_uring instance: {}",
                        e
                    ))
                })?;

            Ok(Self {
                ring: Mutex::new(ring),
                direct_file,
                is_direct_supported,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = path;
            Ok(Self {
                is_direct_supported: false,
            })
        }
    }

    #[inline(always)]
    pub fn is_direct_io_active(&self) -> bool {
        self.is_direct_supported
    }

    pub fn read_exact_at(&self, offset: u64, size: usize) -> GpckResult<Vec<u8>> {
        #[cfg(target_os = "linux")]
        {
            if !self.is_direct_supported {
                let mut buf = vec![0u8; size];
                use std::os::unix::fs::FileExt;
                self.direct_file
                    .read_exact_at(&mut buf, offset)
                    .map_err(GpckError::Io)?;
                return Ok(buf);
            }

            let aligned_offset =
                (offset / DIRECT_IO_SECTOR_ALIGNMENT as u64) * DIRECT_IO_SECTOR_ALIGNMENT as u64;
            let offset_delta = (offset - aligned_offset) as usize;
            let total_required = offset_delta + size;
            let aligned_size = (total_required + DIRECT_IO_SECTOR_ALIGNMENT - 1)
                & !(DIRECT_IO_SECTOR_ALIGNMENT - 1);

            let mut direct_buf = AlignedDirectBuffer::new(aligned_size)?;
            let fd = io_uring::types::Fd(self.direct_file.as_raw_fd());

            let mut ring = self.ring.lock().unwrap();

            let read_e =
                io_uring::opcode::Read::new(fd, direct_buf.as_mut_ptr(), aligned_size as u32)
                    .offset(aligned_offset)
                    .build()
                    .user_data(0x4750434B);

            unsafe {
                ring.submission().push(&read_e).map_err(|_| {
                    GpckError::DirectIoError("io_uring submission queue is full".to_string())
                })?;
            }

            ring.submit_and_wait(1).map_err(|e| {
                GpckError::DirectIoError(format!("io_uring submit_and_wait failed: {}", e))
            })?;

            let cqe = ring.completion().next().ok_or_else(|| {
                GpckError::DirectIoError("io_uring completion event missing".to_string())
            })?;

            let bytes_read = cqe.result();
            if bytes_read < 0 {
                return Err(GpckError::Io(std::io::Error::from_raw_os_error(
                    -bytes_read,
                )));
            }

            let slice = direct_buf.as_slice(bytes_read as usize);
            if slice.len() < offset_delta + size {
                return Err(GpckError::DirectIoError(
                    "Unexpected EOF during io_uring direct NVMe read".to_string(),
                ));
            }

            Ok(slice[offset_delta..offset_delta + size].to_vec())
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (offset, size);
            Err(GpckError::DirectIoError(
                "Linux io_uring Direct I/O is unsupported on this platform".to_string(),
            ))
        }
    }
}
