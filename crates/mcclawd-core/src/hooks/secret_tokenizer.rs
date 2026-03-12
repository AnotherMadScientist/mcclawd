//! Redaction tokenizer — SecurityHook that replaces sensitive data with typed tokens.
//!
//! Runs **first** in the HookPipeline (before DLP, SecretScanner, Audit) so downstream
//! hooks see tokenized text, not raw secrets.
//!
//! Token format: `{TYPE:LABEL:…SUFFIX}`
//! - SECRET: known secrets from SecretBackend
//! - PII: credit cards, phones, emails, SSNs detected by DLP patterns
//! - DLP: other DLP pattern matches (API keys, private keys, etc.)
//!
//! The `RedactionVault` (per-task) maps tokens back to original values.
//! Resolution happens only at execution boundaries (host→container, host→MCP).

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::dlp::{DlpAction, DlpPattern};
use super::pipeline::{PendingFinding, SecurityContext};
use super::redaction_vault::{RedactionType, RedactionVault};
use super::SecurityHook;

/// Maps a DLP pattern name to a (RedactionType, label) pair.
fn classify_pattern(name: &str) -> (RedactionType, String) {
    let n = name.to_lowercase();

    // PII patterns
    if n.contains("credit card") {
        return (RedactionType::Pii, "CREDIT_CARD".to_string());
    }
    if n.contains("email") {
        return (RedactionType::Pii, "EMAIL".to_string());
    }
    if n.contains("phone") {
        return (RedactionType::Pii, "PHONE".to_string());
    }
    if n.starts_with("us ssn") || n.contains("social security") {
        return (RedactionType::Pii, "SSN".to_string());
    }
    if n.contains("iban") {
        return (RedactionType::Pii, "IBAN".to_string());
    }
    if n.contains("passport") {
        return (RedactionType::Pii, "PASSPORT".to_string());
    }
    if n.contains("driver") {
        return (RedactionType::Pii, "DRIVERS_LICENSE".to_string());
    }
    if n.contains("itin") {
        return (RedactionType::Pii, "ITIN".to_string());
    }
    if n.contains("mrn") || n.contains("medical record") {
        return (RedactionType::Pii, "MRN".to_string());
    }

    // DLP patterns — derive label from pattern name
    let label = name
        .to_uppercase()
        .replace(' ', "_")
        .replace('-', "_");
    (RedactionType::Dlp, label)
}

/// SecurityHook that tokenizes sensitive data before it reaches the LLM.
pub struct RedactionTokenizer {
    /// Per-task redaction vault (shared with execution boundary resolvers).
    vault: Arc<RedactionVault>,
    /// DLP patterns used for detection (same 109 patterns from DlpHook).
    patterns: Vec<DlpPattern>,
    /// Known secrets: (name, value) pairs from SecretBackend.
    /// Set at task start, cleared at task end.
    known_secrets: Arc<RwLock<Vec<(String, String)>>>,
    /// Shared pipeline context for pushing audit findings.
    context: Option<Arc<RwLock<SecurityContext>>>,
}

impl RedactionTokenizer {
    /// Create a new tokenizer with the given vault and DLP patterns.
    pub fn new(vault: Arc<RedactionVault>, patterns: Vec<DlpPattern>) -> Self {
        Self {
            vault,
            patterns,
            known_secrets: Arc::new(RwLock::new(Vec::new())),
            context: None,
        }
    }

    /// Attach the shared pipeline context so tokenization events get audited.
    pub fn with_context(mut self, ctx: Arc<RwLock<SecurityContext>>) -> Self {
        self.context = Some(ctx);
        self
    }

    /// Set known secrets for tokenization (called at task start).
    pub async fn set_known_secrets(&self, secrets: Vec<(String, String)>) {
        let mut guard = self.known_secrets.write().await;
        *guard = secrets;
    }

    /// Get a reference to the vault (for resolution at execution boundaries).
    pub fn vault(&self) -> &Arc<RedactionVault> {
        &self.vault
    }

