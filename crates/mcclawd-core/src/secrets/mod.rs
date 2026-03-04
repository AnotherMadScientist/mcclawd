use async_trait::async_trait;

pub mod encrypted_file;
pub use encrypted_file::EncryptedFileBackend;

/// Trait for secret storage backends.
/// Phase 0: EncryptedFileBackend (AES-256-GCM-SIV + argon2).
/// Future: VaultBackend, KeychainBackend.
#[async_trait]
pub trait SecretBackend: Send + Sync {
    async fn get(&self, key: &str) -> crate::Result<Option<String>>;
    async fn set(&self, key: &str, value: &str) -> crate::Result<()>;
    async fn delete(&self, key: &str) -> crate::Result<()>;
    async fn list(&self) -> crate::Result<Vec<String>>;
}
