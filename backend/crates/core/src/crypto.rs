//! AES-256-GCM helpers for encrypting per-user GitHub PATs at rest.
//!
//! Wire format for `encrypt_token` output / `decrypt_token` input:
//! `nonce (12 bytes) || ciphertext || tag (16 bytes)` as produced by the
//! `aes-gcm` crate (tag is appended to the ciphertext).

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;

/// AES-GCM nonce size in bytes.
pub const NONCE_LEN: usize = 12;

/// Minimum ciphertext length: nonce + empty plaintext + 16-byte tag.
const MIN_BLOB_LEN: usize = NONCE_LEN + 16;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum CryptoError {
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed (wrong key, corrupt data, or truncated blob)")]
    Decrypt,
    #[error("ciphertext too short")]
    TooShort,
    #[error("plaintext is not valid UTF-8")]
    InvalidUtf8,
}

/// Encrypt `plaintext` with AES-256-GCM using a random 12-byte nonce.
///
/// Returns `nonce || ciphertext_with_tag`.
pub fn encrypt_token(key: &[u8; 32], plaintext: &str) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::Encrypt)?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| CryptoError::Encrypt)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a blob produced by [`encrypt_token`].
pub fn decrypt_token(key: &[u8; 32], bytes: &[u8]) -> Result<String, CryptoError> {
    if bytes.len() < MIN_BLOB_LEN {
        return Err(CryptoError::TooShort);
    }
    let (nonce_bytes, ciphertext) = bytes.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::Decrypt)?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plain = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CryptoError::Decrypt)?;
    String::from_utf8(plain).map_err(|_| CryptoError::InvalidUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn test_key() -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"test-secret");
        hasher.update(b"ght-github-token-v1");
        hasher.finalize().into()
    }

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let key = test_key();
        let token = "ghp_examplePersonalAccessToken_abc123";
        let blob = encrypt_token(&key, token).unwrap();
        assert!(blob.len() > NONCE_LEN);
        assert_eq!(decrypt_token(&key, &blob).unwrap(), token);
    }

    #[test]
    fn different_nonces_each_encrypt() {
        let key = test_key();
        let a = encrypt_token(&key, "same").unwrap();
        let b = encrypt_token(&key, "same").unwrap();
        assert_ne!(a, b, "random nonce should make ciphertexts differ");
        assert_eq!(decrypt_token(&key, &a).unwrap(), "same");
        assert_eq!(decrypt_token(&key, &b).unwrap(), "same");
    }

    #[test]
    fn wrong_key_fails() {
        let key = test_key();
        let blob = encrypt_token(&key, "secret").unwrap();
        let mut other = key;
        other[0] ^= 0xff;
        assert_eq!(decrypt_token(&other, &blob).unwrap_err(), CryptoError::Decrypt);
    }

    #[test]
    fn truncated_blob_fails() {
        let key = test_key();
        assert_eq!(
            decrypt_token(&key, &[0u8; 8]).unwrap_err(),
            CryptoError::TooShort
        );
    }

    #[test]
    fn empty_plaintext_roundtrip() {
        let key = test_key();
        let blob = encrypt_token(&key, "").unwrap();
        assert_eq!(decrypt_token(&key, &blob).unwrap(), "");
    }
}
