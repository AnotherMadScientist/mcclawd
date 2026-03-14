//! Redaction vault — stores original values behind opaque tokens.
//!
//! When DLP or secret scanning detects sensitive data in tool arguments or
//! results, the value is replaced with a token like `{PII:CREDIT_CARD:…4242}`.
//! The original value is stored in the vault so that authorised callers (e.g.
//! the tool executor) can resolve tokens back to real values when needed.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use regex::Regex;
use zeroize::Zeroize;

use super::dlp::DlpPattern;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The category of redacted data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedactionType {
    /// Personally identifiable information (credit card, SSN, email, …).
    Pii,
    /// Secrets / API keys / tokens.
    Secret,
    /// Prompt injection or other injection content.
    Injection,
}

impl fmt::Display for RedactionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RedactionType::Pii => write!(f, "PII"),
            RedactionType::Secret => write!(f, "SECRET"),
            RedactionType::Injection => write!(f, "INJECTION"),
        }
    }
}

/// A single entry in the vault.
#[derive(Debug, Clone)]
pub struct RedactionEntry {
    pub redaction_type: RedactionType,
    pub label: String,
    pub original: String,
    pub token: String,
}

impl Drop for RedactionEntry {
    fn drop(&mut self) {
        self.original.zeroize();
    }
}

// ---------------------------------------------------------------------------
// Vault
// ---------------------------------------------------------------------------

/// Thread-safe store mapping opaque tokens to their original values.
///
/// Tokens have the format `{TYPE:LABEL:…SUFFIX}` where `SUFFIX` is the last
/// 4 characters of the original value (or the full value if shorter).  When
/// two different values would produce the same token, a numeric disambiguator
/// is appended.
pub struct RedactionVault {
    entries: DashMap<String, RedactionEntry>,
    /// Monotonic counter used to disambiguate colliding tokens.
    counter: AtomicU64,
}

impl fmt::Debug for RedactionVault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedactionVault")
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl Default for RedactionVault {
    fn default() -> Self {
        Self::new()
    }
}

