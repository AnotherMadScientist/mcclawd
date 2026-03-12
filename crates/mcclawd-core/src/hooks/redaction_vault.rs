//! Per-task vault mapping redaction tokens to original sensitive values.
//!
//! Tokens use the format `{TYPE:LABEL:…SUFFIX}` where:
//! - TYPE: SECRET | PII | DLP
//! - LABEL: what was detected (e.g. CREDIT_CARD, ANTHROPIC_API_KEY)
//! - SUFFIX: last N chars for human identification (never enough to reconstruct)
//!
//! The vault is created per-task, lives in memory, and is dropped when the task ends.
//! Original values are zeroized on drop — no secret residue in memory.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use regex::Regex;
use std::sync::LazyLock;
use zeroize::Zeroize;

/// Category of redacted data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum RedactionType {
    /// Known secret from SecretBackend.
    Secret,
    /// Personally identifiable information.
    Pii,
    /// Generic DLP pattern match.
    Dlp,
}

impl std::fmt::Display for RedactionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Secret => write!(f, "SECRET"),
            Self::Pii => write!(f, "PII"),
            Self::Dlp => write!(f, "DLP"),
        }
    }
}

/// A single redaction entry: maps a token to its original value.
pub struct RedactionEntry {
    /// The raw sensitive value. Zeroized on Drop.
    original: String,
    pub redaction_type: RedactionType,
    pub label: String,
    pub suffix: String,
    pub created_at: DateTime<Utc>,
}

impl Drop for RedactionEntry {
    fn drop(&mut self) {
        self.original.zeroize();
    }
}

impl RedactionEntry {
    /// Expose the original value for resolution at execution boundaries only.
    pub fn original(&self) -> &str {
        &self.original
    }
}

/// Per-task vault mapping `{TYPE:LABEL:…SUFFIX}` tokens to original values.
///
/// Thread-safe via DashMap. Created when a task starts, dropped when it ends.
/// Never serialized. Never persisted. Never enters LLM context.
pub struct RedactionVault {
    entries: DashMap<String, RedactionEntry>,
}

/// Regex to match redaction tokens in text: `{TYPE:LABEL:…SUFFIX}` or `{TYPE:LABEL:…SUFFIX:XX}`
static TOKEN_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{(SECRET|PII|DLP):([A-Z0-9_]+):…([^\}]+)\}").unwrap()
});

impl RedactionVault {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    /// Register a sensitive value and return its redaction token.
    ///
    /// If the same (type, label, value) was already registered, returns the existing token.
    /// If a different value has the same suffix, appends a 2-char hash to disambiguate.
    pub fn register(
        &self,
        redaction_type: RedactionType,
        label: &str,
        original: &str,
    ) -> String {
        // Check if this exact value is already registered.
        for entry in self.entries.iter() {
            let e = entry.value();
            if e.redaction_type == redaction_type
                && e.label == label
                && e.original == original
            {
                return entry.key().clone();
            }
        }

        let suffix = Self::generate_suffix(redaction_type, label, original);

        // Check for collision: same type+label+suffix but different value.
        let base_token = format!("{{{redaction_type}:{label}:…{suffix}}}");
        if let Some(existing) = self.entries.get(&base_token) {
            if existing.original != original {
                // Collision — append 2-char hash from the original value.
                let disambig = Self::disambiguator(original);
                let collision_token =
                    format!("{{{redaction_type}:{label}:…{suffix}:{disambig}}}");
                self.entries.insert(
                    collision_token.clone(),
                    RedactionEntry {
                        original: original.to_string(),
                        redaction_type,
                        label: label.to_string(),
                        suffix: format!("{suffix}:{disambig}"),
                        created_at: Utc::now(),
                    },
                );
                return collision_token;
            }
            // Same value, same token — return existing.
            return base_token;
        }

        self.entries.insert(
            base_token.clone(),
            RedactionEntry {
                original: original.to_string(),
                redaction_type,
                label: label.to_string(),
                suffix: suffix.to_string(),
                created_at: Utc::now(),
            },
        );
        base_token
    }

