//! Provider pool, models, pricing, and config reload API route handlers.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::Deserialize;

use mcclawd_core::providers::{
    AccountCredits, BudgetAlerts, BudgetConfig, BudgetInfo, DailyUsage, DetailedUsageSummary,
    ProviderKind, ProviderPoolConfig, UsageSummary,
};

use super::state::AppState;

// ---------------------------------------------------------------------------
// Existing endpoints
// ---------------------------------------------------------------------------

/// Summary of a provider for the list endpoint.
#[derive(Debug, Serialize)]
pub struct ProviderInfo {
    pub name: String,
    pub kind: ProviderKind,
    pub models: Vec<String>,
    pub enabled: bool,
    pub priority: u8,
}

/// GET /api/providers -- list providers from pool config.
pub async fn list_providers(State(state): State<AppState>) -> Json<Vec<ProviderInfo>> {
    let config = state.config.read().await;
    let pool_config = state.provider_pool_config(&config);

    let providers = pool_config
        .providers
        .iter()
        .map(|p| ProviderInfo {
            name: p.name.clone(),
            kind: p.kind.clone(),
            models: p.models.clone(),
            enabled: p.enabled,
            priority: p.priority,
        })
        .collect();

    Json(providers)
}

/// GET /api/providers/usage -- current usage summary.
pub async fn provider_usage(State(state): State<AppState>) -> Json<UsageSummary> {
    let pool = state.provider_pool.read().await;
    Json(pool.get_usage())
}

/// Query params for the usage/detailed endpoint.
#[derive(Debug, Deserialize)]
pub struct UsageQueryParams {
    /// Granularity: "daily" (default), "monthly", or "hourly" (same as daily).
    pub granularity: Option<String>,
}

/// Aggregate daily usage entries into monthly buckets.
fn aggregate_monthly(daily: &[DailyUsage]) -> Vec<DailyUsage> {
    let mut by_month: BTreeMap<String, (f64, u64)> = BTreeMap::new();
    for d in daily {
        let month = if d.date.len() >= 7 {
            d.date[..7].to_string()
        } else {
            d.date.clone()
        };
        let entry = by_month.entry(month).or_insert((0.0, 0));
        entry.0 += d.cost_usd;
        entry.1 += d.tokens;
    }
    by_month
        .into_iter()
        .map(|(month, (cost_usd, tokens))| DailyUsage {
            date: month,
            cost_usd,
            tokens,
        })
        .collect()
}

/// GET /api/providers/usage/detailed -- detailed usage with per-model and per-task breakdown.
/// Accepts optional `?granularity=daily|monthly|hourly` query param.
pub async fn provider_usage_detailed(
    State(state): State<AppState>,
    Query(params): Query<UsageQueryParams>,
) -> Json<DetailedUsageSummary> {
    let pool = state.provider_pool.read().await;
    let mut summary = pool.get_detailed_usage();

    if let Some(ref gran) = params.granularity {
        if gran == "monthly" {
            summary.daily_history = aggregate_monthly(&summary.daily_history);
            summary.period = "monthly".to_string();
        }
    }

    Json(summary)
}

/// GET /api/providers/budget/info -- flat budget info for the frontend.
/// Enriches pool budget info with account credits from local tracking.
pub async fn budget_info(State(state): State<AppState>) -> Json<BudgetInfo> {
    let pool = state.provider_pool.read().await;
    let mut info = pool.get_budget_info();

    // Enrich with local tracking data (always available)
    info.account_credits = Some(AccountCredits {
        source: "local_tracking".to_string(),
        monthly_cost_usd: info.monthly_spent_usd,
        data_available: true,
    });

    Json(info)
}

