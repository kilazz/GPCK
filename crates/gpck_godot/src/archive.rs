// crates/gpck_godot/src/archive.rs
//! # GPCK Archive Inspection & Integrity Verifier for Godot

use godot::prelude::*;
use gpck_core::compression::codecs::CompressionMethod;
use gpck_core::crypto::aes_gcm::derive_key;
use gpck_core::format::archive::GameArchive;
use gpck_core::gacl::GaclTransform;
use std::sync::Arc;
use uuid::Uuid;

#[derive(GodotClass)]
#[class(init, base=RefCounted)]
pub struct GpckArchive {
    base: Base<RefCounted>,
    inner: Option<Arc<GameArchive>>,
}

#[godot_api]
impl GpckArchive {
    /// Opens a `.gtoc` archive file with an optional AES-256-GCM passphrase.
    #[func]
    pub fn open_archive(&mut self, path: GString, passphrase: GString) -> bool {
        let path_str = path.to_string();
        let pass_str = passphrase.to_string();

        let key = if pass_str.is_empty() {
            None
        } else {
            Some(derive_key(&pass_str))
        };

        match GameArchive::open(&path_str) {
            Ok(mut arch) => {
                arch.decryption_key = key;
                self.inner = Some(Arc::new(arch));
                true
            }
            Err(e) => {
                godot_error!("[GPCK] Failed to open archive: {}", e);
                false
            }
        }
    }

    /// Returns the total count of file entries in the archive's Table of Contents (TOC).
    #[func]
    pub fn get_entry_count(&self) -> i64 {
        if let Some(ref arch) = self.inner {
            arch.get_all_entries().map(|e| e.len() as i64).unwrap_or(0)
        } else {
            0
        }
    }

    /// Returns the total uncompressed size of all assets in bytes.
    #[func]
    pub fn get_total_uncompressed_size(&self) -> i64 {
        self.inner
            .as_ref()
            .map(|a| a.total_uncompressed_size())
            .unwrap_or(0)
    }

    /// Returns an array of dictionaries containing metadata for each file in the Table of Contents (TOC).
    #[func]
    pub fn get_entries(&self) -> VariantArray {
        let mut array = VariantArray::new();
        if let Some(ref arch) = self.inner
            && let Ok(entries) = arch.get_all_entries()
        {
            for e in entries {
                let path = arch
                    .get_path_for_asset(&e)
                    .unwrap_or_else(|| Uuid::from_bytes(e.asset_id).to_string());
                let method = CompressionMethod::from_flags(e.flags);
                let gacl = GaclTransform::from_u32(e.gacl_transform());

                let mut dict = Dictionary::new();
                dict.set("path", GString::from(path));
                dict.set("original_size", e.original_size as i64);
                dict.set("compressed_size", e.compressed_size as i64);
                dict.set(
                    "ratio",
                    if e.original_size > 0 {
                        (e.compressed_size as f64 / e.original_size as f64) * 100.0
                    } else {
                        100.0
                    },
                );
                dict.set("method", GString::from(format!("{:?}", method)));
                dict.set("gacl", GString::from(gacl.display_name()));
                dict.set("partition_id", e.partition_id as i64);

                array.push(&dict.to_variant());
            }
        }
        array
    }

    /// Verifies the decompression integrity of all chunks in the archive.
    #[func]
    pub fn verify_integrity(&self) -> Dictionary {
        let mut dict = Dictionary::new();
        if let Some(ref arch) = self.inner
            && let Ok(entries) = arch.get_all_entries()
        {
            let total = entries.len();
            let mut errors = 0;
            for e in &entries {
                if arch.read_asset(e).is_err() {
                    errors += 1;
                }
            }
            dict.set("total", total as i64);
            dict.set("errors", errors as i64);
            dict.set("ok", errors == 0);
            return dict;
        }
        dict.set("total", 0);
        dict.set("errors", -1);
        dict.set("ok", false);
        dict
    }
}
