//! # Runtime I/O & Virtual File System (VFS)
//!
//! Handles runtime archive streaming (`ArchiveStream`), Virtual File System (`VirtualFileSystem`),
//! Linux `io_uring` kernel-bypass Direct I/O (`direct_io`), and asynchronous resource management.

pub mod direct_io;
pub mod extract;
pub mod resource_manager;
pub mod stream;
pub mod vfs;
