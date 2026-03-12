//! User-defined hooks — shell commands or HTTP calls triggered before/after tool use.
//!
//! # Config (JSON5 — in mcclawd.json)
//! ```json5
//! {
//!   "hooks": [
//!     {
//!       "name": "notify-slack",
//!       "trigger": "after_tool_call",
//!       "type": "http",
//!       "url": "https://hooks.slack.com/services/...",
//!       "method": "POST",
//!       "action": "allow",
//!       "timeout_ms": 5000,
//!       "enabled": true
//!     },
//!     {
//!       "name": "block-write-tools",
//!       "trigger": "before_tool_call",
//!       "type": "shell",
//!       "command": "echo 'write blocked' >> /tmp/mc-audit.log",
//!       "pattern": "^write_",
//!       "action": "block",
//!       "message": "Write tools are not permitted in this workspace",
//!       "enabled": true
//!     }
//!   ]
//! }
//! ```

use async_trait::async_trait;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::process::Command;

use super::SecurityHook;
use crate::McclawdError;

/// When the hook fires relative to tool execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserHookTrigger {
    /// Fires before the tool is called; Block action prevents execution.
    BeforeToolCall,
    /// Fires after the tool returns; Block action suppresses the result.
    AfterToolCall,
}

/// What to do when the hook fires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserHookAction {
    /// Allow the call to proceed (hook runs as side-effect only).
    Allow,
    /// Block the call and return an error to the agent.
    Block,
    /// Log a warning but allow the call.
    Warn,
}

/// How the hook side-effect is executed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserHookType {
    /// Run a shell command via `sh -c <command>`.
    Shell,
    /// POST JSON to an HTTP endpoint.
    Http,
}

/// Configuration for a single user-defined hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserHookConfig {
    /// Human-readable name for this hook.
    pub name: String,
    /// When the hook fires.
    pub trigger: UserHookTrigger,
    /// How the hook is executed.
    #[serde(rename = "type")]
    pub hook_type: UserHookType,
    /// Shell: command to run (`sh -c <command>`). HTTP: ignored.
    #[serde(default)]
    pub command: Option<String>,
    /// HTTP: URL to call. Shell: ignored.
    #[serde(default)]
    pub url: Option<String>,
    /// HTTP method (default: `POST`).
    #[serde(default = "UserHookConfig::default_method")]
    pub method: String,
    /// Extra HTTP headers sent with every call.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Optional regex — hook only fires when the tool name matches.
    /// `None` means match all tools.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Action to apply when the hook fires (default: `allow`).
    #[serde(default = "UserHookConfig::default_action")]
    pub action: UserHookAction,
    /// Message shown to the agent/user when action is `block` or `warn`.
    #[serde(default)]
    pub message: Option<String>,
    /// Timeout for the shell/HTTP call in milliseconds (default: 5000).
    #[serde(default = "UserHookConfig::default_timeout_ms")]
    pub timeout_ms: u64,
    /// Whether this hook is active (default: `true`).
    #[serde(default = "UserHookConfig::default_enabled")]
    pub enabled: bool,
}

impl UserHookConfig {
    fn default_method() -> String {
        "POST".to_string()
    }
    fn default_action() -> UserHookAction {
        UserHookAction::Allow
    }
    fn default_timeout_ms() -> u64 {
        5000
    }
    fn default_enabled() -> bool {
        true
    }
}

/// A compiled user hook: config + pre-compiled regex.
pub struct UserHook {
    config: UserHookConfig,
    pattern: Option<Regex>,
    http: reqwest::Client,
}

impl UserHook {
    /// Build a `UserHook` from config. Fails if the pattern regex is invalid.
    pub fn new(config: UserHookConfig) -> crate::Result<Self> {
        let pattern = config
            .pattern
            .as_deref()
            .map(|p| {
                Regex::new(p)
                    .map_err(|e| McclawdError::Config(format!("invalid hook pattern '{p}': {e}")))
            })
            .transpose()?;
        let http = reqwest::Client::new();
        Ok(Self { config, pattern, http })
    }

    /// Returns `true` if this hook should fire for `tool_name`.
    fn matches_tool(&self, tool_name: &str) -> bool {
        match &self.pattern {
            Some(re) => re.is_match(tool_name),
            None => true,
        }
    }

    /// Execute the hook's side-effect (shell or HTTP). Errors are logged but not propagated —
    /// the configured action determines whether to block.
    async fn execute(&self, tool_name: &str, payload: &serde_json::Value) {
        let timeout = Duration::from_millis(self.config.timeout_ms);
        let result = match self.config.hook_type {
            UserHookType::Shell => self.run_shell(tool_name, payload, timeout).await,
            UserHookType::Http => self.run_http(tool_name, payload, timeout).await,
        };
        if let Err(e) = result {
            tracing::warn!(hook = %self.config.name, error = %e, "user hook side-effect error");
        }
    }

