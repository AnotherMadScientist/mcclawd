//! Security API route handlers — events, summary, status, DLP policies, taint trace.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use super::state::AppState;

// ---------------------------------------------------------------------------
// Query-parameter structs
// ---------------------------------------------------------------------------

/// Query parameters for `GET /api/security/events`.
#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    /// Optional task ID filter.
    pub task_id: Option<String>,
    /// Human-readable duration, e.g. "2h", "24h", "7d".
    pub since: Option<String>,
    /// Max rows (default 200).
    pub limit: Option<i64>,
}

/// Query parameters for `GET /api/security/summary`.
#[derive(Debug, Deserialize)]
pub struct SummaryQuery {
    /// Human-readable duration, e.g. "24h", "7d".
    pub since: Option<String>,
}

/// JSON body for `POST /api/security/policies`.
#[derive(Debug, Deserialize)]
pub struct CreatePolicyRequest {
    pub name: String,
    pub description: Option<String>,
    pub tag_pattern: String,
    pub tool_pattern: Option<String>,
    pub action: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a human-readable duration string ("2h", "30m", "7d") into a
/// `chrono::DateTime<Utc>` representing `now - duration`.
fn parse_since(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, unit) = s.split_at(s.len().saturating_sub(1));
    let num: i64 = num_str.parse().ok()?;

    let duration = match unit {
        "m" => chrono::Duration::minutes(num),
        "h" => chrono::Duration::hours(num),
        "d" => chrono::Duration::days(num),
        "w" => chrono::Duration::weeks(num),
        _ => return None,
    };