    /// Resolve a single token back to its original value.
    pub fn resolve(&self, token: &str) -> Option<String> {
        self.entries.get(token).map(|e| e.original.clone())
    }

    /// Replace all `{TYPE:LABEL:…SUFFIX}` tokens in text with their original values.
    pub fn resolve_all(&self, text: &str) -> String {
        TOKEN_PATTERN
            .replace_all(text, |caps: &regex::Captures| {
                let full_token = caps.get(0).unwrap().as_str();
                self.resolve(full_token)
                    .unwrap_or_else(|| full_token.to_string())
            })
            .to_string()
    }

    /// Replace all known sensitive values in text with their redaction tokens.
    ///
    /// This is the inverse of `resolve_all`: given text that may contain raw secret
    /// values, replaces each with its `{TYPE:LABEL:…SUFFIX}` token.
    ///
    /// `known_secrets` is a list of (label, raw_value) pairs from SecretBackend.
    pub fn tokenize_secrets(&self, text: &str, known_secrets: &[(&str, &str)]) -> String {
        let mut result = text.to_string();
        for (label, value) in known_secrets {
            if value.len() < 4 || !result.contains(*value) {
                continue;
            }
            let token = self.register(RedactionType::Secret, label, value);
            result = result.replace(*value, &token);
        }
        result
    }

    /// Number of entries in the vault.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the vault is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over all entries (for audit/debugging).
    pub fn iter(&self) -> impl Iterator<Item = dashmap::mapref::multiple::RefMulti<'_, String, RedactionEntry>> {
        self.entries.iter()
    }

    /// Generate a human-glanceable suffix from the original value.
    ///
    /// Rules:
    /// - Numbers (card, phone, SSN): last 4 digits
    /// - Email: `@domain.tld`
    /// - API keys/tokens: last 4 alphanumeric characters
    /// - Private keys: last 4 hex chars of SHA256 hash
    /// - Generic: last 4 characters
    fn generate_suffix(_redaction_type: RedactionType, label: &str, original: &str) -> String {
        let label_lower = label.to_lowercase();

        // Email: use @domain
        if label_lower.contains("email") {
            if let Some(at_pos) = original.rfind('@') {
                return format!("@{}", &original[at_pos + 1..]);
            }
        }

        // Numeric types: last 4 digits
        if label_lower.contains("credit_card")
            || label_lower.contains("phone")
            || label_lower.contains("ssn")
            || label_lower.contains("iban")
        {
            let digits: String = original.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 4 {
                return digits[digits.len() - 4..].to_string();
            }
        }

        // Private keys: SHA256 last 4 hex
        if label_lower.contains("private_key") || label_lower.contains("pem") {
            use sha2::{Digest, Sha256};
            let hash = Sha256::digest(original.as_bytes());
            return format!("{:02x}{:02x}", hash[30], hash[31]);
        }

        // Default: last 4 alphanumeric characters
        let alnum: String = original.chars().filter(|c| c.is_alphanumeric()).collect();
        if alnum.len() >= 4 {
            alnum[alnum.len() - 4..].to_string()
        } else if original.len() >= 4 {
            original[original.len() - 4..].to_string()
        } else {
            original.to_string()
        }
    }

    /// 2-character disambiguator derived from the value (for suffix collisions).
    fn disambiguator(value: &str) -> String {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(value.as_bytes());
        format!("{:02x}", hash[0])
    }
}

/// Well-known env var name patterns that likely contain secrets.
const SECRET_ENV_PATTERNS: &[&str] = &[
    "_KEY", "_TOKEN", "_SECRET", "_PASSWORD", "_CREDENTIAL",
    "_API_KEY", "_AUTH", "_PASS", "API_KEY", "AUTH_TOKEN",
    "ACCESS_TOKEN", "REFRESH_TOKEN", "CLIENT_SECRET",
    "DATABASE_URL", "REDIS_URL", "MONGODB_URI",
    "PRIVATE_KEY", "SIGNING_KEY", "ENCRYPTION_KEY",
];