    async fn run_shell(
        &self,
        tool_name: &str,
        payload: &serde_json::Value,
        timeout: Duration,
    ) -> crate::Result<()> {
        let cmd = self.config.command.as_deref().ok_or_else(|| {
            McclawdError::Config(format!(
                "hook '{}': shell type requires 'command'",
                self.config.name
            ))
        })?;

        let payload_str = payload.to_string();
        let output = tokio::time::timeout(
            timeout,
            Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .env("MC_HOOK_TOOL", tool_name)
                .env("MC_HOOK_PAYLOAD", &payload_str)
                .env("MC_HOOK_NAME", &self.config.name)
                .output(),
        )
        .await
        .map_err(|_| {
            McclawdError::Tool(format!(
                "hook '{}': shell command timed out after {}ms",
                self.config.name, self.config.timeout_ms
            ))
        })?
        .map_err(|e| {
            McclawdError::Tool(format!(
                "hook '{}': shell exec error: {e}",
                self.config.name
            ))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                hook = %self.config.name,
                exit_code = ?output.status.code(),
                stderr = %stderr,
                "user hook shell command exited non-zero"
            );
        } else {
            tracing::debug!(hook = %self.config.name, "user hook shell command succeeded");
        }
        Ok(())
    }

    async fn run_http(
        &self,
        tool_name: &str,
        payload: &serde_json::Value,
        timeout: Duration,
    ) -> crate::Result<()> {
        let url = self.config.url.as_deref().ok_or_else(|| {
            McclawdError::Config(format!(
                "hook '{}': http type requires 'url'",
                self.config.name
            ))
        })?;

        let body = serde_json::json!({
            "hook": self.config.name,
            "trigger": self.config.trigger,
            "tool_name": tool_name,
            "payload": payload,
        });

        let method = reqwest::Method::from_bytes(self.config.method.as_bytes())
            .unwrap_or(reqwest::Method::POST);

        let mut builder = self
            .http
            .request(method, url)
            .json(&body)
            .timeout(timeout);

        for (k, v) in &self.config.headers {
            builder = builder.header(k, v);
        }

        let resp = builder.send().await.map_err(|e| {
            McclawdError::Tool(format!("hook '{}': http error: {e}", self.config.name))
        })?;

        if !resp.status().is_success() {
            tracing::warn!(
                hook = %self.config.name,
                status = %resp.status(),
                "user hook HTTP call returned non-2xx"
            );
        } else {
            tracing::debug!(hook = %self.config.name, "user hook HTTP call succeeded");
        }
        Ok(())
    }

    /// Apply the configured action: Block → Err, Warn → log + Ok, Allow → Ok.
    fn apply_action(&self, tool_name: &str) -> crate::Result<()> {
        let msg = self
            .config
            .message
            .as_deref()
            .unwrap_or("blocked by user hook");
        match self.config.action {
            UserHookAction::Block => Err(McclawdError::Tool(format!(
                "hook '{}' blocked tool '{}': {}",
                self.config.name, tool_name, msg
            ))),
            UserHookAction::Warn => {
                tracing::warn!(
                    hook = %self.config.name,
                    tool = %tool_name,
                    "user hook warning: {msg}"
                );
                Ok(())
            }
            UserHookAction::Allow => Ok(()),
        }
    }
}

#[async_trait]
impl SecurityHook for UserHook {
    async fn before_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> crate::Result<()> {
        if !self.config.enabled
            || self.config.trigger != UserHookTrigger::BeforeToolCall
            || !self.matches_tool(tool_name)
        {
            return Ok(());
        }
        self.execute(tool_name, args).await;
        self.apply_action(tool_name)
    }