impl RedactionVault {
    /// Create an empty vault.
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
            counter: AtomicU64::new(0),
        }
    }

    /// Number of stored entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the vault is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Register a sensitive value and return an opaque token.
    ///
    /// If the same original value has already been registered under the same
    /// type + label, the existing token is returned.
    pub fn register(
        &self,
        redaction_type: RedactionType,
        label: &str,
        original: &str,
    ) -> String {
        // Check if this exact original is already stored under any token with
        // matching type + label.
        for entry in self.entries.iter() {
            if entry.value().redaction_type == redaction_type
                && entry.value().label == label
                && entry.value().original == original
            {
                return entry.value().token.clone();
            }
        }

        let suffix = Self::suffix(original);
        let base_token = format!("{{{type_}:{label}:…{suffix}}}",
            type_ = redaction_type,
            label = label,
            suffix = suffix,
        );

        // Disambiguate if this token string is already taken for a *different*
        // original value.
        let token = if self.entries.contains_key(&base_token) {
            let n = self.counter.fetch_add(1, Ordering::Relaxed);
            format!("{{{type_}:{label}:…{suffix}#{n}}}",
                type_ = redaction_type,
                label = label,
                suffix = suffix,
                n = n,
            )
        } else {
            base_token
        };

        let entry = RedactionEntry {
            redaction_type,
            label: label.to_string(),
            original: original.to_string(),
            token: token.clone(),
        };
        self.entries.insert(token.clone(), entry);
        token
    }

    /// Resolve a single token back to its original value.
    pub fn resolve(&self, token: &str) -> Option<String> {
        self.entries.get(token).map(|e| e.original.clone())
    }

    /// Replace every token found in `text` with its original value.
    pub fn resolve_all(&self, text: &str) -> String {
        let mut result = text.to_string();
        for entry in self.entries.iter() {
            // Tokens contain regex-special chars like `{`, `}`, `…`, so use
            // plain string replacement instead of regex.
            result = result.replace(&entry.token, &entry.original);
        }
        result
    }

    /// Scan `text` for DLP pattern matches and known secret values, replacing
    /// each match with a vault token.
    ///
    /// `secrets` is a slice of `(name, value)` pairs representing known secret
    /// values that should be redacted even if no DLP pattern matches them.
    pub fn tokenize_all(
        &self,
        text: &str,
        patterns: &[DlpPattern],
        secrets: &[(&str, &str)],
    ) -> String {
        let mut result = text.to_string();

        // 1. Redact known secret values (longest first to avoid partial matches).
        let mut sorted_secrets: Vec<(&str, &str)> = secrets.to_vec();
        sorted_secrets.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        for (name, value) in &sorted_secrets {
            if value.is_empty() || !result.contains(*value) {
                continue;
            }
            let token = self.register(RedactionType::Secret, name, value);
            result = result.replace(*value, &token);
        }

        // 2. Redact DLP pattern matches.
        for pattern in patterns {
            let rtype = Self::redaction_type_for_pattern(&pattern.name);
            let label = Self::label_for_pattern(&pattern.name);

            // Collect matches first to avoid borrow issues with the regex.
            let matches: Vec<String> = pattern
                .regex
                .find_iter(&result)
                .map(|m| m.as_str().to_string())
                .collect();

            for matched in matches {
                let token = self.register(rtype, &label, &matched);
                result = result.replace(&matched, &token);
            }
        }

        result
    }

    // ── helpers ──────────────────────────────────────────────────────────

    /// Last 4 characters of the value (or all of it if shorter).
    fn suffix(value: &str) -> String {
        let chars: Vec<char> = value.chars().collect();
        if chars.len() <= 4 {
            value.to_string()
        } else {
            chars[chars.len() - 4..].iter().collect()
        }
    }

    /// Map a DLP pattern name to a `RedactionType`.
    fn redaction_type_for_pattern(name: &str) -> RedactionType {
        let n = name.to_lowercase();
        if n.contains("injection") || n.contains("traversal") || n.contains("encoding") {
            RedactionType::Injection
        } else if n.contains("key")
            || n.contains("token")
            || n.contains("secret")
            || n.contains("password")
            || n.contains("credential")
            || n.contains("auth")
            || n.contains("bearer")
            || n.contains("jwt")
            || n.contains("session")
            || n.contains("mnemonic")
            || n.contains("private")
        {
            RedactionType::Secret
        } else {
            RedactionType::Pii
        }
    }

    /// Derive a short label from a DLP pattern name.
    fn label_for_pattern(name: &str) -> String {
        name.to_uppercase().replace(' ', "_")
    }
}

// ---------------------------------------------------------------------------
// Regex for locating vault tokens in text (used by resolve_all as fallback).
// ---------------------------------------------------------------------------

fn lazy_static_token_regex() -> &'static Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Matches tokens like {PII:CREDIT_CARD:…4242} or {SECRET:API_KEY:…abcd#3}
        Regex::new(r"\{(?:PII|SECRET|INJECTION):[A-Z0-9_]+:…[^\}]+\}").unwrap()
    })
}

/// Returns true if `text` contains at least one vault token.
pub fn contains_vault_token(text: &str) -> bool {
    lazy_static_token_regex().is_match(text)
}

