use aes_gcm_siv::{
    aead::{Aead, KeyInit},
    Aes256GcmSiv, Nonce,
};
use argon2::Argon2;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use zeroize::Zeroizing;

use super::SecretBackend;
use crate::{McclawdError, Result};

/// Encrypted file-based secret storage.
/// Secrets are stored as JSON encrypted with AES-256-GCM-SIV.
/// The encryption key is derived from a passphrase via argon2.
pub struct EncryptedFileBackend {
    path: PathBuf,
    key: Zeroizing<[u8; 32]>,
    cache: RwLock<HashMap<String, String>>,
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
        let map: HashMap<String, String> = serde_json::from_slice(&plaintext)?;
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
        tokio::fs::write(&self.path, &output)
            .await
            .map_err(|e| McclawdError::Secret(format!("Failed to write secrets: {e}")))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl SecretBackend for EncryptedFileBackend {
    async fn get(&self, key: &str) -> Result<Option<String>> {
        let cache = self.cache.read().await;
        Ok(cache.get(key).cloned())
    }

    async fn set(&self, key: &str, value: &str) -> Result<()> {
        {
            let mut cache = self.cache.write().await;
            cache.insert(key.to_string(), value.to_string());
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
}

fn derive_key(passphrase: &str) -> Result<[u8; 32]> {
    let salt = b"mcclawd-secrets-v1"; // Fixed salt — acceptable for local-only use
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| McclawdError::Secret(format!("Key derivation failed: {e}")))?;
    Ok(key)
}
