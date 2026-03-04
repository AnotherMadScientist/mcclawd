//! DLP (Data Loss Prevention) scanning hook.
//!
//! Scans tool arguments and results for sensitive patterns such as
//! API keys, credit card numbers, SSNs, and other secrets.

use async_trait::async_trait;
use regex::Regex;

use super::SecurityHook;
use crate::McclawdError;

/// Action to take when a DLP pattern matches.
#[derive(Debug, Clone, PartialEq)]
pub enum DlpAction {
    /// Log a warning but allow the call to proceed.
    Warn,
    /// Block the call and return an error.
    Block,
    /// Log the match but allow (for audit trail without blocking).
    Redact,
}

/// A named regex pattern with an associated action.
pub struct DlpPattern {
    pub name: String,
    pub regex: Regex,
    pub action: DlpAction,
}

impl std::fmt::Debug for DlpPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DlpPattern")
            .field("name", &self.name)
            .field("action", &self.action)
            .finish()
    }
}

/// Configuration for the DLP hook.
pub struct DlpConfig {
    pub patterns: Vec<DlpPattern>,
    pub default_action: DlpAction,
}

impl DlpConfig {
    /// Built-in patterns for common secrets and PII.
    pub fn default_patterns() -> Vec<DlpPattern> {
        vec![
            DlpPattern {
                name: "AWS Access Key".to_string(),
                regex: Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
                action: DlpAction::Block,
            },
            DlpPattern {
                name: "AWS Secret Key".to_string(),
                regex: Regex::new(r"(?i)aws_secret_access_key\s*[=:]\s*\S+").unwrap(),
                action: DlpAction::Block,
            },
            DlpPattern {
                name: "Generic API Key".to_string(),
                regex: Regex::new(r#"(?i)(api[_\-]?key|apikey)\s*[=:]\s*["']?\S{20,}"#).unwrap(),
                action: DlpAction::Block,
            },
            DlpPattern {
                name: "Credit Card Number".to_string(),
                regex: Regex::new(r"\b[0-9]{4}[- ]?[0-9]{4}[- ]?[0-9]{4}[- ]?[0-9]{4}\b")
                    .unwrap(),
                action: DlpAction::Warn,
            },
            DlpPattern {
                name: "SSN".to_string(),
                regex: Regex::new(r"\b[0-9]{3}-[0-9]{2}-[0-9]{4}\b").unwrap(),
                action: DlpAction::Warn,
            },
            DlpPattern {
                name: "GitHub Token".to_string(),
                regex: Regex::new(r"gh[pousr]_[A-Za-z0-9_]{36,}").unwrap(),
                action: DlpAction::Block,
            },
            DlpPattern {
                name: "Slack Token".to_string(),
                regex: Regex::new(r"xox[baprs]-[0-9a-zA-Z-]+").unwrap(),
                action: DlpAction::Block,
            },
        ]
    }

    /// Create a config with default patterns.
    pub fn with_defaults() -> Self {
        Self {
            patterns: Self::default_patterns(),
            default_action: DlpAction::Warn,
        }
    }
}

/// DLP scanning hook — checks tool arguments and results for sensitive data.
pub struct DlpHook {
    config: DlpConfig,
}

impl DlpHook {
    pub fn new(config: DlpConfig) -> Self {
        Self { config }
    }

    /// Create a DlpHook with default patterns.
    pub fn with_defaults() -> Self {
        Self::new(DlpConfig::with_defaults())
    }

    /// Scan text against all configured patterns.
    /// Returns a list of (pattern_name, action) for each match.
    fn scan(&self, text: &str) -> Vec<(String, &DlpAction)> {
        let mut matches = Vec::new();
        for pattern in &self.config.patterns {
            if pattern.regex.is_match(text) {
                matches.push((pattern.name.clone(), &pattern.action));
            }
        }
        matches
    }