/// Response for the credits endpoint.
#[derive(Debug, Serialize)]
pub struct CreditsResponse {
    pub available: bool,
    pub monthly_cost_usd: f64,
    pub source: String,
    /// Admin API error message, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// GET /api/providers/credits -- account credit/cost info.
///
/// If ANTHROPIC_ADMIN_KEY is set in secrets, attempts to fetch real cost data
/// from the Anthropic Admin API. Otherwise falls back to local usage tracking.
pub async fn provider_credits(State(state): State<AppState>) -> Json<CreditsResponse> {
    // Try admin key from secrets
    let admin_key = {
        let secrets = state.secrets.read().await;
        match secrets.as_ref() {
            Some(backend) => backend.get("ANTHROPIC_ADMIN_KEY").await.ok().flatten(),
            None => None,
        }
    };

    if let Some(key) = admin_key {
        // Attempt Anthropic Admin API call
        match fetch_admin_cost_report(&key).await {
            Ok(cost) => {
                return Json(CreditsResponse {
                    available: true,
                    monthly_cost_usd: cost,
                    source: "admin_api".to_string(),
                    error: None,
                });
            }
            Err(e) => {
                tracing::warn!("Anthropic Admin API failed, falling back to local tracking: {e}");
                // Fall through to local tracking — don't surface error to UI
                let pool = state.provider_pool.read().await;
                let info = pool.get_budget_info();
                return Json(CreditsResponse {
                    available: true,
                    monthly_cost_usd: info.monthly_spent_usd,
                    source: "local_tracking".to_string(),
                    error: None,
                });
            }
        }
    }

    // No admin key — return local tracking
    let pool = state.provider_pool.read().await;
    let info = pool.get_budget_info();
    Json(CreditsResponse {
        available: false,
        monthly_cost_usd: info.monthly_spent_usd,
        source: "local_tracking".to_string(),
        error: None,
    })
}

/// Fetch cost report from Anthropic Admin API for the current month.
///
/// Uses `GET /v1/organizations/cost_report` with `starting_at` (required) and
/// `ending_at` (optional) as RFC 3339 timestamps. The response contains
/// `data[].results[].amount` as a decimal string in lowest currency units (cents).
async fn fetch_admin_cost_report(admin_key: &str) -> Result<f64, String> {
    let now = chrono::Utc::now();
    // Start of current month in RFC 3339
    let start = now.format("%Y-%m-01T00:00:00Z").to_string();
    // Now in RFC 3339
    let end = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let resp = client
        .get("https://api.anthropic.com/v1/organizations/cost_report")
        .header("x-api-key", admin_key)
        .header("anthropic-version", "2023-06-01")
        .query(&[
            ("starting_at", start.as_str()),
            ("ending_at", end.as_str()),
            ("bucket_width", "1d"),
        ])
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("API returned {status}: {body}"));
    }

    // Parse the cost report — extract total cost
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    // The Admin API returns { data: [{ results: [{ amount: "123.45", ... }] }] }
    // amount is in lowest currency units (cents) as a decimal string.
    let empty_arr = vec![];
    let mut total_cents: f64 = 0.0;
    for bucket in body["data"].as_array().unwrap_or(&empty_arr) {
        let empty_results = vec![];
        for result in bucket["results"].as_array().unwrap_or(&empty_results) {
            if let Some(amount_str) = result["amount"].as_str() {
                if let Ok(amount) = amount_str.parse::<f64>() {
                    total_cents += amount;
                }
            }
        }
    }

    Ok(total_cents / 100.0) // Convert cents to USD
}

/// Request body for updating budget configuration.
#[derive(Debug, Deserialize)]
pub struct UpdateBudgetRequest {
    pub daily_limit_usd: Option<f64>,
    pub monthly_limit_usd: Option<f64>,
    pub per_task_limit_usd: Option<f64>,
}

/// PUT /api/providers/budget -- update budget limits.
pub async fn update_budget(
    State(state): State<AppState>,
    Json(req): Json<UpdateBudgetRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let budget = if req.daily_limit_usd.is_none()
        && req.monthly_limit_usd.is_none()
        && req.per_task_limit_usd.is_none()
    {
        None
    } else {
        Some(BudgetConfig {
            daily_limit_usd: req.daily_limit_usd,
            monthly_limit_usd: req.monthly_limit_usd,
            per_task_limit_usd: req.per_task_limit_usd,
        })
    };

    let mut pool = state.provider_pool.write().await;
    pool.update_budget(budget.clone());

    Ok(Json(serde_json::json!({
        "status": "ok",
        "budget": budget,
    })))
}

/// GET /api/providers/budget/alerts -- check current budget status.
pub async fn budget_alerts(State(state): State<AppState>) -> Json<BudgetAlerts> {
    let pool = state.provider_pool.read().await;
    Json(pool.get_budget_alerts())
}

// ---------------------------------------------------------------------------
// Live models endpoint (Anthropic API)
// ---------------------------------------------------------------------------

/// A model entry returned by the Anthropic /v1/models API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicModel {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// Response from Anthropic /v1/models.
#[derive(Debug, Deserialize)]
struct AnthropicModelsResponse {
    data: Vec<AnthropicModelRaw>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    last_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicModelRaw {
    id: String,
    display_name: String,
    #[serde(default)]
    created_at: Option<String>,
}

/// Cached models list with TTL.
struct ModelsCache {
    models: Vec<AnthropicModel>,
    fetched_at: Instant,
}

static MODELS_CACHE: std::sync::OnceLock<tokio::sync::RwLock<Option<ModelsCache>>> =
    std::sync::OnceLock::new();

fn models_cache() -> &'static tokio::sync::RwLock<Option<ModelsCache>> {
    MODELS_CACHE.get_or_init(|| tokio::sync::RwLock::new(None))
}

