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

/// Encrypted file-based secret storage.
/// Secrets are stored as JSON encrypted with AES-256-GCM-SIV.
/// The encryption key is derived from a passphrase via argon2.
///
/// Storage format v2: `HashMap<String, SecretRecord>` (value + descriptor).
/// Automatically migrates from v1 format (`HashMap<String, String>`).
pub struct EncryptedFileBackend {
    path: PathBuf,
    key: Zeroizing<[u8; 32]>,
    cache: RwLock<HashMap<String, SecretRecord>>,
}

impl EncryptedFileBackend {
    pub fn new(path: &Path, passphrase: &str) -> Result<Self> {
        let key = derive_key(passphrase)?;
        let mut backend = Self {
            path: path.to_path_buf(),
            key: Zeroizing::new(key),
            cache: RwLock::new(HashMap::new()),
        };
        backend.load_from_disk()?;
        Ok(backend)
    }

    /// Create backend without loading from disk (for when vault doesn't exist yet).
    pub fn new_empty(path: &Path, passphrase: &str) -> Result<Self> {
        let key = derive_key(passphrase)?;
        Ok(Self {
            path: path.to_path_buf(),
            key: Zeroizing::new(key),
            cache: RwLock::new(HashMap::new()),
        })
    }

    fn load_from_disk(&mut self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }
        let ciphertext = std::fs::read(&self.path)
            .map_err(|e| McclawdError::Secret(format!("Failed to read secrets file: {e}")))?;
        if ciphertext.len() < 12 {
            return Err(McclawdError::Secret("Secrets file too short".into()));
        }
        let (nonce_bytes, encrypted) = ciphertext.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        let cipher = Aes256GcmSiv::new_from_slice(self.key.as_ref())
            .map_err(|e| McclawdError::Secret(format!("Cipher init error: {e}")))?;
        let plaintext = cipher
            .decrypt(nonce, encrypted)
            .map_err(|e| McclawdError::Secret(format!("Decryption failed: {e}")))?;

        // Try v2 format first (HashMap<String, SecretRecord>),
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
        let mut output = nonce_bytes.to_vec();
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

fn derive_key(passphrase: &str) -> Result<[u8; 32]> {
    let salt = b"mcclawd-secrets-v1"; // Fixed salt — acceptable for local-only use
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| McclawdError::Secret(format!("Key derivation failed: {e}")))?;
    Ok(key)
}