impl RedactionVault {
    /// Ingest secrets from environment variables into the vault.
    ///
    /// Scans all env vars whose names match common secret patterns
    /// (e.g. `*_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`).
    /// Returns the number of secrets ingested.
    pub fn ingest_env_vars(&self) -> usize {
        let mut count = 0;
        for (key, value) in std::env::vars() {
            if value.len() < 8 {
                continue; // Skip short values (unlikely secrets)
            }
            let key_upper = key.to_uppercase();
            let is_secret = SECRET_ENV_PATTERNS
                .iter()
                .any(|p| key_upper.contains(p) || key_upper.ends_with(p));
            if is_secret {
                self.register(RedactionType::Secret, &key_upper, &value);
                count += 1;
            }
        }
        count
    }

    /// Ingest secrets from a .env file into the vault.
    ///
    /// Parses KEY=VALUE lines (ignoring comments and blank lines).
    /// Only ingests values whose key names match secret patterns.
    /// Returns the number of secrets ingested.
    pub fn ingest_dotenv(&self, path: &std::path::Path) -> std::io::Result<usize> {
        let content = std::fs::read_to_string(path)?;
        let mut count = 0;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');
                if value.len() < 8 {
                    continue;
                }
                let key_upper = key.to_uppercase();
                let is_secret = SECRET_ENV_PATTERNS
                    .iter()
                    .any(|p| key_upper.contains(p) || key_upper.ends_with(p));
                if is_secret {
                    self.register(RedactionType::Secret, &key_upper, value);
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    /// Ingest all secrets from a SecretBackend into the vault.
    ///
    /// Lists all secret names, fetches their values, and registers them.
    /// Returns the number of secrets ingested.
    pub async fn ingest_secret_backend(
        &self,
        backend: &dyn crate::secrets::SecretBackend,
    ) -> crate::Result<usize> {
        let keys = backend.list().await?;
        let mut count = 0;
        for key in &keys {
            if let Some(value) = backend.get(key).await? {
                if value.len() >= 4 {
                    self.register(RedactionType::Secret, key, &value);
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    /// Ingest an OAuth/A2A/AgentAuth token into the vault.
    ///
    /// OAuth tokens, refresh tokens, and agent-to-agent auth tokens are
    /// registered as SECRET type with a specific label. When the token is
    /// refreshed, call this again with the new value — the suffix will differ,
    /// producing a new token. The old vault entry remains valid until the
    /// vault is dropped (task end).
    ///
    /// The LLM sees `{SECRET:OAUTH_ACCESS_TOKEN:…xxxx}` and the real bearer
    /// token is substituted JIT at the MCP/HTTP call boundary.
    pub fn register_auth_token(&self, label: &str, token_value: &str) -> String {
        self.register(RedactionType::Secret, label, token_value)
    }
}

impl Default for RedactionVault {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_resolve_secret() {
        let vault = RedactionVault::new();
        let token = vault.register(RedactionType::Secret, "API_KEY", "sk-proj-abc123def456");
        assert!(token.starts_with("{SECRET:API_KEY:…"));
        assert!(token.ends_with('}'));
        assert_eq!(vault.resolve(&token), Some("sk-proj-abc123def456".to_string()));
    }

    #[test]
    fn register_credit_card_last_4() {
        let vault = RedactionVault::new();
        let token = vault.register(RedactionType::Pii, "CREDIT_CARD", "4111111111114242");
        assert_eq!(token, "{PII:CREDIT_CARD:…4242}");
        assert_eq!(vault.resolve(&token), Some("4111111111114242".to_string()));
    }

    #[test]
    fn register_phone_last_4() {
        let vault = RedactionVault::new();
        let token = vault.register(RedactionType::Pii, "PHONE", "+1-555-867-5309");
        assert_eq!(token, "{PII:PHONE:…5309}");
    }

    #[test]
    fn register_email_domain() {
        let vault = RedactionVault::new();
        let token = vault.register(RedactionType::Pii, "EMAIL", "alice@example.com");
        assert_eq!(token, "{PII:EMAIL:…@example.com}");
    }

    #[test]
    fn register_ssn_last_4() {
        let vault = RedactionVault::new();
        let token = vault.register(RedactionType::Pii, "SSN", "123-45-6789");
        assert_eq!(token, "{PII:SSN:…6789}");
    }

    #[test]
    fn same_value_returns_same_token() {
        let vault = RedactionVault::new();
        let t1 = vault.register(RedactionType::Secret, "KEY", "sk-abc123");
        let t2 = vault.register(RedactionType::Secret, "KEY", "sk-abc123");
        assert_eq!(t1, t2);
        assert_eq!(vault.len(), 1);
    }

    #[test]
    fn different_value_same_label_different_suffix() {
        let vault = RedactionVault::new();
        let t1 = vault.register(RedactionType::Secret, "API_KEY", "sk-abc123");
        let t2 = vault.register(RedactionType::Secret, "API_KEY", "sk-xyz789");
        assert_ne!(t1, t2);
        assert!(t1.contains("c123"));
        assert!(t2.contains("z789"));
        assert_eq!(vault.len(), 2);
    }

    #[test]
    fn suffix_collision_disambiguates() {
        let vault = RedactionVault::new();
        // Two credit cards that both end in 4242
        let t1 = vault.register(RedactionType::Pii, "CREDIT_CARD", "4111111111114242");
        let t2 = vault.register(RedactionType::Pii, "CREDIT_CARD", "5500000000004242");
        assert_eq!(t1, "{PII:CREDIT_CARD:…4242}");
        // Second one gets disambiguator
        assert!(t2.starts_with("{PII:CREDIT_CARD:…4242:"));
        assert!(t2.ends_with('}'));
        assert_ne!(t1, t2);
    }

    #[test]
    fn resolve_unknown_token_returns_none() {
        let vault = RedactionVault::new();
        assert_eq!(vault.resolve("{SECRET:NOPE:…xxxx}"), None);
    }

    #[test]
    fn resolve_all_replaces_multiple_tokens() {
        let vault = RedactionVault::new();
        let t1 = vault.register(RedactionType::Pii, "CREDIT_CARD", "4111111111114242");
        let t2 = vault.register(RedactionType::Pii, "PHONE", "+1-555-867-5309");

        let text = format!("charge {t1} and call {t2}");
        let resolved = vault.resolve_all(&text);
        assert_eq!(resolved, "charge 4111111111114242 and call +1-555-867-5309");
    }

    #[test]
    fn resolve_all_preserves_unknown_tokens() {
        let vault = RedactionVault::new();
        let text = "unknown {SECRET:NOPE:…xxxx} here";
        let resolved = vault.resolve_all(text);
        assert_eq!(resolved, text);
    }

    #[test]
    fn tokenize_secrets_replaces_known_values() {
        let vault = RedactionVault::new();
        let secrets = vec![
            ("ANTHROPIC_API_KEY", "sk-ant-abc123def456"),
            ("GITHUB_TOKEN", "ghp_xxyyzzaabbccdd"),
        ];
        let text = "my key is sk-ant-abc123def456 and token ghp_xxyyzzaabbccdd";
        let tokenized = vault.tokenize_secrets(text, &secrets);

        assert!(!tokenized.contains("sk-ant-abc123def456"));
        assert!(!tokenized.contains("ghp_xxyyzzaabbccdd"));
        assert!(tokenized.contains("{SECRET:ANTHROPIC_API_KEY:…"));
        assert!(tokenized.contains("{SECRET:GITHUB_TOKEN:…"));
    }

    #[test]
    fn tokenize_then_resolve_roundtrips() {
        let vault = RedactionVault::new();
        let original = "use key sk-ant-abc123def456 now";
        let secrets = vec![("API_KEY", "sk-ant-abc123def456")];
        let tokenized = vault.tokenize_secrets(original, &secrets);
        let resolved = vault.resolve_all(&tokenized);
        assert_eq!(resolved, original);
    }

    #[test]
    fn short_secrets_are_skipped() {
        let vault = RedactionVault::new();
        let secrets = vec![("SHORT", "abc")];
        let text = "value abc here";
        let tokenized = vault.tokenize_secrets(text, &secrets);
        // Too short (< 4 chars) — not tokenized
        assert_eq!(tokenized, text);
    }

    #[test]
    fn zeroize_on_drop() {
        // We can't directly test zeroization of memory, but we verify the Drop impl exists
        // by creating and dropping an entry.
        let vault = RedactionVault::new();
        vault.register(RedactionType::Secret, "KEY", "sensitive-value-here");
        assert_eq!(vault.len(), 1);
        drop(vault);
        // If Drop panicked, we wouldn't reach here.
    }

    #[test]
    fn dlp_type_tokens() {
        let vault = RedactionVault::new();
        let token = vault.register(RedactionType::Dlp, "AWS_ACCESS_KEY", "AKIAIOSFODNN7EXAMPLE");
        assert!(token.starts_with("{DLP:AWS_ACCESS_KEY:…"));
        assert_eq!(vault.resolve(&token), Some("AKIAIOSFODNN7EXAMPLE".to_string()));
    }

    #[test]
    fn ingest_dotenv_file() {
        let dir = tempfile::tempdir().unwrap();
        let dotenv_path = dir.path().join(".env");
        std::fs::write(
            &dotenv_path,
            r#"
# Comment
ANTHROPIC_API_KEY=sk-ant-test1234567890
GITHUB_TOKEN="ghp_abcdefghijklmnop"
NORMAL_VAR=not-a-secret
SHORT_KEY=abc
"#,
        )
        .unwrap();

        let vault = RedactionVault::new();
        let count = vault.ingest_dotenv(&dotenv_path).unwrap();
        assert_eq!(count, 2); // ANTHROPIC_API_KEY and GITHUB_TOKEN
        assert_eq!(vault.len(), 2);
    }

    #[test]
    fn register_auth_token_oauth() {
        let vault = RedactionVault::new();
        let t1 = vault.register_auth_token("OAUTH_ACCESS_TOKEN", "ya29.a0ARrdaM..longtoken..xyz");
        assert!(t1.starts_with("{SECRET:OAUTH_ACCESS_TOKEN:…"));
        assert_eq!(vault.resolve(&t1).unwrap(), "ya29.a0ARrdaM..longtoken..xyz");

        // Refresh: new token value produces different token
        let t2 = vault.register_auth_token("OAUTH_ACCESS_TOKEN", "ya29.a0ARrdaM..newtoken..abc");
        assert_ne!(t1, t2);
        // Both resolvable
        assert!(vault.resolve(&t1).is_some());
        assert!(vault.resolve(&t2).is_some());
    }

    #[test]
    fn register_a2a_agent_token() {
        let vault = RedactionVault::new();
        let token = vault.register_auth_token("A2A_AGENT_TOKEN", "agt_sk_live_abcdef1234567890");
        assert!(token.contains("A2A_AGENT_TOKEN"));
        assert!(vault.resolve(&token).is_some());
    }

    #[test]
    fn private_key_uses_sha256_suffix() {
        let vault = RedactionVault::new();
        let token = vault.register(
            RedactionType::Dlp,
            "PRIVATE_KEY",
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKC...\n-----END RSA PRIVATE KEY-----",
        );
        assert!(token.starts_with("{DLP:PRIVATE_KEY:…"));
        // Suffix should be 4 hex chars from SHA256
        let suffix = token
            .trim_start_matches("{DLP:PRIVATE_KEY:…")
            .trim_end_matches('}');
        assert_eq!(suffix.len(), 4);
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