const CACHE_TTL: Duration = Duration::from_secs(3600); // 1 hour

/// GET /api/providers/models -- list available models from Anthropic API (cached 1h).
pub async fn list_models(
    State(state): State<AppState>,
) -> Result<Json<Vec<AnthropicModel>>, (StatusCode, Json<serde_json::Value>)> {
    // Check cache first
    {
        let cache = models_cache().read().await;
        if let Some(ref c) = *cache {
            if c.fetched_at.elapsed() < CACHE_TTL {
                return Ok(Json(c.models.clone()));
            }
        }
    }

    // Get API key from secrets
    let api_key = get_anthropic_key(&state).await.map_err(|e| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": e })),
        )
    })?;

    // Fetch from Anthropic API (paginated) with 10s timeout
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let mut all_models: Vec<AnthropicModel> = Vec::new();
    let mut after_id: Option<String> = None;

    loop {
        let mut req = client
            .get("https://api.anthropic.com/v1/models")
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .query(&[("limit", "100")]);

        if let Some(ref aid) = after_id {
            req = req.query(&[("after_id", aid.as_str())]);
        }

        let resp = req.send().await.map_err(|e| {
            tracing::error!("Failed to fetch models from Anthropic API: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": format!("Anthropic API error: {}", e) })),
            )
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::error!("Anthropic models API returned {}: {}", status, body);
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(
                    serde_json::json!({ "error": format!("Anthropic API returned {}", status) }),
                ),
            ));
        }

        let page: AnthropicModelsResponse = resp.json().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("Failed to parse models response: {}", e) })),
            )
        })?;

        for m in &page.data {
            all_models.push(AnthropicModel {
                id: m.id.clone(),
                display_name: m.display_name.clone(),
                created_at: m.created_at.clone(),
            });
        }

        if !page.has_more {
            break;
        }
        after_id = page.last_id;
    }

    // Sort: newest models first (by id for stability)
    all_models.sort_by(|a, b| a.display_name.cmp(&b.display_name));

    // Update cache
    {
        let mut cache = models_cache().write().await;
        *cache = Some(ModelsCache {
            models: all_models.clone(),
            fetched_at: Instant::now(),
        });
    }

    Ok(Json(all_models))
}

// ---------------------------------------------------------------------------
// Pricing endpoint
// ---------------------------------------------------------------------------

/// Per-model pricing info.
#[derive(Debug, Clone, Serialize)]
pub struct ModelPricing {
    pub model_id: String,
    pub input_price_per_mtok: f64,
    pub output_price_per_mtok: f64,
}

/// Known Anthropic pricing (updated manually as prices change).
/// Prices are per million tokens.
fn known_pricing() -> Vec<ModelPricing> {
    vec![
        ModelPricing {
            model_id: "claude-opus-4-6-20250514".into(),
            input_price_per_mtok: 15.0,
            output_price_per_mtok: 75.0,
        },
        ModelPricing {
            model_id: "claude-sonnet-4-6-20250514".into(),
            input_price_per_mtok: 3.0,
            output_price_per_mtok: 15.0,
        },
        ModelPricing {
            model_id: "claude-haiku-4-5-20251001".into(),
            input_price_per_mtok: 0.80,
            output_price_per_mtok: 4.0,
        },
        // Aliases / short names
        ModelPricing {
            model_id: "claude-opus-4-6".into(),
            input_price_per_mtok: 15.0,
            output_price_per_mtok: 75.0,
        },
        ModelPricing {
            model_id: "claude-sonnet-4-6".into(),
            input_price_per_mtok: 3.0,
            output_price_per_mtok: 15.0,
        },
        ModelPricing {
            model_id: "claude-haiku-4-5".into(),
            input_price_per_mtok: 0.80,
            output_price_per_mtok: 4.0,
        },
        // Older models
        ModelPricing {
            model_id: "claude-sonnet-4-5-20250514".into(),
            input_price_per_mtok: 3.0,
            output_price_per_mtok: 15.0,
        },
        ModelPricing {
            model_id: "claude-3-5-sonnet-20241022".into(),
            input_price_per_mtok: 3.0,
            output_price_per_mtok: 15.0,
        },
        ModelPricing {
            model_id: "claude-3-5-haiku-20241022".into(),
            input_price_per_mtok: 0.80,
            output_price_per_mtok: 4.0,
        },
    ]
}