    /// Process scan results: log warnings, return error for Block actions.
    fn process_matches(
        &self,
        matches: &[(String, &DlpAction)],
        context: &str,
    ) -> crate::Result<Vec<String>> {
        let mut flags = Vec::new();
        for (name, action) in matches {
            match action {
                DlpAction::Block => {
                    tracing::warn!(
                        pattern = %name,
                        context = %context,
                        "DLP: blocked — sensitive data detected"
                    );
                    return Err(McclawdError::Tool(format!(
                        "DLP violation: {} detected in {}",
                        name, context
                    )));
                }
                DlpAction::Warn => {
                    tracing::warn!(
                        pattern = %name,
                        context = %context,
                        "DLP: warning — possible sensitive data"
                    );
                    flags.push(name.clone());
                }
                DlpAction::Redact => {
                    tracing::info!(
                        pattern = %name,
                        context = %context,
                        "DLP: redact — sensitive data noted"
                    );
                    flags.push(name.clone());
                }
            }
        }
        Ok(flags)
    }
}

#[async_trait]
impl SecurityHook for DlpHook {
    async fn before_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> crate::Result<()> {
        let text = args.to_string();
        let matches = self.scan(&text);
        if !matches.is_empty() {
            let context = format!("tool '{}' args", tool_name);
            self.process_matches(&matches, &context)?;
        }
        Ok(())
    }

    async fn after_tool_call(
        &self,
        tool_name: &str,
        result: &serde_json::Value,
    ) -> crate::Result<()> {
        let text = result.to_string();
        let matches = self.scan(&text);
        if !matches.is_empty() {
            let context = format!("tool '{}' result", tool_name);
            self.process_matches(&matches, &context)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn detect_aws_key_in_args() {
        let hook = DlpHook::with_defaults();
        let args = serde_json::json!({"key": "AKIAIOSFODNN7EXAMPLE"});
        let result = hook.before_tool_call("test", &args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("AWS Access Key"));
    }

    #[tokio::test]
    async fn detect_ssn_in_result() {
        let hook = DlpHook::with_defaults();
        let result = serde_json::json!({"data": "SSN: 123-45-6789"});
        // SSN is Warn action, so it should pass
        let res = hook.after_tool_call("test", &result).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn detect_credit_card() {
        let hook = DlpHook::with_defaults();
        let args = serde_json::json!({"card": "4111-1111-1111-1111"});
        // Credit card is Warn, should pass
        let res = hook.before_tool_call("test", &args).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn pass_clean_data() {
        let hook = DlpHook::with_defaults();
        let args = serde_json::json!({"message": "Hello, world!"});
        let res = hook.before_tool_call("test", &args).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn block_action_returns_error() {
        let config = DlpConfig {
            patterns: vec![DlpPattern {
                name: "test_secret".to_string(),
                regex: Regex::new(r"SECRET_VALUE").unwrap(),
                action: DlpAction::Block,
            }],
            default_action: DlpAction::Warn,
        };
        let hook = DlpHook::new(config);
        let args = serde_json::json!({"val": "SECRET_VALUE"});
        let res = hook.before_tool_call("test", &args).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn warn_action_passes() {
        let config = DlpConfig {
            patterns: vec![DlpPattern {
                name: "test_warn".to_string(),
                regex: Regex::new(r"WARN_ME").unwrap(),
                action: DlpAction::Warn,
            }],
            default_action: DlpAction::Warn,
        };
        let hook = DlpHook::new(config);
        let args = serde_json::json!({"val": "WARN_ME please"});
        let res = hook.before_tool_call("test", &args).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn redact_action_passes() {
        let config = DlpConfig {
            patterns: vec![DlpPattern {
                name: "test_redact".to_string(),
                regex: Regex::new(r"REDACT_THIS").unwrap(),
                action: DlpAction::Redact,
            }],
            default_action: DlpAction::Warn,
        };
        let hook = DlpHook::new(config);
        let args = serde_json::json!({"val": "REDACT_THIS content"});
        let res = hook.before_tool_call("test", &args).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn multiple_patterns_match() {
        let hook = DlpHook::with_defaults();
        // Contains both SSN (Warn) and AWS key (Block)
        let args = serde_json::json!({
            "ssn": "123-45-6789",
            "key": "AKIAIOSFODNN7EXAMPLE"
        });
        let res = hook.before_tool_call("test", &args).await;
        // Should be blocked because AWS key is Block action
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn detect_github_token() {
        let hook = DlpHook::with_defaults();
        let args = serde_json::json!({
            "token": "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmn"
        });
        let res = hook.before_tool_call("test", &args).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn detect_slack_token() {
        let hook = DlpHook::with_defaults();
        let args = serde_json::json!({
            "token": "xoxb-FAKE-TOKEN-FOR-DLP-TEST"
        });
        let res = hook.before_tool_call("test", &args).await;
        assert!(res.is_err());
    }
}
