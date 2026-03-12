use mcclawd_core::secrets::{self, EncryptedFileBackend, SecretBackend};
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

#[tokio::test]
async fn test_set_with_descriptor() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");
    let backend = EncryptedFileBackend::new(&path, "test-passphrase").unwrap();

    backend
        .set_with_descriptor("DB_URL", "postgres://...", Some("prod billing db"))
        .await
        .unwrap();

    let value = backend.get("DB_URL").await.unwrap();
    assert_eq!(value, Some("postgres://...".to_string()));

    let descriptor = backend.get_descriptor("DB_URL").await.unwrap();
    assert_eq!(descriptor, Some("prod billing db".to_string()));
}

#[tokio::test]
async fn test_set_preserves_descriptor() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");
    let backend = EncryptedFileBackend::new(&path, "test-passphrase").unwrap();

    // Set with descriptor
    backend
        .set_with_descriptor("KEY", "val1", Some("my descriptor"))
        .await
        .unwrap();

    // Update value only via set() — descriptor should be preserved
    backend.set("KEY", "val2").await.unwrap();

    let value = backend.get("KEY").await.unwrap();
    assert_eq!(value, Some("val2".to_string()));

    let descriptor = backend.get_descriptor("KEY").await.unwrap();
    assert_eq!(descriptor, Some("my descriptor".to_string()));
}

#[tokio::test]
async fn test_list_with_metadata() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");
    let backend = EncryptedFileBackend::new(&path, "test-passphrase").unwrap();

    backend.set("PLAIN_KEY", "val").await.unwrap();
    backend
        .set_with_descriptor("DESC_KEY", "val", Some("described"))
        .await
        .unwrap();

    let mut metas = backend.list_with_metadata().await.unwrap();
    metas.sort_by(|a, b| a.key.cmp(&b.key));

    assert_eq!(metas.len(), 2);
    assert_eq!(metas[0].key, "DESC_KEY");
    assert_eq!(metas[0].descriptor, Some("described".to_string()));
    assert_eq!(metas[1].key, "PLAIN_KEY");
    assert_eq!(metas[1].descriptor, None);
}

#[tokio::test]
async fn test_v1_to_v2_migration() {
    use aes_gcm_siv::{aead::{Aead, KeyInit}, Aes256GcmSiv, Nonce};
    use std::collections::HashMap;

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");

    // Write a v1 format file (HashMap<String, String>) manually
    let passphrase = "migration-test";
    let salt = b"mcclawd-secrets-v1";
    let mut key = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .unwrap();

    let mut v1_map: HashMap<String, String> = HashMap::new();
    v1_map.insert("OLD_KEY".to_string(), "old_value".to_string());
    v1_map.insert("OTHER_KEY".to_string(), "other_value".to_string());

    let plaintext = serde_json::to_vec(&v1_map).unwrap();
    let cipher = Aes256GcmSiv::new_from_slice(&key).unwrap();
    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext.as_ref()).unwrap();
    let mut output = nonce_bytes.to_vec();
    output.extend_from_slice(&ciphertext);
    std::fs::write(&path, &output).unwrap();

    // Load with v2 backend — should auto-migrate
    let backend = EncryptedFileBackend::new(&path, passphrase).unwrap();

    let val = backend.get("OLD_KEY").await.unwrap();
    assert_eq!(val, Some("old_value".to_string()));

    // Migrated entries should have no descriptor
    let desc = backend.get_descriptor("OLD_KEY").await.unwrap();
    assert_eq!(desc, None);

    // After adding a descriptor and re-loading, v2 format persists
    backend
        .set_with_descriptor("OLD_KEY", "old_value", Some("migrated"))
        .await
        .unwrap();

    let backend2 = EncryptedFileBackend::new(&path, passphrase).unwrap();
    let desc2 = backend2.get_descriptor("OLD_KEY").await.unwrap();
    assert_eq!(desc2, Some("migrated".to_string()));
}

#[tokio::test]
async fn test_resolve_secret_tokens() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("secrets.enc");
    let backend = EncryptedFileBackend::new(&path, "test-passphrase").unwrap();

    backend.set("API_KEY", "sk-secret-123").await.unwrap();
    backend.set("DB_PASS", "hunter2").await.unwrap();

    let env_vars = vec![
        "PLAIN=hello".to_string(),
        "MY_API_KEY=${API_KEY}".to_string(),
        "DSN=postgres://user:${DB_PASS}@host/db".to_string(),
        "MISSING=${NONEXISTENT}".to_string(),
        "NO_EQUALS_SIGN".to_string(),
    ];

    let resolved = secrets::resolve_secret_tokens(&env_vars, &backend)
        .await
        .unwrap();

    assert_eq!(resolved[0], "PLAIN=hello");
    assert_eq!(resolved[1], "MY_API_KEY=sk-secret-123");
    assert_eq!(resolved[2], "DSN=postgres://user:hunter2@host/db");
    assert_eq!(resolved[3], "MISSING=${NONEXISTENT}"); // unresolved — left as-is
    assert_eq!(resolved[4], "NO_EQUALS_SIGN"); // no '=' — passed through
}
