//! Entropy-based secret scanning hook.
//!
//! Detects high-entropy strings that may be secrets or API keys
//! by calculating Shannon entropy of string tokens extracted from JSON.

use async_trait::async_trait;

use super::SecurityHook;
use crate::McclawdError;

/// Configuration for the secret scanner.
pub struct SecretScannerConfig {
    /// Shannon entropy threshold (default: 4.5 bits per character).
    pub entropy_threshold: f64,
    /// Minimum string length to consider (default: 20).
    pub min_length: usize,
}

impl Default for SecretScannerConfig {
    fn default() -> Self {
        Self {
            entropy_threshold: 4.5,
            min_length: 20,
        }
    }
}

/// Entropy-based secret scanning hook.
pub struct SecretScannerHook {
    config: SecretScannerConfig,
}

impl SecretScannerHook {
    pub fn new(config: SecretScannerConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(SecretScannerConfig::default())
    }

    /// Scan JSON value for high-entropy string tokens.
    fn scan_value(&self, value: &serde_json::Value) -> Vec<(String, f64)> {
        let mut flagged = Vec::new();
        self.extract_and_check(value, &mut flagged);
        flagged
    }

    fn extract_and_check(&self, value: &serde_json::Value, flagged: &mut Vec<(String, f64)>) {
        match value {
            serde_json::Value::String(s) => {
                for token in s.split_whitespace() {
                    if token.len() >= self.config.min_length {
                        let entropy = shannon_entropy(token);
                        if entropy >= self.config.entropy_threshold {
                            let preview = if token.len() > 12 {
                                format!("{}...{}", &token[..6], &token[token.len() - 4..])
                            } else {
                                token.to_string()
                            };
                            flagged.push((preview, entropy));
                        }
                    }
                }
            }
            serde_json::Value::Array(arr) => {
                for item in arr {
                    self.extract_and_check(item, flagged);
                }
            }
            serde_json::Value::Object(map) => {
                for (_k, v) in map {
                    self.extract_and_check(v, flagged);
                }
            }
            _ => {}
        }
    }
}

/// Calculate Shannon entropy of a string (bits per character).
pub fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }

    let len = s.len() as f64;
    let mut freq = std::collections::HashMap::new();
    for c in s.chars() {
        *freq.entry(c).or_insert(0u64) += 1;
    }

    freq.values().fold(0.0, |acc, &count| {
        let p = count as f64 / len;
        acc - p * p.log2()
    })
}

#[async_trait]
impl SecurityHook for SecretScannerHook {
    async fn before_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> crate::Result<()> {
        let flagged = self.scan_value(args);
        if !flagged.is_empty() {
            for (preview, entropy) in &flagged {
                tracing::warn!(
                    tool = %tool_name,
                    token_preview = %preview,
                    entropy = %entropy,
                    "Secret scanner: high-entropy token in args"
                );
            }
            return Err(McclawdError::Tool(format!(
                "Secret scanner: {} high-entropy token(s) detected in tool '{}' args",
                flagged.len(),
                tool_name,
            )));
        }
        Ok(())
    }

    async fn after_tool_call(
        &self,
        tool_name: &str,
        result: &serde_json::Value,
    ) -> crate::Result<()> {
        let flagged = self.scan_value(result);
        if !flagged.is_empty() {
            for (preview, entropy) in &flagged {
                tracing::warn!(
                    tool = %tool_name,
                    token_preview = %preview,
                    entropy = %entropy,
                    "Secret scanner: high-entropy token in result"
                );
            }
            // After-call: warn but don't block (data already returned)
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shannon_entropy_empty() {
        assert_eq!(shannon_entropy(""), 0.0);
    }

    #[test]
    fn shannon_entropy_single_char() {
        assert!(shannon_entropy("aaaa") < 0.1);
    }

    #[test]
    fn shannon_entropy_high_for_random() {
        let random = "aB3kM9pQ7rS2tU5vW8xY0z";
        let e = shannon_entropy(random);
        assert!(e > 4.0, "Expected high entropy, got {}", e);
    }

    #[tokio::test]
    async fn high_entropy_base64_detected() {
        let hook = SecretScannerHook::with_defaults();
        let args = serde_json::json!({
            "token": "aB3kM9pQ7rS2tU5vW8xY0zAb1Cd2Ef3Gh"
        });
        let res = hook.before_tool_call("test", &args).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn low_entropy_passes() {
        let hook = SecretScannerHook::with_defaults();
        let args = serde_json::json!({
            "message": "hello world this is a normal message"
        });
        let res = hook.before_tool_call("test", &args).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn short_strings_pass() {
        let hook = SecretScannerHook::with_defaults();
        let args = serde_json::json!({
            "short": "aB3kM9"
        });
        let res = hook.before_tool_call("test", &args).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn configurable_threshold() {
        let config = SecretScannerConfig {
            entropy_threshold: 6.0,
            min_length: 10,
        };
        let hook = SecretScannerHook::new(config);
        let args = serde_json::json!({
            "token": "aB3kM9pQ7rS2tU5vW8xY0z"
        });
        let res = hook.before_tool_call("test", &args).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn after_call_warns_but_passes() {
        let hook = SecretScannerHook::with_defaults();
        let result = serde_json::json!({
            "token": "aB3kM9pQ7rS2tU5vW8xY0zAb1Cd2Ef3Gh"
        });
        let res = hook.after_tool_call("test", &result).await;
        assert!(res.is_ok());
    }
}
