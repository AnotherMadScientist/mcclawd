use mcclawd_core::secrets::{EncryptedFileBackend, SecretBackend};
use tempfile::TempDir;

#[tokio::test]
async fn test_set_and_get_secret() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");
    let backend = EncryptedFileBackend::new(&path, "test-passphrase").unwrap();

    backend.set("API_KEY", "sk-test-123").await.unwrap();
    let value = backend.get("API_KEY").await.unwrap();
    assert_eq!(value, Some("sk-test-123".to_string()));
}

#[tokio::test]
async fn test_get_missing_secret() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");
    let backend = EncryptedFileBackend::new(&path, "test-passphrase").unwrap();

    let value = backend.get("NONEXISTENT").await.unwrap();
    assert_eq!(value, None);
}

#[tokio::test]
async fn test_list_secrets() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");
    let backend = EncryptedFileBackend::new(&path, "test-passphrase").unwrap();

    backend.set("KEY_A", "val_a").await.unwrap();
    backend.set("KEY_B", "val_b").await.unwrap();

    let keys = backend.list().await.unwrap();
    assert!(keys.contains(&"KEY_A".to_string()));
    assert!(keys.contains(&"KEY_B".to_string()));
}

#[tokio::test]
async fn test_delete_secret() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");
    let backend = EncryptedFileBackend::new(&path, "test-passphrase").unwrap();

    backend.set("KEY", "value").await.unwrap();
    backend.delete("KEY").await.unwrap();
    let value = backend.get("KEY").await.unwrap();
    assert_eq!(value, None);
}

#[tokio::test]
async fn test_persistence_across_instances() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");

    {
        let backend = EncryptedFileBackend::new(&path, "passphrase").unwrap();
        backend.set("PERSIST_KEY", "persist_value").await.unwrap();
    }

    {
        let backend = EncryptedFileBackend::new(&path, "passphrase").unwrap();
        let value = backend.get("PERSIST_KEY").await.unwrap();
        assert_eq!(value, Some("persist_value".to_string()));
    }
}
