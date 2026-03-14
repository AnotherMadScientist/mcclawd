use aes_gcm_siv::{
    aead::{Aead, KeyInit},
    Aes256GcmSiv, Nonce,
};
use argon2::Argon2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use zeroize::Zeroizing;

use super::{SecretBackend, SecretMeta};
use crate::{McclawdError, Result};

/// A stored secret: value + optional human-readable descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SecretRecord {
    value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    descriptor: Option<String>,
}

/// Legacy fixed salt used by the original format (pre-random-salt).
const LEGACY_FIXED_SALT: &[u8] = b"mcclawd-secrets-v1";

/// Salt length for the new format.
const SALT_LEN: usize = 16;

/// Nonce length for AES-256-GCM-SIV.
const NONCE_LEN: usize = 12;

/// Encrypted file-based secret storage.
/// Secrets are stored as JSON encrypted with AES-256-GCM-SIV.
/// The encryption key is derived from a passphrase via argon2.
///
/// File format (current): `[16 bytes salt][12 bytes nonce][ciphertext]`
/// Legacy format: `[12 bytes nonce][ciphertext]` (uses fixed salt)
///
/// Storage payload v2: `HashMap<String, SecretRecord>` (value + descriptor).
/// Automatically migrates from v1 payload (`HashMap<String, String>`)
/// and from legacy file format (fixed salt).
pub struct EncryptedFileBackend {
    path: PathBuf,
    passphrase: Zeroizing<String>,
    salt: [u8; SALT_LEN],
    key: Zeroizing<[u8; 32]>,
    cache: RwLock<HashMap<String, SecretRecord>>,
}

impl EncryptedFileBackend {
    pub fn new(path: &Path, passphrase: &str) -> Result<Self> {
        let salt: [u8; SALT_LEN] = rand::random();
        let key = derive_key(passphrase, &salt)?;
        let mut backend = Self {
            path: path.to_path_buf(),
            passphrase: Zeroizing::new(passphrase.to_string()),
            salt,
            key: Zeroizing::new(key),
            cache: RwLock::new(HashMap::new()),
        };
        backend.load_from_disk()?;
        Ok(backend)
    }

    /// Create backend without loading from disk (for when vault doesn't exist yet).
    pub fn new_empty(path: &Path, passphrase: &str) -> Result<Self> {
        let salt: [u8; SALT_LEN] = rand::random();
        let key = derive_key(passphrase, &salt)?;
        Ok(Self {
            path: path.to_path_buf(),
            passphrase: Zeroizing::new(passphrase.to_string()),
            salt,
            key: Zeroizing::new(key),
            cache: RwLock::new(HashMap::new()),
        })
    }

    fn load_from_disk(&mut self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }
        let data = std::fs::read(&self.path)
            .map_err(|e| McclawdError::Secret(format!("Failed to read secrets file: {e}")))?;
        if data.len() < NONCE_LEN {
            return Err(McclawdError::Secret("Secrets file too short".into()));
        }

        // Try new format first: [16-byte salt][12-byte nonce][ciphertext]
        let plaintext = if data.len() >= SALT_LEN + NONCE_LEN {
            let (salt_bytes, rest) = data.split_at(SALT_LEN);
            let (nonce_bytes, encrypted) = rest.split_at(NONCE_LEN);

            let mut file_salt = [0u8; SALT_LEN];
            file_salt.copy_from_slice(salt_bytes);
            let key = derive_key(&self.passphrase, &file_salt)?;

            let cipher = Aes256GcmSiv::new_from_slice(&key)
                .map_err(|e| McclawdError::Secret(format!("Cipher init error: {e}")))?;
            let nonce = Nonce::from_slice(nonce_bytes);

            match cipher.decrypt(nonce, encrypted) {
                Ok(pt) => {
                    // New format succeeded — adopt the file's salt and key
                    self.salt = file_salt;
                    self.key = Zeroizing::new(key);
                    pt
                }
                Err(_) => {
                    // New format failed — try legacy format: [12-byte nonce][ciphertext]
                    self.try_legacy_decrypt(&data)?
                }
            }
        } else {
            // File too short for new format but >= 12 bytes — must be legacy
            self.try_legacy_decrypt(&data)?
        };

