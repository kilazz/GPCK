// src/format/mod.rs
//! # GPCK Data Formats & Deserialization
//!
//! Defines the binary structures for GPCK `.gtoc` (Table of Contents), `.gdat` (Payloads),
//! zero-copy `MasterTocView`, CHD minimal perfect hashing, DDS, and KTX2.

pub mod archive;
pub mod chd;
pub mod dds;
pub mod ktx2;
pub mod toc_view;
