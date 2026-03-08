//! Taint trace -- tracks data flow through tool calls for security analysis.
//!
//! Each task gets a TaintTrace (conversation-level). Each tool call creates a TaintSpan.
//! The accumulated trace can be converted to OpenAI-format messages for Invariant policy evaluation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single tool call span within a taint trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintSpan {
    pub span_id: String,
    pub tool_name: String,
    pub direction: String,
    pub tags: Vec<String>,
    pub threat_level: String,
    pub action: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Conversation-level taint trace tracking all tool call security events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintTrace {
    pub trace_id: String,
    pub task_id: String,
    pub spans: Vec<TaintSpan>,
    pub created_at: DateTime<Utc>,
}

impl TaintTrace {
    /// Create a new trace for a task.
    pub fn new(task_id: &str) -> Self {
        Self {
            trace_id: Uuid::new_v4().to_string(),
            task_id: task_id.to_string(),
            spans: Vec::new(),
            created_at: Utc::now(),
        }
    }

    /// Start a new span (before tool call).
    pub fn start_span(&mut self, tool_name: &str) -> String {
        let span_id = Uuid::new_v4().to_string();
        self.spans.push(TaintSpan {
            span_id: span_id.clone(),
            tool_name: tool_name.to_string(),
            direction: "inbound".to_string(),
            tags: Vec::new(),
            threat_level: "safe".to_string(),
            action: "allowed".to_string(),
            started_at: Utc::now(),
            completed_at: None,
        });
        span_id
    }

    /// Complete a span with findings (after tool call).
    pub fn complete_span(
        &mut self,
        span_id: &str,
        tags: Vec<String>,
        threat_level: &str,
        action: &str,
    ) {
        if let Some(span) = self.spans.iter_mut().find(|s| s.span_id == span_id) {
            span.tags = tags;
            span.threat_level = threat_level.to_string();
            span.action = action.to_string();
            span.direction = "outbound".to_string();
            span.completed_at = Some(Utc::now());
        }
    }

    /// Convert to OpenAI-format messages for Invariant policy evaluation.
    /// Each tool call becomes a pair of assistant (tool_call) + tool (result) messages.
    pub fn to_invariant_messages(&self) -> Vec<serde_json::Value> {
        let mut messages = Vec::new();
        for (i, span) in self.spans.iter().enumerate() {
            // Assistant message with tool_call
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": &span.span_id,
                    "type": "function",
                    "function": {
                        "name": &span.tool_name,
                        "arguments": serde_json::json!({"span_index": i}).to_string(),
                    }
                }]
            }));
            // Tool output message
            messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": &span.span_id,
                "content": format!("tags: {:?}, threat: {}", span.tags, span.threat_level),
            }));
        }
        messages
    }

    /// Get accumulated tags across all spans (for flow analysis).
    pub fn all_tags(&self) -> Vec<String> {
        self.spans
            .iter()
            .flat_map(|s| s.tags.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect()
    }
}
