// crates/gpck_core/src/crypto/aes_gcm.rs
//! # AES-256-GCM Encryption & PBKDF2 Key Derivation
//!
//! Handles key derivation from passphrases and AES-256-GCM encryption/decryption
//! for GPCK metadata and chunk tables.

use crate::core::error::{GpckError, GpckResult};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

/// Static salt used for VFS key derivation (100,000 iterations PBKDF2-HMAC-SHA256).
const STATIC_SALT: &[u8] = b"GPCK_VFS_STATIC_SALT_V1";

/// Derives a 256-bit AES key from a passphrase using PBKDF2-HMAC-SHA256 with 100,000 iterations.
pub fn derive_key(passphrase: &str) -> [u8; 32] {
    let mut derived_key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(
        passphrase.as_bytes(),
        STATIC_SALT,
        100_000,
        &mut derived_key,
    );
    derived_key
}

/// Encrypts a chunk table byte slice using AES-256-GCM.
///
/// Output binary format: `[12-byte Nonce | 16-byte Auth Tag | Ciphertext]`
pub fn encrypt_chunk_table(data: &[u8], key: &[u8; 32]) -> GpckResult<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| {
        GpckError::Crypto(format!("Failed to initialize AES-256-GCM cipher: {}", e))
    })?;

    let mut nonce_bytes = [0u8; 12];
    rand::fill(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);

    let encrypted = cipher
        .encrypt(&nonce, data)
        .map_err(|e| GpckError::Crypto(format!("AES-256-GCM encryption error: {}", e)))?;

    let ciphertext_len = data.len();
    let tag = &encrypted[ciphertext_len..];
    let ciphertext = &encrypted[..ciphertext_len];

    let mut output = Vec::with_capacity(28 + data.len());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(tag);
    output.extend_from_slice(ciphertext);
    Ok(output)
}

/// Decrypts an AES-256-GCM encrypted chunk table byte slice.
///
/// Input binary format: `[12-byte Nonce | 16-byte Auth Tag | Ciphertext]`
pub fn decrypt_chunk_table(encrypted_data: &[u8], key: &[u8; 32]) -> GpckResult<Vec<u8>> {
    if encrypted_data.len() < 28 {
        return Err(GpckError::Crypto(
            "Corrupted encrypted table payload: size is less than 28 bytes".to_string(),
        ));
    }

    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| {
        GpckError::Crypto(format!("Failed to initialize AES-256-GCM cipher: {}", e))
    })?;

    let nonce_bytes: &[u8; 12] = encrypted_data[0..12]
        .try_into()
        .map_err(|_| GpckError::Crypto("Invalid nonce byte slice".to_string()))?;
    let nonce = Nonce::from(*nonce_bytes);

    let tag = &encrypted_data[12..28];
    let ciphertext = &encrypted_data[28..];

    // Recombine into [Ciphertext | Tag] as expected by aes-gcm Aead trait
    let mut payload = Vec::with_capacity(ciphertext.len() + tag.len());
    payload.extend_from_slice(ciphertext);
    payload.extend_from_slice(tag);

    cipher
        .decrypt(&nonce, payload.as_slice())
        .map_err(|_| GpckError::DecryptionFailed)
}