        // Try v2 payload first (HashMap<String, SecretRecord>),
        // fall back to v1 (HashMap<String, String>) and migrate.
        let map: HashMap<String, SecretRecord> =
            match serde_json::from_slice::<HashMap<String, SecretRecord>>(&plaintext) {
                Ok(v2) => v2,
                Err(_) => {
                    // v1 migration: plain string values → SecretRecord with no descriptor
                    let v1: HashMap<String, String> = serde_json::from_slice(&plaintext)?;
                    v1.into_iter()
                        .map(|(k, v)| {
                            (
                                k,
                                SecretRecord {
                                    value: v,
                                    descriptor: None,
                                },
                            )
                        })
                        .collect()
                }
            };
        *self.cache.get_mut() = map;
        Ok(())
    }

    /// Attempt to decrypt using the legacy fixed salt format: [12-byte nonce][ciphertext].
    fn try_legacy_decrypt(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let legacy_key = derive_key(&self.passphrase, LEGACY_FIXED_SALT)?;
        let (nonce_bytes, encrypted) = data.split_at(NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);
        let cipher = Aes256GcmSiv::new_from_slice(&legacy_key)
            .map_err(|e| McclawdError::Secret(format!("Cipher init error: {e}")))?;
        let plaintext = cipher
            .decrypt(nonce, encrypted)
            .map_err(|e| McclawdError::Secret(format!("Decryption failed: {e}")))?;

        // Legacy format succeeded — generate a fresh random salt for future writes
        let new_salt: [u8; SALT_LEN] = rand::random();
        let new_key = derive_key(&self.passphrase, &new_salt)?;
        self.salt = new_salt;
        self.key = Zeroizing::new(new_key);

        Ok(plaintext)
    }

    async fn save_to_disk(&self) -> Result<()> {
        let cache = self.cache.read().await;
        let plaintext = serde_json::to_vec(&*cache)?;
        let cipher = Aes256GcmSiv::new_from_slice(self.key.as_ref())
            .map_err(|e| McclawdError::Secret(format!("Cipher init error: {e}")))?;
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|e| McclawdError::Secret(format!("Encryption failed: {e}")))?;

        // New format: [16-byte salt][12-byte nonce][ciphertext]
        let mut output = self.salt.to_vec();
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| McclawdError::Secret(format!("Failed to create dir: {e}")))?;
        }

        // Atomic write: temp file + rename prevents corruption if process is killed mid-write
        // (e.g., cargo-watch SIGTERM during save)
        let tmp_path = self.path.with_extension("enc.tmp");
        tokio::fs::write(&tmp_path, &output)
            .await
            .map_err(|e| McclawdError::Secret(format!("Failed to write secrets temp: {e}")))?;
        tokio::fs::rename(&tmp_path, &self.path)
            .await
            .map_err(|e| McclawdError::Secret(format!("Failed to rename secrets: {e}")))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl SecretBackend for EncryptedFileBackend {
    async fn get(&self, key: &str) -> Result<Option<String>> {
        let cache = self.cache.read().await;
        match cache.get(key) {
            Some(record) => {
                tracing::debug!(
                    key = %key,
                    descriptor = record.descriptor.as_deref().unwrap_or(""),
                    "secret.accessed"
                );
                Ok(Some(record.value.clone()))
            }
            None => Ok(None),
        }
    }

    async fn set(&self, key: &str, value: &str) -> Result<()> {
        {
            let mut cache = self.cache.write().await;
            // Preserve existing descriptor when updating just the value
            let existing_descriptor = cache.get(key).and_then(|r| r.descriptor.clone());
            cache.insert(
                key.to_string(),
                SecretRecord {
                    value: value.to_string(),
                    descriptor: existing_descriptor,
                },
            );
        }
        self.save_to_disk().await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        {
            let mut cache = self.cache.write().await;
            cache.remove(key);
        }
        self.save_to_disk().await
    }

    async fn list(&self) -> Result<Vec<String>> {
        let cache = self.cache.read().await;
        Ok(cache.keys().cloned().collect())
    }

    async fn set_with_descriptor(
        &self,
        key: &str,
        value: &str,
        descriptor: Option<&str>,
    ) -> Result<()> {
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                key.to_string(),
                SecretRecord {
                    value: value.to_string(),
                    descriptor: descriptor.map(|d| d.to_string()),
                },
            );
        }
        self.save_to_disk().await
    }

    async fn get_descriptor(&self, key: &str) -> Result<Option<String>> {
        let cache = self.cache.read().await;
        Ok(cache.get(key).and_then(|r| r.descriptor.clone()))
    }

    async fn list_with_metadata(&self) -> Result<Vec<SecretMeta>> {
        let cache = self.cache.read().await;
        Ok(cache
            .iter()
            .map(|(k, r)| SecretMeta {
                key: k.clone(),
                descriptor: r.descriptor.clone(),
            })
            .collect())
    }
}

fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| McclawdError::Secret(format!("Key derivation failed: {e}")))?;
    Ok(key)
}
