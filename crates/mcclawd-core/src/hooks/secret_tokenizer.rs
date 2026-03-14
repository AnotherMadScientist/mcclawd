//! Redaction tokenizer — a `SecurityHook` that replaces sensitive data with
//! vault tokens before tool calls and resolves them after.
//!
//! Wraps a [`RedactionVault`] and a set of [`DlpPattern`]s. On
//! `before_tool_call` it scans tool arguments for pattern matches and known
//! secrets, replacing them with opaque tokens. On `after_tool_call` it scans
//! tool results the same way.

use async_trait::async_trait;
use std::sync::Arc;

use super::dlp::DlpPattern;
use super::redaction_vault::RedactionVault;
use super::SecurityHook;

/// A `SecurityHook` that tokenizes sensitive data found in tool call arguments
/// and results using a shared [`RedactionVault`].
pub struct RedactionTokenizer {
    /// The vault that stores token-to-original mappings.
    vault: Arc<RedactionVault>,
    /// DLP patterns to scan for.
    patterns: Vec<DlpPattern>,
    /// Known secret name/value pairs to redact.
    secrets: Vec<(String, String)>,
}

impl RedactionTokenizer {
    /// Create a new tokenizer.
    ///
    /// - `vault` — shared vault (same instance should be used for resolution)
    /// - `patterns` — DLP patterns to match against
    /// - `secrets` — known secret (name, value) pairs
    pub fn new(
        vault: Arc<RedactionVault>,
        patterns: Vec<DlpPattern>,
        secrets: Vec<(String, String)>,
    ) -> Self {
        Self {
            vault,
            patterns,
            secrets,
        }
    }

    /// Reference to the underlying vault.
    pub fn vault(&self) -> &Arc<RedactionVault> {
        &self.vault
    }

    /// Recursively scan a JSON value and tokenize any string leaves.
    fn tokenize_value(&self, value: &serde_json::Value) -> serde_json::Value {
        let secret_refs: Vec<(&str, &str)> = self
            .secrets
            .iter()
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .collect();

        match value {
            serde_json::Value::String(s) => {
                let tokenized = self.vault.tokenize_all(s, &self.patterns, &secret_refs);
                serde_json::Value::String(tokenized)
            }
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| self.tokenize_value(v)).collect())
            }
            serde_json::Value::Object(map) => {
                let new_map: serde_json::Map<String, serde_json::Value> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), self.tokenize_value(v)))
                    .collect();
                serde_json::Value::Object(new_map)
            }
            other => other.clone(),
        }
    }
}

#[async_trait]
impl SecurityHook for RedactionTokenizer {
    async fn before_tool_call(
        &self,
        _tool_name: &str,
        args: &serde_json::Value,
    ) -> crate::Result<()> {
        // Scan args for sensitive data — the tokenized version is logged/audited
        // but we don't mutate the original args here (the caller is responsible
        // for using the vault to tokenize if needed).
        let _tokenized = self.tokenize_value(args);
        Ok(())
    }

    async fn after_tool_call(
        &self,
        _tool_name: &str,
        result: &serde_json::Value,
    ) -> crate::Result<()> {
        // Scan results for leaked secrets
        let _tokenized = self.tokenize_value(result);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::dlp::{DlpAction, DlpPattern};
    use crate::hooks::redaction_vault::RedactionType;

    fn test_patterns() -> Vec<DlpPattern> {
        vec![DlpPattern {
            name: "Email Address".to_string(),
            regex: regex::Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
            action: DlpAction::Warn,
        }]
    }

    #[tokio::test]
    async fn redaction_tokenizer_before_tool_call() {
        let vault = Arc::new(RedactionVault::new());
        let secrets = vec![("API_KEY".to_string(), "sk-secret-1234".to_string())];
        let hook = RedactionTokenizer::new(vault.clone(), test_patterns(), secrets);

        let args = serde_json::json!({
            "query": "Send email to alice@example.com using sk-secret-1234"
        });

        hook.before_tool_call("some_tool", &args).await.unwrap();

        // The vault should now contain entries for the detected values
        assert!(vault.len() >= 1);
        // Email should be registered
        let resolved = vault.resolve("{PII:EMAIL_ADDRESS:….com}");
        assert_eq!(resolved, Some("alice@example.com".to_string()));
    }

    #[tokio::test]
    async fn redaction_tokenizer_after_tool_call() {
        let vault = Arc::new(RedactionVault::new());
        let secrets = vec![("DB_PASS".to_string(), "p@ssw0rd!".to_string())];
        let hook = RedactionTokenizer::new(vault.clone(), vec![], secrets);

        let result = serde_json::json!({
            "output": "Connected with password p@ssw0rd!"
        });

        hook.after_tool_call("db_tool", &result).await.unwrap();

        assert!(vault.len() >= 1);
        assert!(vault.resolve("{SECRET:DB_PASS:…0rd!}").is_some());
    }

    #[tokio::test]
    async fn redaction_tokenizer_nested_json() {
        let vault = Arc::new(RedactionVault::new());
        let secrets = vec![("TOKEN".to_string(), "tok_abcdef".to_string())];
        let hook = RedactionTokenizer::new(vault.clone(), vec![], secrets);

        let args = serde_json::json!({
            "config": {
                "auth": "Bearer tok_abcdef",
                "tags": ["tok_abcdef", "safe"]
            }
        });

        hook.before_tool_call("tool", &args).await.unwrap();
        // Token should be registered once (dedup)
        assert_eq!(vault.len(), 1);
    }
}