    Some(chrono::Utc::now() - duration)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/security/events?task_id=X&since=2h&limit=200
pub async fn list_events(
    State(state): State<AppState>,
    Query(params): Query<EventsQuery>,
) -> (StatusCode, Json<serde_json::Value>) {
    let since = params.since.as_deref().and_then(parse_since);
    let limit = params.limit.unwrap_or(200);

    match state
        .pg_store
        .list_security_events(params.task_id.as_deref(), since, limit)
        .await
    {
        Ok(events) => (StatusCode::OK, Json(serde_json::json!(events))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// GET /api/security/summary?since=24h
pub async fn get_summary(
    State(state): State<AppState>,
    Query(params): Query<SummaryQuery>,
) -> (StatusCode, Json<serde_json::Value>) {
    let since = params.since.as_deref().and_then(parse_since);

    // Use a fixed user_id for now (single-tenant Phase 0/1).
    let user_id = "admin";

    match state.pg_store.security_summary(user_id, since).await {
        Ok(summary) => (StatusCode::OK, Json(serde_json::json!(summary))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// GET /api/security/status — pipeline status + sidecar health check.
pub async fn get_status(
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let pipeline_hooks = state.security_pipeline.len();
    let pipeline_active = pipeline_hooks > 0;

    // Check sidecar health (2s timeout, fail gracefully).
    let sidecar_status = check_sidecar_health().await;

    let dlp_pattern_count = mcclawd_core::hooks::DlpHook::with_defaults().pattern_count();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "pipeline_hooks": pipeline_hooks,
            "pipeline_active": pipeline_active,
            "sidecar_healthy": sidecar_status == "healthy",
            "sidecar_status": sidecar_status,
            "sidecar_url": "http://localhost:8082",
            "dlp_pattern_count": dlp_pattern_count,
        })),
    )
}

/// Check security sidecar health with a 2-second timeout.
/// Returns a status string: "healthy", "unhealthy", or "not_configured".
async fn check_sidecar_health() -> &'static str {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap_or_default();

    match client.get("http://localhost:8082/health").send().await {
        Ok(r) if r.status().is_success() => "healthy",
        Ok(_) => "unhealthy",
        // Connection refused / timeout → sidecar not running
        Err(_) => "not_configured",
    }
}

/// GET /api/security/policies — list all DLP policies.
pub async fn list_policies(
    State(state): State<AppState>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.pg_store.list_dlp_policies().await {
        Ok(policies) => (StatusCode::OK, Json(serde_json::json!(policies))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// POST /api/security/policies — create or update a DLP policy.
pub async fn create_policy(
    State(state): State<AppState>,
    Json(body): Json<CreatePolicyRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state
        .pg_store
        .upsert_dlp_policy(
            &body.name,
            body.description.as_deref(),
            &body.tag_pattern,
            body.tool_pattern.as_deref(),
            &body.action,
            body.enabled,
        )
        .await
    {
        Ok(id) => (
            StatusCode::OK,
            Json(serde_json::json!({ "id": id, "name": body.name, "tag_pattern": body.tag_pattern, "action": body.action, "enabled": body.enabled })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// DELETE /api/security/policies/{id} — delete a DLP policy by ID.
pub async fn delete_policy(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.pg_store.delete_dlp_policy(id).await {
        Ok(true) => (StatusCode::NO_CONTENT, Json(serde_json::json!(null))),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Policy not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// GET /api/security/events/grouped?since=24h — events grouped by task with task prompt.
pub async fn list_events_grouped(
    State(state): State<AppState>,
    Query(params): Query<SummaryQuery>,
) -> (StatusCode, Json<serde_json::Value>) {
    let since = params.since.as_deref().and_then(parse_since);

    match state.pg_store.list_events_grouped_by_task(since, 500).await {
        Ok(groups) => (StatusCode::OK, Json(serde_json::json!(groups))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// GET /api/security/trace/{task_id} — taint trace for a task.
///
/// Placeholder: returns the security events for the task, grouped as a trace.
/// Full taint-propagation tracking will be implemented in a later phase.
pub async fn get_trace(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    // Re-use list_security_events filtered to the given task.
    match state
        .pg_store
        .list_security_events(Some(&task_id), None, 500)
        .await
    {
        Ok(events) => (StatusCode::OK, Json(serde_json::json!(events))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// GET /api/security/patterns — list all built-in DLP detection patterns.
pub async fn list_patterns(
    State(state): State<AppState>,
) -> Json<Vec<mcclawd_core::hooks::dlp::DlpPatternInfo>> {
    Json(state.dlp_patterns.clone())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_since_minutes() {
        let dt = parse_since("30m");
        assert!(dt.is_some());
        let diff = chrono::Utc::now() - dt.unwrap();
        assert!((diff.num_minutes() - 30).unsigned_abs() < 2);
    }

    #[test]
    fn test_parse_since_hours() {
        let dt = parse_since("2h");
        assert!(dt.is_some());
        let diff = chrono::Utc::now() - dt.unwrap();
        assert!((diff.num_hours() - 2).unsigned_abs() < 1);
    }

    #[test]
    fn test_parse_since_days() {
        let dt = parse_since("7d");
        assert!(dt.is_some());
        let diff = chrono::Utc::now() - dt.unwrap();
        assert!((diff.num_days() - 7).unsigned_abs() < 1);
    }

    #[test]
    fn test_parse_since_weeks() {
        let dt = parse_since("2w");
        assert!(dt.is_some());
        let diff = chrono::Utc::now() - dt.unwrap();
        assert!((diff.num_weeks() - 2).unsigned_abs() < 1);
    }

    #[test]
    fn test_parse_since_empty() {
        assert!(parse_since("").is_none());
    }

    #[test]
    fn test_parse_since_invalid_unit() {
        assert!(parse_since("5x").is_none());
    }

    #[test]
    fn test_parse_since_invalid_number() {
        assert!(parse_since("abch").is_none());
    }

    #[test]
    fn test_default_true() {
        assert!(default_true());
    }

    #[test]
    fn test_create_policy_request_deserialize() {
        let json = serde_json::json!({
            "name": "block_aws_keys",
            "description": "Block AWS access keys in tool calls",
            "tag_pattern": "aws_.*",
            "tool_pattern": "*",
            "action": "block",
            "enabled": true
        });
        let req: CreatePolicyRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.name, "block_aws_keys");
        assert_eq!(
            req.description.as_deref(),
            Some("Block AWS access keys in tool calls")
        );
        assert_eq!(req.tag_pattern, "aws_.*");
        assert_eq!(req.tool_pattern.as_deref(), Some("*"));
        assert_eq!(req.action, "block");
        assert!(req.enabled);
    }

    #[test]
    fn test_create_policy_request_defaults() {
        let json = serde_json::json!({
            "name": "test",
            "tag_pattern": ".*",
            "action": "warn"
        });
        let req: CreatePolicyRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.name, "test");
        assert!(req.description.is_none());
        assert!(req.tool_pattern.is_none());
        assert!(req.enabled); // default = true
    }
}
