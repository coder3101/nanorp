//! Encryption for provider API keys at rest (ChaCha20-Poly1305), so a copied
//! database file doesn't leak credentials. The per-installation key lives at
//! `<config dir>/secret.key`; values are stored as `enc:v1:` +
//! base64(nonce || ciphertext).
//!
//! The key sits next to the database, so this protects a leaked DB file
//! (backups, sync folders) — not against full filesystem access.

use std::sync::OnceLock;

use anyhow::{Context, Result};
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{AeadCore, ChaCha20Poly1305, Nonce};

use crate::config;

const PREFIX: &str = "enc:v1:";
const KEY_FILENAME: &str = "secret.key";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

static KEY: OnceLock<[u8; KEY_LEN]> = OnceLock::new();

/// Returns whether a stored value is in the encrypted format.
pub fn is_encrypted(stored: &str) -> bool {
    stored.starts_with(PREFIX)
}

/// Encrypt a secret for storage.
pub fn encrypt(plaintext: &str) -> Result<String> {
    encrypt_with_key(plaintext, key()?)
}

/// Decrypt a stored secret.
pub fn decrypt(stored: &str) -> Result<String> {
    decrypt_with_key(stored, key()?)
}

/// Override the installation key (test-only). Must be called before any
/// encrypt/decrypt in the process; returns whether the override took effect.
#[cfg(test)]
pub fn set_test_key(key_bytes: [u8; KEY_LEN]) -> bool {
    KEY.set(key_bytes).is_ok()
}

fn key() -> Result<&'static [u8; KEY_LEN]> {
    if let Some(k) = KEY.get() {
        return Ok(k);
    }
    let loaded = load_or_create_key()?;
    // A racing initialization would have generated the same file contents.
    Ok(KEY.get_or_init(|| loaded))
}

fn load_or_create_key() -> Result<[u8; KEY_LEN]> {
    let path = config::config_dir()?.join(KEY_FILENAME);

    if path.exists() {
        let bytes = std::fs::read(&path)
            .with_context(|| format!("read key file {}", path.display()))?;
        let arr: [u8; KEY_LEN] = bytes.as_slice().try_into().map_err(|_| {
            anyhow::anyhow!(
                "key file {} is corrupt (expected {KEY_LEN} bytes, found {})",
                path.display(),
                bytes.len()
            )
        })?;
        return Ok(arr);
    }

    config::ensure_dirs()?;
    let generated = ChaCha20Poly1305::generate_key(&mut OsRng);
    std::fs::write(&path, generated.as_slice())
        .with_context(|| format!("write key file {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("restrict permissions on {}", path.display()))?;
    }
    tracing::info!("generated new secret key at {}", path.display());
    Ok(generated.into())
}

fn encrypt_with_key(plaintext: &str, key_bytes: &[u8; KEY_LEN]) -> Result<String> {
    let cipher = ChaCha20Poly1305::new(key_bytes.into());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

    let mut blob = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ciphertext);
    Ok(format!(
        "{PREFIX}{}",
        base64::engine::general_purpose::STANDARD.encode(blob)
    ))
}

fn decrypt_with_key(stored: &str, key_bytes: &[u8; KEY_LEN]) -> Result<String> {
    let b64 = stored
        .strip_prefix(PREFIX)
        .context("value is not in encrypted format")?;
    let blob = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("decode encrypted value")?;
    anyhow::ensure!(blob.len() > NONCE_LEN, "encrypted value too short");

    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new(key_bytes.into());
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption failed (wrong or missing key?): {e}"))?;
    String::from_utf8(plaintext).context("decrypted value is not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: [u8; KEY_LEN] = [42; KEY_LEN];

    #[test]
    fn round_trip() {
        let stored = encrypt_with_key("sk-super-secret", &TEST_KEY).unwrap();
        assert!(is_encrypted(&stored));
        assert!(!stored.contains("sk-super-secret"));
        assert_eq!(decrypt_with_key(&stored, &TEST_KEY).unwrap(), "sk-super-secret");
    }

    #[test]
    fn same_plaintext_produces_different_ciphertexts() {
        let a = encrypt_with_key("secret", &TEST_KEY).unwrap();
        let b = encrypt_with_key("secret", &TEST_KEY).unwrap();
        assert_ne!(a, b, "nonces must be random");
    }

    #[test]
    fn wrong_key_fails() {
        let stored = encrypt_with_key("secret", &TEST_KEY).unwrap();
        let other_key = [7u8; KEY_LEN];
        assert!(decrypt_with_key(&stored, &other_key).is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let stored = encrypt_with_key("secret", &TEST_KEY).unwrap();
        let mut tampered = stored.into_bytes();
        let last = tampered.len() - 1;
        tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
        let tampered = String::from_utf8(tampered).unwrap();
        assert!(decrypt_with_key(&tampered, &TEST_KEY).is_err());
    }

    #[test]
    fn non_encrypted_input_is_rejected() {
        assert!(decrypt_with_key("not-an-encrypted-value", &TEST_KEY).is_err());
    }
}