/// GET /api/providers/pricing -- per-model pricing.
pub async fn model_pricing() -> Json<Vec<ModelPricing>> {
    Json(known_pricing())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read ANTHROPIC_API_KEY from the secrets backend.
/// Falls back to ANTHROPIC_ADMIN_KEY if the regular API key is missing.
async fn get_anthropic_key(state: &AppState) -> Result<String, String> {
    let secrets = state.secrets.read().await;
    match secrets.as_ref() {
        Some(backend) => {
            // Try ANTHROPIC_API_KEY first
            match backend.get("ANTHROPIC_API_KEY").await {
                Ok(Some(key)) if !key.is_empty() => return Ok(key),
                _ => {}
            }
            // Fallback: try ANTHROPIC_ADMIN_KEY
            match backend.get("ANTHROPIC_ADMIN_KEY").await {
                Ok(Some(key)) if !key.is_empty() => {
                    tracing::debug!("Using ANTHROPIC_ADMIN_KEY as fallback for models API");
                    return Ok(key);
                }
                _ => {}
            }
            // Fallback: check env vars
            if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                if !key.is_empty() {
                    return Ok(key);
                }
            }
            if let Ok(key) = std::env::var("ANTHROPIC_ADMIN_KEY") {
                if !key.is_empty() {
                    return Ok(key);
                }
            }
            Err("ANTHROPIC_API_KEY not set. Add it via Settings > Secrets.".into())
        }
        None => {
            // No secrets backend — check env vars
            if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                if !key.is_empty() {
                    return Ok(key);
                }
            }
            if let Ok(key) = std::env::var("ANTHROPIC_ADMIN_KEY") {
                if !key.is_empty() {
                    return Ok(key);
                }
            }
            Err("No secrets backend and ANTHROPIC_API_KEY env var not set".into())
        }
    }
}

// ---------------------------------------------------------------------------
// Config reload
// ---------------------------------------------------------------------------

/// Response for config reload endpoint.
#[derive(Debug, Serialize)]
pub struct ReloadResponse {
    pub status: String,
    pub message: String,
}

/// POST /api/config/reload -- trigger config reload from disk.
pub async fn reload_config(
    State(state): State<AppState>,
) -> Result<Json<ReloadResponse>, StatusCode> {
    match state.reload_config().await {
        Ok(()) => Ok(Json(ReloadResponse {
            status: "ok".to_string(),
            message: "Configuration reloaded successfully".to_string(),
        })),
        Err(e) => {
            tracing::error!("Config reload failed: {}", e);
            Ok(Json(ReloadResponse {
                status: "error".to_string(),
                message: format!("Config reload failed: {}", e),
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcclawd_core::providers::{BudgetConfig, ProviderEntry, ProviderPool};

    #[test]
    fn provider_info_serialization() {
        let info = ProviderInfo {
            name: "anthropic".to_string(),
            kind: ProviderKind::Anthropic,
            models: vec!["claude-sonnet-4-5".to_string()],
            enabled: true,
            priority: 10,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("anthropic"));
        assert!(json.contains("Anthropic"));
        assert!(json.contains("claude-sonnet-4-5"));
    }

    #[test]
    fn reload_response_serialization() {
        let resp = ReloadResponse {
            status: "ok".to_string(),
            message: "Configuration reloaded successfully".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("ok"));
        assert!(json.contains("Configuration reloaded"));
    }

    #[test]
    fn provider_info_empty_models() {
        let info = ProviderInfo {
            name: "test".to_string(),
            kind: ProviderKind::Ollama,
            models: vec![],
            enabled: false,
            priority: 100,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"models\":[]"));
    }

    #[test]
    fn pool_config_default_is_empty() {
        let config = ProviderPoolConfig {
            providers: vec![],
            budget: None,
            fallback_order: None,
        };
        assert!(config.providers.is_empty());
        assert!(config.budget.is_none());
    }

    #[test]
    fn known_pricing_has_entries() {
        let pricing = known_pricing();
        assert!(!pricing.is_empty());
        // Verify opus pricing
        let opus = pricing.iter().find(|p| p.model_id.contains("opus")).unwrap();
        assert_eq!(opus.input_price_per_mtok, 15.0);
        assert_eq!(opus.output_price_per_mtok, 75.0);
    }

    #[test]
    fn model_pricing_serialization() {
        let p = ModelPricing {
            model_id: "claude-sonnet-4-6".into(),
            input_price_per_mtok: 3.0,
            output_price_per_mtok: 15.0,
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("claude-sonnet-4-6"));
        assert!(json.contains("3"));
        assert!(json.contains("15"));
    }

    #[test]
    fn anthropic_model_serialization() {
        let m = AnthropicModel {
            id: "claude-sonnet-4-6-20250514".into(),
            display_name: "Claude Sonnet 4.6".into(),
            created_at: Some("2025-05-14T00:00:00Z".into()),
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("claude-sonnet-4-6"));
        assert!(json.contains("Claude Sonnet 4.6"));
    }
}