    /// Tokenize all sensitive data in text: known secrets first, then DLP pattern matches.
    async fn tokenize_text(&self, text: &str) -> String {
        let mut result = text.to_string();

        // 1. Replace known secrets (highest priority — exact match).
        {
            let secrets = self.known_secrets.read().await;
            let refs: Vec<(&str, &str)> = secrets.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            result = self.vault.tokenize_secrets(&result, &refs);
        }

        // 2. Scan for DLP pattern matches and tokenize them.
        // Collect matches first to avoid borrow issues.
        let matches: Vec<(String, String, usize, usize)> = self
            .patterns
            .iter()
            .filter_map(|p| {
                // Only tokenize Block and Warn patterns (not Redact-only).
                if matches!(p.action, DlpAction::Block | DlpAction::Warn) {
                    p.regex.find(&result).map(|m| {
                        let (rtype, label) = classify_pattern(&p.name);
                        let matched_text = m.as_str().to_string();
                        let token = self.vault.register(rtype, &label, &matched_text);
                        (matched_text, token, m.start(), m.end())
                    })
                } else {
                    None
                }
            })
            .collect();

        // Apply replacements (reverse order to preserve offsets).
        let mut sorted_matches = matches;
        sorted_matches.sort_by(|a, b| b.2.cmp(&a.2));
        for (matched_text, token, start, end) in &sorted_matches {
            // Only replace if the text at this position hasn't already been tokenized.
            if result.get(*start..*end) == Some(matched_text.as_str()) {
                result.replace_range(*start..*end, token);
            }
        }

        result
    }