/// Extract all vault tokens from `text`.
pub fn extract_vault_tokens(text: &str) -> Vec<String> {
    lazy_static_token_regex()
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::dlp::{DlpAction, DlpPattern};

    #[test]
    fn redaction_register_and_resolve() {
        let vault = RedactionVault::new();
        let token = vault.register(RedactionType::Pii, "CREDIT_CARD", "4111111111114242");
        assert_eq!(token, "{PII:CREDIT_CARD:…4242}");
        assert_eq!(vault.resolve(&token), Some("4111111111114242".to_string()));
    }

    #[test]
    fn redaction_short_value_suffix() {
        let vault = RedactionVault::new();
        let token = vault.register(RedactionType::Secret, "PIN", "1234");
        assert_eq!(token, "{SECRET:PIN:…1234}");
    }

    #[test]
    fn redaction_duplicate_returns_same_token() {
        let vault = RedactionVault::new();
        let t1 = vault.register(RedactionType::Pii, "SSN", "123-45-6789");
        let t2 = vault.register(RedactionType::Pii, "SSN", "123-45-6789");
        assert_eq!(t1, t2);
        assert_eq!(vault.len(), 1);
    }

    #[test]
    fn redaction_suffix_collision_disambiguates() {
        let vault = RedactionVault::new();
        let t1 = vault.register(RedactionType::Pii, "CREDIT_CARD", "4111111111114242");
        let t2 = vault.register(RedactionType::Pii, "CREDIT_CARD", "5500000000004242");
        assert_eq!(t1, "{PII:CREDIT_CARD:…4242}");
        assert!(t2.starts_with("{PII:CREDIT_CARD:…4242#"));
        assert_ne!(t1, t2);
        assert_eq!(vault.resolve(&t1), Some("4111111111114242".to_string()));
        assert_eq!(vault.resolve(&t2), Some("5500000000004242".to_string()));
    }

    #[test]
    fn redaction_resolve_all_replaces_tokens() {
        let vault = RedactionVault::new();
        let tok1 = vault.register(RedactionType::Pii, "CREDIT_CARD", "4111111111114242");
        let tok2 = vault.register(RedactionType::Pii, "EMAIL", "user@example.com");

        let redacted = format!("Card {} belongs to {}", tok1, tok2);
        let resolved = vault.resolve_all(&redacted);
        assert_eq!(resolved, "Card 4111111111114242 belongs to user@example.com");
    }

    #[test]
    fn redaction_resolve_missing_returns_none() {
        let vault = RedactionVault::new();
        assert!(vault.resolve("{PII:CREDIT_CARD:…9999}").is_none());
    }

    #[test]
    fn redaction_tokenize_all_with_dlp_pattern() {
        let vault = RedactionVault::new();
        let patterns = vec![DlpPattern {
            name: "Credit Card Number".to_string(),
            regex: regex::Regex::new(r"\b4[0-9]{15}\b").unwrap(),
            action: DlpAction::Redact,
        }];

        let text = "Please charge 4111111111114242 for the order.";
        let result = vault.tokenize_all(text, &patterns, &[]);
        assert!(result.contains("{PII:CREDIT_CARD_NUMBER:…4242}"));
        assert!(!result.contains("4111111111114242"));
        // Can resolve back
        let resolved = vault.resolve_all(&result);
        assert_eq!(resolved, text);
    }

    #[test]
    fn redaction_tokenize_all_with_secrets() {
        let vault = RedactionVault::new();
        let secrets = vec![("ANTHROPIC_API_KEY", "sk-ant-api03-AAAA")];
        let text = "Using key sk-ant-api03-AAAA to call the API.";
        let result = vault.tokenize_all(text, &[], &secrets);
        assert!(result.contains("{SECRET:ANTHROPIC_API_KEY:…AAAA}"));
        assert!(!result.contains("sk-ant-api03-AAAA"));
    }

    #[test]
    fn redaction_tokenize_all_mixed() {
        let vault = RedactionVault::new();
        let patterns = vec![DlpPattern {
            name: "Email Address".to_string(),
            regex: regex::Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
            action: DlpAction::Warn,
        }];
        let secrets = vec![("DB_PASS", "hunter2")];
        let text = "Email alice@example.com, password hunter2";
        let result = vault.tokenize_all(text, &patterns, &secrets);
        assert!(!result.contains("alice@example.com"));
        assert!(!result.contains("hunter2"));
        // Resolve round-trips
        let resolved = vault.resolve_all(&result);
        assert_eq!(resolved, text);
    }

    #[test]
    fn redaction_contains_vault_token() {
        assert!(contains_vault_token("Hello {PII:SSN:…6789} world"));
        assert!(contains_vault_token("{SECRET:API_KEY:…abcd#3}"));
        assert!(!contains_vault_token("No tokens here"));
    }

    #[test]
    fn redaction_extract_vault_tokens() {
        let text = "Card {PII:CC:…4242} email {PII:EMAIL:….com}";
        let tokens = extract_vault_tokens(text);
        assert_eq!(tokens.len(), 2);
        assert!(tokens.contains(&"{PII:CC:…4242}".to_string()));
        assert!(tokens.contains(&"{PII:EMAIL:….com}".to_string()));
    }

    #[test]
    fn redaction_vault_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RedactionVault>();
    }
}
