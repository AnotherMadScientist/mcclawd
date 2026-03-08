//! Security event types for the DLP/audit pipeline.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Type of security event detected.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEventType {
    DlpMatch,
    SecretDetected,
    PiiDetected,
    InjectionAttempt,
    FlowViolation,
    ToolBlocked,
}

/// Threat severity level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum ThreatLevel {
    Safe,
    Suspicious,
    Dangerous,
    Critical,
}

/// Action taken by the security pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SecurityAction {
    Allowed,
    Warned,
    Blocked,
    Redacted,
}

/// Direction of data flow being scanned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScanDirection {
    /// Scanning tool call arguments (before execution)
    Inbound,
    /// Scanning tool call results (after execution)
    Outbound,
}

/// A single DLP finding (tag + metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlpFinding {
    pub finding_type: String,
    pub tag: String,
    pub pattern_name: String,
    pub confidence: f64,
    pub data_hash: Option<String>,
    pub redacted_preview: Option<String>,
}

/// A structured security event flowing through the pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityEvent {
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub tool_name: String,
    pub direction: ScanDirection,
    pub event_type: SecurityEventType,
    pub threat_level: ThreatLevel,
    pub action: SecurityAction,
    pub findings: Vec<DlpFinding>,
    pub scan_time_ms: f64,
    pub timestamp: DateTime<Utc>,
}

impl SecurityEvent {
    /// Create a new "clean" event (no findings).
    pub fn clean(tool_name: &str, direction: ScanDirection) -> Self {
        Self {
            task_id: None,
            agent_id: None,
            trace_id: None,
            span_id: None,
            tool_name: tool_name.to_string(),
            direction,
            event_type: SecurityEventType::DlpMatch,
            threat_level: ThreatLevel::Safe,
            action: SecurityAction::Allowed,
            findings: vec![],
            scan_time_ms: 0.0,
            timestamp: Utc::now(),
        }
    }
}