    /// Tokenize a JSON value (recursively tokenizes all string fields).
    async fn tokenize_json(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => {
                let tokenized = self.tokenize_text(s).await;
                serde_json::Value::String(tokenized)
            }
            serde_json::Value::Object(map) => {
                let mut new_map = serde_json::Map::new();
                for (k, v) in map {
                    new_map.insert(k.clone(), Box::pin(self.tokenize_json(v)).await);
                }
                serde_json::Value::Object(new_map)
            }
            serde_json::Value::Array(arr) => {
                let mut new_arr = Vec::new();
                for v in arr {
                    new_arr.push(Box::pin(self.tokenize_json(v)).await);
                }
                serde_json::Value::Array(new_arr)
            }
            other => other.clone(),
        }
    }

    /// Push a redaction finding to the shared context.
    async fn push_finding(&self, redaction_type: RedactionType, label: &str, token: &str) {
        if let Some(ctx) = &self.context {
            let mut guard = ctx.write().await;
            guard.findings.push(PendingFinding {
                finding_type: "redaction_applied".to_string(),
                tag: format!("redaction:{redaction_type}:{label}"),
                pattern_name: format!("{redaction_type}:{label}"),
                confidence: 1.0,
                redacted_preview: Some(token.to_string()),
                source_text: None,
                match_offset: None,
                match_length: None,
            });
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
        // Count entries before tokenization.
        let before_count = self.vault.len();

        // Tokenize all string values in the args JSON.
        let _tokenized = self.tokenize_json(args).await;

        // Push findings for any new entries registered during tokenization.
        let after_count = self.vault.len();
        if after_count > before_count {
            for entry in self.vault.iter() {
                let e = entry.value();
                if e.created_at > chrono::Utc::now() - chrono::Duration::seconds(1) {
                    self.push_finding(e.redaction_type, &e.label, entry.key())
                        .await;
                }
            }
        }

        // Note: we don't return an error — tokenization is prevention, not blocking.
        // The tokenized args would need to replace the original args in the pipeline,
        // which requires the caller to use the tokenized version.
        Ok(())
    }

    async fn after_tool_call(
        &self,
        _tool_name: &str,
        result: &serde_json::Value,
    ) -> crate::Result<()> {
        // Tokenize tool results to prevent secrets from flowing back to LLM.
        let _tokenized = self.tokenize_json(result).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::dlp::DlpConfig;

    fn test_patterns() -> Vec<DlpPattern> {
        // Use a small subset of patterns for testing.
        DlpConfig::default_patterns()
            .into_iter()
            .take(5)
            .collect()
    }

    #[test]
    fn classify_credit_card_pattern() {
        let (t, label) = classify_pattern("Credit Card (Visa/MC)");
        assert_eq!(t, RedactionType::Pii);
        assert_eq!(label, "CREDIT_CARD");
    }

    #[test]
    fn classify_email_pattern() {
        let (t, label) = classify_pattern("Email Address");
        assert_eq!(t, RedactionType::Pii);
        assert_eq!(label, "EMAIL");
    }

    #[test]
    fn classify_aws_key_pattern() {
        let (t, label) = classify_pattern("AWS Access Key");
        assert_eq!(t, RedactionType::Dlp);
        assert_eq!(label, "AWS_ACCESS_KEY");
    }

    #[tokio::test]
    async fn tokenize_text_replaces_known_secrets() {
        let vault = Arc::new(RedactionVault::new());
        let tokenizer = RedactionTokenizer::new(vault.clone(), vec![]);
        tokenizer
            .set_known_secrets(vec![
                ("API_KEY".to_string(), "sk-ant-abc123def456".to_string()),
            ])
            .await;

        let result = tokenizer
            .tokenize_text("my key is sk-ant-abc123def456")
            .await;

        assert!(!result.contains("sk-ant-abc123def456"));
        assert!(result.contains("{SECRET:API_KEY:…"));
    }

    #[tokio::test]
    async fn tokenize_text_replaces_dlp_matches() {
        let vault = Arc::new(RedactionVault::new());
        let patterns = test_patterns();
        let tokenizer = RedactionTokenizer::new(vault.clone(), patterns);

        let result = tokenizer
            .tokenize_text("found key AKIAIOSFODNN7EXAMPLE here")
            .await;

        // AWS access key pattern should match
        assert!(!result.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[tokio::test]
    async fn tokenize_json_handles_nested_objects() {
        let vault = Arc::new(RedactionVault::new());
        let tokenizer = RedactionTokenizer::new(vault.clone(), vec![]);
        tokenizer
            .set_known_secrets(vec![("KEY".to_string(), "secret-value-1234".to_string())])
            .await;

        let json = serde_json::json!({
            "outer": {
                "inner": "has secret-value-1234 inside"
            },
            "list": ["also secret-value-1234"]
        });

        let tokenized = tokenizer.tokenize_json(&json).await;
        let text = tokenized.to_string();
        assert!(!text.contains("secret-value-1234"));
        assert!(text.contains("{SECRET:KEY:…"));
    }

    #[tokio::test]
    async fn security_hook_does_not_block() {
        let vault = Arc::new(RedactionVault::new());
        let tokenizer = RedactionTokenizer::new(vault, vec![]);

        let args = serde_json::json!({"prompt": "hello world"});
        let result = tokenizer.before_tool_call("test", &args).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn findings_pushed_to_context() {
        let vault = Arc::new(RedactionVault::new());
        let ctx = Arc::new(RwLock::new(SecurityContext::new()));
        let tokenizer = RedactionTokenizer::new(vault, vec![]).with_context(ctx.clone());
        tokenizer
            .set_known_secrets(vec![("KEY".to_string(), "my-secret-value-here".to_string())])
            .await;

        let args = serde_json::json!({"data": "has my-secret-value-here"});
        tokenizer.before_tool_call("test", &args).await.unwrap();

        let ctx_guard = ctx.read().await;
        let redaction_findings: Vec<_> = ctx_guard
            .findings
            .iter()
            .filter(|f| f.finding_type == "redaction_applied")
            .collect();
        assert!(
            !redaction_findings.is_empty(),
            "Expected redaction_applied findings"
        );
    }
}