    async fn after_tool_call(
        &self,
        tool_name: &str,
        result: &serde_json::Value,
    ) -> crate::Result<()> {
        if !self.config.enabled
            || self.config.trigger != UserHookTrigger::AfterToolCall
            || !self.matches_tool(tool_name)
        {
            return Ok(());
        }
        self.execute(tool_name, result).await;
        self.apply_action(tool_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell_config(
        trigger: UserHookTrigger,
        action: UserHookAction,
        command: &str,
        pattern: Option<&str>,
        enabled: bool,
    ) -> UserHookConfig {
        UserHookConfig {
            name: "test-hook".to_string(),
            trigger,
            hook_type: UserHookType::Shell,
            command: Some(command.to_string()),
            url: None,
            method: "POST".to_string(),
            headers: HashMap::new(),
            pattern: pattern.map(str::to_string),
            action,
            message: None,
            timeout_ms: 2000,
            enabled,
        }
    }

    #[tokio::test]
    async fn test_shell_hook_executes_command() {
        let hook = UserHook::new(shell_config(
            UserHookTrigger::BeforeToolCall,
            UserHookAction::Allow,
            "echo test",
            None,
            true,
        ))
        .unwrap();
        let result = hook
            .before_tool_call("my_tool", &serde_json::json!({"arg": 1}))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_shell_hook_timeout_allow_passes() {
        // Timeout on the side-effect is swallowed; Allow action still returns Ok.
        let mut cfg = shell_config(
            UserHookTrigger::BeforeToolCall,
            UserHookAction::Allow,
            "sleep 10",
            None,
            true,
        );
        cfg.timeout_ms = 100;
        let hook = UserHook::new(cfg).unwrap();
        let result = hook
            .before_tool_call("my_tool", &serde_json::json!({}))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_block_action_returns_error() {
        let hook = UserHook::new(shell_config(
            UserHookTrigger::BeforeToolCall,
            UserHookAction::Block,
            "echo blocked",
            None,
            true,
        ))
        .unwrap();
        let result = hook
            .before_tool_call("my_tool", &serde_json::json!({}))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("blocked"));
    }

    #[tokio::test]
    async fn test_warn_action_returns_ok() {
        let hook = UserHook::new(shell_config(
            UserHookTrigger::BeforeToolCall,
            UserHookAction::Warn,
            "echo warn",
            None,
            true,
        ))
        .unwrap();
        let result = hook
            .before_tool_call("my_tool", &serde_json::json!({}))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_disabled_hook_skipped() {
        // Disabled Block hook must not block.
        let hook = UserHook::new(shell_config(
            UserHookTrigger::BeforeToolCall,
            UserHookAction::Block,
            "echo cmd",
            None,
            false,
        ))
        .unwrap();
        let result = hook
            .before_tool_call("my_tool", &serde_json::json!({}))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_hook_trigger_filtering_before_vs_after() {
        // AfterToolCall hook should be a no-op in before_tool_call.
        let hook = UserHook::new(shell_config(
            UserHookTrigger::AfterToolCall,
            UserHookAction::Block,
            "echo cmd",
            None,
            true,
        ))
        .unwrap();
        let result = hook
            .before_tool_call("my_tool", &serde_json::json!({}))
            .await;
        assert!(result.is_ok(), "wrong trigger must be a no-op");

        // And it DOES fire in after_tool_call.
        let result = hook
            .after_tool_call("my_tool", &serde_json::json!({}))
            .await;
        assert!(result.is_err(), "correct trigger must apply action");
    }

    #[tokio::test]
    async fn test_pattern_matching_allow_non_matching() {
        let hook = UserHook::new(shell_config(
            UserHookTrigger::BeforeToolCall,
            UserHookAction::Block,
            "echo cmd",
            Some("^write_"),
            true,
        ))
        .unwrap();

        // Non-matching tool → ok
        assert!(
            hook.before_tool_call("read_file", &serde_json::json!({}))
                .await
                .is_ok(),
            "non-matching tool should pass"
        );

        // Matching tool → blocked
        assert!(
            hook.before_tool_call("write_file", &serde_json::json!({}))
                .await
                .is_err(),
            "matching tool should be blocked"
        );
    }

    #[tokio::test]
    async fn test_empty_user_hooks_pass_through() {
        // A hook with Allow action and no pattern is a pure pass-through.
        let hook = UserHook::new(shell_config(
            UserHookTrigger::BeforeToolCall,
            UserHookAction::Allow,
            "true",
            None,
            true,
        ))
        .unwrap();
        assert!(hook
            .before_tool_call("any_tool", &serde_json::json!(null))
            .await
            .is_ok());
        assert!(hook
            .after_tool_call("any_tool", &serde_json::json!(null))
            .await
            .is_ok());
    }

    #[test]
    fn test_invalid_pattern_rejected() {
        let cfg = shell_config(
            UserHookTrigger::BeforeToolCall,
            UserHookAction::Allow,
            "echo cmd",
            Some("[invalid regex"),
            true,
        );
        assert!(UserHook::new(cfg).is_err());
    }

    #[test]
    fn test_serde_roundtrip_json() {
        let cfg = UserHookConfig {
            name: "my-hook".to_string(),
            trigger: UserHookTrigger::AfterToolCall,
            hook_type: UserHookType::Http,
            command: None,
            url: Some("https://example.com/webhook".to_string()),
            method: "POST".to_string(),
            headers: [("x-api-key".to_string(), "secret".to_string())]
                .into_iter()
                .collect(),
            pattern: Some("^exec_".to_string()),
            action: UserHookAction::Warn,
            message: Some("Dangerous tool".to_string()),
            timeout_ms: 3000,
            enabled: true,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: UserHookConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.name, cfg.name);
        assert_eq!(decoded.trigger, cfg.trigger);
        assert_eq!(decoded.action, cfg.action);
        assert_eq!(decoded.url, cfg.url);
        assert_eq!(decoded.pattern, cfg.pattern);
    }

    #[test]
    fn test_serde_json5_roundtrip() {
        let cfg = UserHookConfig {
            name: "shell-hook".to_string(),
            trigger: UserHookTrigger::BeforeToolCall,
            hook_type: UserHookType::Shell,
            command: Some("echo $MC_HOOK_TOOL".to_string()),
            url: None,
            method: "POST".to_string(),
            headers: HashMap::new(),
            pattern: None,
            action: UserHookAction::Allow,
            message: None,
            timeout_ms: 5000,
            enabled: true,
        };
        let json_str = serde_json::to_string(&cfg).unwrap();
        let decoded: UserHookConfig = json5::from_str(&json_str).unwrap();
        assert_eq!(decoded.name, cfg.name);
        assert_eq!(decoded.command, cfg.command);
    }
}
