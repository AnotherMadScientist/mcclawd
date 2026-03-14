//! Provider pool implementation with budget controls.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const SECONDS_PER_DAY: u64 = 86400;
const SECONDS_PER_MONTH: u64 = 30 * 86400;

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Supported LLM provider kinds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProviderKind {
    Anthropic,
    OpenAI,
    Ollama,
}

/// A single provider entry in the pool configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub name: String,
    pub kind: ProviderKind,
    pub api_key_secret: String,
    pub models: Vec<String>,
    #[serde(default = "default_priority")]
    pub priority: u8,
    pub max_rpm: Option<u32>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_priority() -> u8 {
    100
}

fn default_true() -> bool {
    true
}

/// Budget limits for provider usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub daily_limit_usd: Option<f64>,
    pub monthly_limit_usd: Option<f64>,
    pub per_task_limit_usd: Option<f64>,
}

/// Configuration for the provider pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPoolConfig {
    pub providers: Vec<ProviderEntry>,
    pub budget: Option<BudgetConfig>,
    pub fallback_order: Option<Vec<String>>,
}

/// Atomic usage record for a single provider.
#[derive(Debug)]
pub struct UsageRecord {
    pub tokens: AtomicU64,
    /// Cost in 1/1000 cents (millicents) for atomic integer precision.
    pub cost_millicents: AtomicU64,
    pub requests: AtomicU64,
}

impl UsageRecord {
    fn new() -> Self {
        Self {
            tokens: AtomicU64::new(0),
            cost_millicents: AtomicU64::new(0),
            requests: AtomicU64::new(0),
        }
    }
}

/// Budget tracking with windowed daily/monthly spend counters and auto-reset.
#[derive(Debug)]
struct BudgetTracker {
    daily_cost_millicents: AtomicU64,
    monthly_cost_millicents: AtomicU64,
    /// Epoch second when the current daily period started.
    daily_period_start: AtomicU64,
    /// Epoch second when the current monthly period started.
    monthly_period_start: AtomicU64,
}

impl BudgetTracker {
    fn new() -> Self {
        let now = now_epoch_secs();
        Self {
            daily_cost_millicents: AtomicU64::new(0),
            monthly_cost_millicents: AtomicU64::new(0),
            daily_period_start: AtomicU64::new(now),
            monthly_period_start: AtomicU64::new(now),
        }
    }

    /// Record a cost, auto-resetting if the period has elapsed.
    fn record_cost(&self, cost_usd: f64) {
        self.maybe_reset();
        let millicents = (cost_usd * 100_000.0) as u64;
        self.daily_cost_millicents
            .fetch_add(millicents, Ordering::Relaxed);
        self.monthly_cost_millicents
            .fetch_add(millicents, Ordering::Relaxed);
    }

    /// Reset counters if the daily or monthly period has elapsed.
    fn maybe_reset(&self) {
        let now = now_epoch_secs();
        let daily_start = self.daily_period_start.load(Ordering::Relaxed);
        if now.saturating_sub(daily_start) >= SECONDS_PER_DAY {
            self.daily_cost_millicents.store(0, Ordering::Relaxed);
            self.daily_period_start.store(now, Ordering::Relaxed);
        }
        let monthly_start = self.monthly_period_start.load(Ordering::Relaxed);
        if now.saturating_sub(monthly_start) >= SECONDS_PER_MONTH {
            self.monthly_cost_millicents.store(0, Ordering::Relaxed);
            self.monthly_period_start.store(now, Ordering::Relaxed);
        }
    }

    /// Add to monthly cost only (for hydrating past days in the current month).
    fn record_monthly_only(&self, cost_usd: f64) {
        let millicents = (cost_usd * 100_000.0) as u64;
        self.monthly_cost_millicents
            .fetch_add(millicents, Ordering::Relaxed);
    }

    fn daily_spend_usd(&self) -> f64 {
        self.maybe_reset();
        self.daily_cost_millicents.load(Ordering::Relaxed) as f64 / 100_000.0
    }

    fn monthly_spend_usd(&self) -> f64 {
        self.maybe_reset();
        self.monthly_cost_millicents.load(Ordering::Relaxed) as f64 / 100_000.0
    }
}

/// Aggregated usage summary across all providers.
#[derive(Debug, Clone, Serialize)]
pub struct UsageSummary {
    pub total_tokens: u64,
    pub total_cost_usd: f64,
    pub total_requests: u64,
    pub per_provider: Vec<ProviderUsage>,
}

/// Usage data for a single provider.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderUsage {
    pub name: String,
    pub tokens: u64,
    pub cost_usd: f64,
    pub requests: u64,
}

/// Budget alert severity levels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BudgetAlertLevel {
    /// Within budget (< 80% used).
    Ok,
    /// Approaching budget limit (>= 80% used).
    Warning,
    /// Budget limit exceeded (>= 100% used).
    Exceeded,
}

/// Detail for a single budget dimension (daily or monthly).
#[derive(Debug, Clone, Serialize)]
pub struct BudgetAlertDetail {
    pub level: BudgetAlertLevel,
    pub spent_usd: f64,
    pub limit_usd: f64,
    pub percent_used: f64,
}

/// Budget alert status across all dimensions.
#[derive(Debug, Clone, Serialize)]
pub struct BudgetAlerts {
    pub daily: Option<BudgetAlertDetail>,
    pub monthly: Option<BudgetAlertDetail>,
    pub per_task_limit_usd: Option<f64>,
    /// Per-task alerts for tasks approaching or exceeding per-task limit.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub per_task: Vec<TaskBudgetAlert>,
}

/// Budget alert for a specific task's accumulated spend.
#[derive(Debug, Clone, Serialize)]
pub struct TaskBudgetAlert {
    pub task_id: String,
    pub detail: BudgetAlertDetail,
}

/// Estimate cost in USD for a given model and token counts.
/// Approximate pricing for display purposes only.
pub fn estimate_cost_usd(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
    match model {
        m if m.contains("opus") => {
            (input_tokens as f64 * 15.0 + output_tokens as f64 * 75.0) / 1_000_000.0
        }
        m if m.contains("sonnet") => {
            (input_tokens as f64 * 3.0 + output_tokens as f64 * 15.0) / 1_000_000.0
        }
        m if m.contains("haiku") => {
            (input_tokens as f64 * 0.25 + output_tokens as f64 * 1.25) / 1_000_000.0
        }
        _ => (input_tokens as f64 * 3.0 + output_tokens as f64 * 15.0) / 1_000_000.0, // default to sonnet pricing
    }
}

/// Per-model usage breakdown entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsageEntry {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
    pub request_count: u64,
}

/// Per-task usage entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskUsageEntry {
    pub task_id: String,
    pub prompt_preview: String,
    pub model: String,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
}

/// A single day's aggregated usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyUsage {
    /// ISO-8601 date string, e.g. "2026-03-06".
    pub date: String,
    pub cost_usd: f64,
    pub tokens: u64,
}

/// Enhanced usage summary with per-model and per-task breakdowns.
#[derive(Debug, Clone, Serialize)]
pub struct DetailedUsageSummary {
    pub by_model: Vec<ModelUsageEntry>,
    pub by_task: Vec<TaskUsageEntry>,
    pub total: ModelUsageEntry,
    pub period: String,
    /// Daily usage history (up to 365 entries), oldest first.
    pub daily_history: Vec<DailyUsage>,
}

/// Account credit/cost information from Anthropic Admin API or local tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountCredits {
    /// Source of the data: "admin_api" or "local_tracking".
    pub source: String,
    /// Actual or estimated cost this month in USD.
    pub monthly_cost_usd: f64,
    /// Whether real data is available (admin API key configured).
    pub data_available: bool,
}

/// Flat budget info for the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct BudgetInfo {
    pub daily_limit_usd: Option<f64>,
    pub monthly_limit_usd: Option<f64>,
    pub daily_spent_usd: f64,
    pub monthly_spent_usd: f64,
    pub alerts: Vec<String>,
    /// Optional account credit/cost info from Anthropic Admin API or local tracking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_credits: Option<AccountCredits>,
}

const MAX_TASK_USAGE_ENTRIES: usize = 50;

fn budget_alert_detail(spent: f64, limit: f64) -> BudgetAlertDetail {
    let percent = if limit > 0.0 {
        (spent / limit) * 100.0
    } else {
        0.0
    };
    let level = if percent >= 100.0 {
        BudgetAlertLevel::Exceeded
    } else if percent >= 80.0 {
        BudgetAlertLevel::Warning
    } else {
        BudgetAlertLevel::Ok
    };
    BudgetAlertDetail {
        level,
        spent_usd: spent,
        limit_usd: limit,
        percent_used: percent,
    }
}

/// Persisted usage snapshot (saved to JSON file).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedUsage {
    daily_history: Vec<DailyUsage>,
    model_usage: Vec<ModelUsageEntry>,
    task_usage: Vec<TaskUsageEntry>,
}

/// Provider pool managing multiple LLM providers with budget and rate controls.
pub struct ProviderPool {
    config: ProviderPoolConfig,
    usage: DashMap<String, Arc<UsageRecord>>,
    budget_tracker: BudgetTracker,
    /// Per-model usage tracking (model_name -> ModelUsageEntry).
    model_usage: DashMap<String, ModelUsageEntry>,
    /// Recent per-task usage entries (bounded to MAX_TASK_USAGE_ENTRIES).
    task_usage: Mutex<Vec<TaskUsageEntry>>,
    /// Daily aggregated usage history (bounded to 365 entries), oldest first.
    daily_history: Mutex<Vec<DailyUsage>>,
    /// Optional path to persist usage data as JSON file (fallback when no DB).
    data_dir: Option<PathBuf>,
    /// Per-task accumulated cost in millicents (task_id -> cost_millicents).
    task_cost_millicents: DashMap<String, AtomicU64>,
}

impl ProviderPool {
    /// Create a new provider pool from configuration.
    pub fn new(config: ProviderPoolConfig) -> Self {
        Self::with_data_dir(config, None)
    }

    /// Create a new provider pool with optional file-based persistence directory.
    pub fn with_data_dir(config: ProviderPoolConfig, data_dir: Option<PathBuf>) -> Self {
        let usage = DashMap::new();
        // Pre-populate usage records for all configured providers.
        for provider in &config.providers {
            usage.insert(provider.name.clone(), Arc::new(UsageRecord::new()));
        }
        let mut pool = Self {
            config,
            usage,
            budget_tracker: BudgetTracker::new(),
            model_usage: DashMap::new(),
            task_usage: Mutex::new(Vec::new()),
            daily_history: Mutex::new(Vec::new()),
            data_dir: data_dir.clone(),
            task_cost_millicents: DashMap::new(),
        };
        // Load persisted usage from file if available
        if let Some(ref dir) = data_dir {
            pool.load_from_file(dir);
        }
        pool
    }

    /// Load usage data from a JSON file in the given directory.
    fn load_from_file(&mut self, dir: &std::path::Path) {
        let path = dir.join("usage_history.json");
        if !path.exists() {
            return;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<PersistedUsage>(&content) {
                Ok(data) => {
                    if let Ok(mut h) = self.daily_history.lock() {
                        *h = data.daily_history;
                    }
                    for entry in data.model_usage {
                        self.model_usage.insert(entry.model.clone(), entry);
                    }
                    if let Ok(mut t) = self.task_usage.lock() {
                        *t = data.task_usage;
                    }
                    // Hydrate budget tracker
                    self.hydrate_budget_from_history();
                    tracing::info!("Usage data loaded from {}", path.display());
                }
                Err(e) => tracing::warn!("Failed to parse usage_history.json: {e}"),
            },
            Err(e) => tracing::warn!("Failed to read usage_history.json: {e}"),
        }
    }

    /// Save current usage data to JSON file (if data_dir is configured).
    fn save_to_file(&self) {
        let Some(ref dir) = self.data_dir else { return };
        let data = PersistedUsage {
            daily_history: self.daily_history.lock().map(|h| h.clone()).unwrap_or_default(),
            model_usage: self.model_usage.iter().map(|e| e.value().clone()).collect(),
            task_usage: self.task_usage.lock().map(|t| t.clone()).unwrap_or_default(),
        };
        let path = dir.join("usage_history.json");
        match serde_json::to_string(&data) {
            Ok(json) => {
                let tmp = path.with_extension("json.tmp");
                if std::fs::write(&tmp, &json).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
            Err(e) => tracing::warn!("Failed to serialize usage data: {e}"),
        }
    }

    /// Hydrate budget tracker from loaded daily history.
    fn hydrate_budget_from_history(&self) {
        if let Ok(history) = self.daily_history.lock() {
            let today = {
                let now = now_epoch_secs();
                let days = now / 86400;
                let z = days as i64 + 719468;
                let era = if z >= 0 { z } else { z - 146096 } / 146097;
                let doe = z - era * 146097;
                let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
                let y = yoe + era * 400;
                let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
                let mp = (5 * doy + 2) / 153;
                let d = doy - (153 * mp + 2) / 5 + 1;
                let m = if mp < 10 { mp + 3 } else { mp - 9 };
                let y = if m <= 2 { y + 1 } else { y };
                format!("{:04}-{:02}-{:02}", y, m, d)
            };
            for entry in history.iter() {
                if entry.date == today {
                    self.budget_tracker.record_cost(entry.cost_usd);
                } else if entry.date.starts_with(&today[..7]) {
                    self.budget_tracker.record_monthly_only(entry.cost_usd);
                }
            }
        }
    }

    /// Select the best available provider for a model.
    ///
    /// Picks by priority (lowest number = highest priority), skipping
    /// disabled providers and those over budget or rate limit.
    /// If `fallback_order` is configured, uses that order instead of priority.
    pub fn select_provider(&self, model: &str) -> anyhow::Result<ProviderEntry> {
        // Collect candidates that support the requested model.
        let mut candidates: Vec<&ProviderEntry> = self
            .config
            .providers
            .iter()
            .filter(|p| p.enabled && p.models.contains(&model.to_string()))
            .collect();

        if candidates.is_empty() {
            anyhow::bail!(
                "No provider found for model '{}' (no enabled provider supports it)",
                model
            );
        }

        // Sort by fallback_order if configured, otherwise by priority.
        if let Some(ref fallback_order) = self.config.fallback_order {
            candidates.sort_by_key(|p| {
                fallback_order
                    .iter()
                    .position(|name| name == &p.name)
                    .unwrap_or(usize::MAX)
            });
        } else {
            candidates.sort_by_key(|p| p.priority);
        }

        // Pick the first candidate that is within budget and rate limits.
        for candidate in &candidates {
            if !self.check_budget() {
                continue;
            }
            if !self.check_rate_limit(&candidate.name) {
                continue;
            }
            return Ok((*candidate).clone());
        }

        // If budget is exceeded for all, return an error.
        anyhow::bail!(
            "All providers for model '{}' are over budget or rate limit",
            model
        )
    }

    /// Select the best available provider for a model, also enforcing per-task budget.
    ///
    /// Like `select_provider` but additionally checks the accumulated spend for
    /// `task_id` against the per-task limit.
    pub fn select_provider_for_task(
        &self,
        model: &str,
        task_id: &str,
    ) -> anyhow::Result<ProviderEntry> {
        // Check per-task budget first (applies to all candidates).
        if !self.check_task_budget_by_id(task_id) {
            let spent = self.get_task_spend_usd(task_id);
            let limit = self
                .config
                .budget
                .as_ref()
                .and_then(|b| b.per_task_limit_usd)
                .unwrap_or(0.0);
            anyhow::bail!(
                "Task '{}' exceeded per-task budget (${:.2} / ${:.2})",
                task_id,
                spent,
                limit
            );
        }
        self.select_provider(model)
    }

    /// Record usage for a provider (backward-compatible: no model tracking).
    pub fn record_usage(&self, provider_name: &str, tokens: u64, cost_usd: f64) {
        self.record_usage_detailed(provider_name, tokens, 0, 0, cost_usd, None);
    }

    /// Record detailed usage with model and optional task info.
    pub fn record_usage_detailed(
        &self,
        provider_name: &str,
        total_tokens: u64,
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
        task_info: Option<(&str, &str, &str)>, // (task_id, prompt_preview, model)
    ) {
        // Provider-level tracking (existing)
        let record = self
            .usage
            .entry(provider_name.to_string())
            .or_insert_with(|| Arc::new(UsageRecord::new()));
        record.tokens.fetch_add(total_tokens, Ordering::Relaxed);
        let millicents = (cost_usd * 100_000.0) as u64;
        record
            .cost_millicents
            .fetch_add(millicents, Ordering::Relaxed);
        record.requests.fetch_add(1, Ordering::Relaxed);
        self.budget_tracker.record_cost(cost_usd);

        // Per-model tracking
        if let Some((_, _, model)) = task_info {
            let mut entry = self
                .model_usage
                .entry(model.to_string())
                .or_insert_with(|| ModelUsageEntry {
                    model: model.to_string(),
                    input_tokens: 0,
                    output_tokens: 0,
                    total_tokens: 0,
                    estimated_cost_usd: 0.0,
                    request_count: 0,
                });
            entry.input_tokens += input_tokens;
            entry.output_tokens += output_tokens;
            entry.total_tokens += total_tokens;
            entry.estimated_cost_usd += cost_usd;
            entry.request_count += 1;
        }

        // Per-task cost accumulation (for budget enforcement)
        if let Some((task_id, _, _)) = task_info {
            self.task_cost_millicents
                .entry(task_id.to_string())
                .or_insert_with(|| AtomicU64::new(0))
                .fetch_add(millicents, Ordering::Relaxed);
        }

        // Per-task tracking
        if let Some((task_id, prompt_preview, model)) = task_info {
            if let Ok(mut tasks) = self.task_usage.lock() {
                // Update existing entry or add new
                if let Some(existing) = tasks.iter_mut().find(|t| t.task_id == task_id) {
                    existing.total_tokens += total_tokens;
                    existing.estimated_cost_usd += cost_usd;
                } else {
                    tasks.push(TaskUsageEntry {
                        task_id: task_id.to_string(),
                        prompt_preview: prompt_preview.to_string(),
                        model: model.to_string(),
                        total_tokens,
                        estimated_cost_usd: cost_usd,
                    });
                    // Keep bounded
                    if tasks.len() > MAX_TASK_USAGE_ENTRIES {
                        tasks.remove(0);
                    }
                }
            }
        }

        // Daily history tracking
        if cost_usd > 0.0 {
            let today = {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                // seconds since epoch → days since epoch → naive date string
                let days = now / 86400;
                // days since 1970-01-01 → year/month/day via Zeller-like calc
                let z = days as i64 + 719468;
                let era = if z >= 0 { z } else { z - 146096 } / 146097;
                let doe = z - era * 146097;
                let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
                let y = yoe + era * 400;
                let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
                let mp = (5 * doy + 2) / 153;
                let d = doy - (153 * mp + 2) / 5 + 1;
                let m = if mp < 10 { mp + 3 } else { mp - 9 };
                let y = if m <= 2 { y + 1 } else { y };
                format!("{:04}-{:02}-{:02}", y, m, d)
            };
            if let Ok(mut history) = self.daily_history.lock() {
                if let Some(last) = history.last_mut() {
                    if last.date == today {
                        last.cost_usd += cost_usd;
                        last.tokens += total_tokens;
                    } else {
                        history.push(DailyUsage { date: today, cost_usd, tokens: total_tokens });
                        if history.len() > 365 {
                            history.remove(0);
                        }
                    }
                } else {
                    history.push(DailyUsage { date: today, cost_usd, tokens: total_tokens });
                }
            }
        }

        // Persist to file after every update
        self.save_to_file();
    }

    /// Get current usage summary across all providers.
    pub fn get_usage(&self) -> UsageSummary {
        let mut total_tokens = 0u64;
        let mut total_cost_millicents = 0u64;
        let mut total_requests = 0u64;
        let mut per_provider = Vec::new();

        for entry in self.usage.iter() {
            let name = entry.key().clone();
            let tokens = entry.value().tokens.load(Ordering::Relaxed);
            let cost_mc = entry.value().cost_millicents.load(Ordering::Relaxed);
            let requests = entry.value().requests.load(Ordering::Relaxed);

            total_tokens += tokens;
            total_cost_millicents += cost_mc;
            total_requests += requests;

            per_provider.push(ProviderUsage {
                name,
                tokens,
                cost_usd: cost_mc as f64 / 100_000.0,
                requests,
            });
        }

        // Sort by name for deterministic output.
        per_provider.sort_by(|a, b| a.name.cmp(&b.name));

        UsageSummary {
            total_tokens,
            total_cost_usd: total_cost_millicents as f64 / 100_000.0,
            total_requests,
            per_provider,
        }
    }

    /// Get detailed usage breakdown by model and task.
    pub fn get_detailed_usage(&self) -> DetailedUsageSummary {
        let mut by_model: Vec<ModelUsageEntry> = self
            .model_usage
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        by_model.sort_by(|a, b| {
            b.estimated_cost_usd
                .partial_cmp(&a.estimated_cost_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let by_task = self
            .task_usage
            .lock()
            .map(|tasks| {
                let mut t = tasks.clone();
                t.reverse(); // most recent first
                t
            })
            .unwrap_or_default();

        let total = ModelUsageEntry {
            model: "all".to_string(),
            input_tokens: by_model.iter().map(|m| m.input_tokens).sum(),
            output_tokens: by_model.iter().map(|m| m.output_tokens).sum(),
            total_tokens: by_model.iter().map(|m| m.total_tokens).sum(),
            estimated_cost_usd: by_model.iter().map(|m| m.estimated_cost_usd).sum(),
            request_count: by_model.iter().map(|m| m.request_count).sum(),
        };

        let daily_history = self
            .daily_history
            .lock()
            .map(|h| h.clone())
            .unwrap_or_default();

        DetailedUsageSummary {
            by_model,
            by_task,
            total,
            period: "daily".to_string(),
            daily_history,
        }
    }

    /// Hydrate in-memory usage data from persisted DB records.
    /// Called once on startup after loading from PostgreSQL.
    /// Only replaces file-loaded data when Postgres has records; preserves
    /// file-based data as fallback when Postgres tables are empty.
    pub fn hydrate_usage(
        &self,
        daily: Vec<DailyUsage>,
        models: Vec<ModelUsageEntry>,
        tasks: Vec<TaskUsageEntry>,
    ) {
        // Daily history — only replace if DB has data (preserve file-loaded fallback)
        if !daily.is_empty() {
            if let Ok(mut history) = self.daily_history.lock() {
                *history = daily;
            }
        }
        // Model usage (merges via and_modify — already correct)
        for entry in models {
            self.model_usage
                .entry(entry.model.clone())
                .and_modify(|e| {
                    e.input_tokens += entry.input_tokens;
                    e.output_tokens += entry.output_tokens;
                    e.total_tokens += entry.total_tokens;
                    e.estimated_cost_usd += entry.estimated_cost_usd;
                    e.request_count += entry.request_count;
                })
                .or_insert(entry);
        }
        // Task usage — only replace if DB has data (preserve file-loaded fallback)
        if !tasks.is_empty() {
            if let Ok(mut task_vec) = self.task_usage.lock() {
                *task_vec = tasks;
            }
        }
        // Update budget tracker from hydrated history
        self.hydrate_budget_from_history();
        tracing::info!("Provider pool hydrated from database");
    }

    /// Get flat budget info for the frontend.
    pub fn get_budget_info(&self) -> BudgetInfo {
        let budget = &self.config.budget;
        let daily_limit = budget.as_ref().and_then(|b| b.daily_limit_usd);
        let monthly_limit = budget.as_ref().and_then(|b| b.monthly_limit_usd);
        let daily_spent = self.budget_tracker.daily_spend_usd();
        let monthly_spent = self.budget_tracker.monthly_spend_usd();

        let mut alerts = Vec::new();
        if let Some(limit) = daily_limit {
            let pct = if limit > 0.0 {
                (daily_spent / limit) * 100.0
            } else {
                0.0
            };
            if pct >= 100.0 {
                alerts.push(format!("Daily budget exceeded (${:.2} / ${:.2})", daily_spent, limit));
            } else if pct >= 80.0 {
                alerts.push(format!(
                    "Daily spend at {:.0}% of limit (${:.2} / ${:.2})",
                    pct, daily_spent, limit
                ));
            }
        }
        if let Some(limit) = monthly_limit {
            let pct = if limit > 0.0 {
                (monthly_spent / limit) * 100.0
            } else {
                0.0
            };
            if pct >= 100.0 {
                alerts.push(format!(
                    "Monthly budget exceeded (${:.2} / ${:.2})",
                    monthly_spent, limit
                ));
            } else if pct >= 80.0 {
                alerts.push(format!(
                    "Monthly spend at {:.0}% of limit (${:.2} / ${:.2})",
                    pct, monthly_spent, limit
                ));
            }
        }

        BudgetInfo {
            daily_limit_usd: daily_limit,
            monthly_limit_usd: monthly_limit,
            daily_spent_usd: daily_spent,
            monthly_spent_usd: monthly_spent,
            alerts,
            account_credits: None,
        }
    }

    /// Check if the pool is within budget limits.
    ///
    /// Returns `true` if no budget is configured or if spending is within limits.
    /// Uses windowed daily/monthly counters with automatic reset.
    pub fn check_budget(&self) -> bool {
        let budget = match &self.config.budget {
            Some(b) => b,
            None => return true, // No budget = no limit.
        };

        if let Some(daily) = budget.daily_limit_usd {
            if self.budget_tracker.daily_spend_usd() >= daily {
                return false;
            }
        }

        if let Some(monthly) = budget.monthly_limit_usd {
            if self.budget_tracker.monthly_spend_usd() >= monthly {
                return false;
            }
        }

        true
    }

    /// Check if a task with the given estimated cost is within the per-task budget.
    ///
    /// Returns `true` if no per-task limit is configured or if the cost is within limit.
    pub fn check_task_budget(&self, estimated_cost_usd: f64) -> bool {
        let budget = match &self.config.budget {
            Some(b) => b,
            None => return true,
        };
        match budget.per_task_limit_usd {
            Some(limit) => estimated_cost_usd <= limit,
            None => true,
        }
    }

    /// Check if a running task's accumulated spend is within the per-task budget.
    ///
    /// Returns `true` if no per-task limit is configured, the task has no recorded
    /// spend, or if the accumulated cost is within the limit.
    pub fn check_task_budget_by_id(&self, task_id: &str) -> bool {
        let budget = match &self.config.budget {
            Some(b) => b,
            None => return true,
        };
        let limit = match budget.per_task_limit_usd {
            Some(l) => l,
            None => return true,
        };
        let spent = self.get_task_spend_usd(task_id);
        spent <= limit
    }

    /// Get accumulated spend for a specific task in USD.
    pub fn get_task_spend_usd(&self, task_id: &str) -> f64 {
        self.task_cost_millicents
            .get(task_id)
            .map(|v| v.load(Ordering::Relaxed) as f64 / 100_000.0)
            .unwrap_or(0.0)
    }

    /// Clear per-task cost tracking for a completed task.
    pub fn clear_task_cost(&self, task_id: &str) {
        self.task_cost_millicents.remove(task_id);
    }

    /// Update the budget configuration.
    pub fn update_budget(&mut self, budget: Option<BudgetConfig>) {
        self.config.budget = budget;
    }

    /// Get current budget alert status across all dimensions.
    pub fn get_budget_alerts(&self) -> BudgetAlerts {
        let budget = match &self.config.budget {
            Some(b) => b,
            None => {
                return BudgetAlerts {
                    daily: None,
                    monthly: None,
                    per_task_limit_usd: None,
                    per_task: Vec::new(),
                }
            }
        };

        let daily = budget
            .daily_limit_usd
            .map(|limit| budget_alert_detail(self.budget_tracker.daily_spend_usd(), limit));

        let monthly = budget
            .monthly_limit_usd
            .map(|limit| budget_alert_detail(self.budget_tracker.monthly_spend_usd(), limit));

        // Per-task alerts: check all tracked tasks against per-task limit.
        let per_task = if let Some(limit) = budget.per_task_limit_usd {
            self.task_cost_millicents
                .iter()
                .filter_map(|entry| {
                    let spent = entry.value().load(Ordering::Relaxed) as f64 / 100_000.0;
                    let detail = budget_alert_detail(spent, limit);
                    if detail.level != BudgetAlertLevel::Ok {
                        Some(TaskBudgetAlert {
                            task_id: entry.key().clone(),
                            detail,
                        })
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        BudgetAlerts {
            daily,
            monthly,
            per_task_limit_usd: budget.per_task_limit_usd,
            per_task,
        }
    }

    /// Check if a specific provider is within its rate limit.
    ///
    /// Currently a simplified check — counts total requests against max_rpm.
    /// A production implementation would use a sliding window.
    pub fn check_rate_limit(&self, provider_name: &str) -> bool {
        let provider = match self.config.providers.iter().find(|p| p.name == provider_name) {
            Some(p) => p,
            None => return true, // Unknown provider = no limit.
        };

        let max_rpm = match provider.max_rpm {
            Some(rpm) => rpm,
            None => return true, // No rate limit configured.
        };

        let requests = self
            .usage
            .get(provider_name)
            .map(|r| r.requests.load(Ordering::Relaxed))
            .unwrap_or(0);

        requests < max_rpm as u64
    }

    /// Get a reference to the pool configuration.
    pub fn config(&self) -> &ProviderPoolConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider(name: &str, kind: ProviderKind, models: Vec<&str>, priority: u8) -> ProviderEntry {
        ProviderEntry {
            name: name.to_string(),
            kind,
            api_key_secret: format!("{}_KEY", name.to_uppercase()),
            models: models.into_iter().map(|s| s.to_string()).collect(),
            priority,
            max_rpm: None,
            enabled: true,
        }
    }

    fn make_pool(providers: Vec<ProviderEntry>, budget: Option<BudgetConfig>) -> ProviderPool {
        ProviderPool::new(ProviderPoolConfig {
            providers,
            budget,
            fallback_order: None,
        })
    }

    #[test]
    fn select_by_priority() {
        let pool = make_pool(
            vec![
                make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10),
                make_provider("openai", ProviderKind::OpenAI, vec!["claude-sonnet-4-5", "gpt-4"], 20),
            ],
            None,
        );
        let selected = pool.select_provider("claude-sonnet-4-5").unwrap();
        assert_eq!(selected.name, "anthropic");
    }

    #[test]
    fn select_lower_priority_number_wins() {
        let pool = make_pool(
            vec![
                make_provider("openai", ProviderKind::OpenAI, vec!["gpt-4"], 50),
                make_provider("anthropic", ProviderKind::Anthropic, vec!["gpt-4"], 10),
            ],
            None,
        );
        let selected = pool.select_provider("gpt-4").unwrap();
        assert_eq!(selected.name, "anthropic");
    }

    #[test]
    fn fallback_when_disabled() {
        let mut low_priority = make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10);
        low_priority.enabled = false;
        let pool = make_pool(
            vec![
                low_priority,
                make_provider("openai", ProviderKind::OpenAI, vec!["claude-sonnet-4-5"], 20),
            ],
            None,
        );
        let selected = pool.select_provider("claude-sonnet-4-5").unwrap();
        assert_eq!(selected.name, "openai");
    }

    #[test]
    fn budget_exceeded_blocks_selection() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: Some(1.0),
                monthly_limit_usd: None,
                per_task_limit_usd: None,
            }),
        );
        // Record usage that exceeds the daily limit.
        pool.record_usage("anthropic", 10000, 1.50);
        let result = pool.select_provider("claude-sonnet-4-5");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("over budget"));
    }

    #[test]
    fn rate_limit_blocks_selection() {
        let mut provider = make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10);
        provider.max_rpm = Some(2);
        let pool = make_pool(vec![provider], None);
        // Record 2 requests (at the limit).
        pool.record_usage("anthropic", 100, 0.01);
        pool.record_usage("anthropic", 100, 0.01);
        let result = pool.select_provider("claude-sonnet-4-5");
        assert!(result.is_err());
    }

    #[test]
    fn rate_limit_check_within_limit() {
        let mut provider = make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10);
        provider.max_rpm = Some(10);
        let pool = make_pool(vec![provider], None);
        pool.record_usage("anthropic", 100, 0.01);
        assert!(pool.check_rate_limit("anthropic"));
    }

    #[test]
    fn rate_limit_check_exceeded() {
        let mut provider = make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10);
        provider.max_rpm = Some(2);
        let pool = make_pool(vec![provider], None);
        pool.record_usage("anthropic", 100, 0.01);
        pool.record_usage("anthropic", 100, 0.01);
        assert!(!pool.check_rate_limit("anthropic"));
    }

    #[test]
    fn usage_accumulation() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            None,
        );
        pool.record_usage("anthropic", 1000, 0.05);
        pool.record_usage("anthropic", 2000, 0.10);

        let usage = pool.get_usage();
        assert_eq!(usage.total_tokens, 3000);
        assert_eq!(usage.total_requests, 2);
        // 0.15 USD = 15000 millicents, back to USD.
        assert!((usage.total_cost_usd - 0.15).abs() < 0.001);
    }

    #[test]
    fn usage_summary_per_provider() {
        let pool = make_pool(
            vec![
                make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10),
                make_provider("openai", ProviderKind::OpenAI, vec!["gpt-4"], 20),
            ],
            None,
        );
        pool.record_usage("anthropic", 1000, 0.05);
        pool.record_usage("openai", 500, 0.02);

        let usage = pool.get_usage();
        assert_eq!(usage.per_provider.len(), 2);
        // Sorted by name.
        assert_eq!(usage.per_provider[0].name, "anthropic");
        assert_eq!(usage.per_provider[0].tokens, 1000);
        assert_eq!(usage.per_provider[1].name, "openai");
        assert_eq!(usage.per_provider[1].tokens, 500);
    }

    #[test]
    fn empty_pool_error() {
        let pool = make_pool(vec![], None);
        let result = pool.select_provider("claude-sonnet-4-5");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No provider found"));
    }

    #[test]
    fn model_not_found_error() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            None,
        );
        let result = pool.select_provider("nonexistent-model");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No provider found"));
    }

    #[test]
    fn check_budget_no_config() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            None,
        );
        assert!(pool.check_budget());
    }

    #[test]
    fn check_budget_within_daily() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: Some(10.0),
                monthly_limit_usd: None,
                per_task_limit_usd: None,
            }),
        );
        pool.record_usage("anthropic", 1000, 5.0);
        assert!(pool.check_budget());
    }

    #[test]
    fn check_budget_exceeded_daily() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: Some(1.0),
                monthly_limit_usd: None,
                per_task_limit_usd: None,
            }),
        );
        pool.record_usage("anthropic", 10000, 1.50);
        assert!(!pool.check_budget());
    }

    #[test]
    fn check_budget_exceeded_monthly() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: None,
                monthly_limit_usd: Some(100.0),
                per_task_limit_usd: None,
            }),
        );
        pool.record_usage("anthropic", 10000, 150.0);
        assert!(!pool.check_budget());
    }

    #[test]
    fn fallback_order_overrides_priority() {
        let pool = ProviderPool::new(ProviderPoolConfig {
            providers: vec![
                make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10),
                make_provider("openai", ProviderKind::OpenAI, vec!["claude-sonnet-4-5"], 20),
            ],
            budget: None,
            fallback_order: Some(vec!["openai".to_string(), "anthropic".to_string()]),
        });
        let selected = pool.select_provider("claude-sonnet-4-5").unwrap();
        // Fallback order puts openai first despite higher priority number.
        assert_eq!(selected.name, "openai");
    }

    #[test]
    fn record_usage_unknown_provider() {
        let pool = make_pool(vec![], None);
        // Should not panic — creates a new record.
        pool.record_usage("unknown", 100, 0.01);
        let usage = pool.get_usage();
        assert_eq!(usage.total_tokens, 100);
    }

    #[test]
    fn provider_kind_serialization() {
        let json = serde_json::to_string(&ProviderKind::Anthropic).unwrap();
        assert_eq!(json, "\"Anthropic\"");
        let parsed: ProviderKind = serde_json::from_str("\"OpenAI\"").unwrap();
        assert_eq!(parsed, ProviderKind::OpenAI);
    }

    #[test]
    fn provider_entry_defaults() {
        let json = r#"{
            "name": "test",
            "kind": "Anthropic",
            "api_key_secret": "TEST_KEY",
            "models": ["claude-sonnet-4-5"]
        }"#;
        let entry: ProviderEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.priority, 100); // default_priority
        assert!(entry.enabled); // default_true
        assert!(entry.max_rpm.is_none());
    }

    #[test]
    fn pool_config_roundtrip() {
        let config = ProviderPoolConfig {
            providers: vec![make_provider("test", ProviderKind::Ollama, vec!["llama3"], 5)],
            budget: Some(BudgetConfig {
                daily_limit_usd: Some(10.0),
                monthly_limit_usd: Some(300.0),
                per_task_limit_usd: Some(0.50),
            }),
            fallback_order: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ProviderPoolConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.providers.len(), 1);
        assert_eq!(parsed.providers[0].name, "test");
        assert!(parsed.budget.is_some());
    }

    #[test]
    fn check_rate_limit_no_limit_configured() {
        let provider = make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10);
        assert!(provider.max_rpm.is_none());
        let pool = make_pool(vec![provider], None);
        // Lots of requests, but no rate limit — should always pass.
        for _ in 0..100 {
            pool.record_usage("anthropic", 100, 0.01);
        }
        assert!(pool.check_rate_limit("anthropic"));
    }

    #[test]
    fn check_rate_limit_unknown_provider() {
        let pool = make_pool(vec![], None);
        // Unknown provider = no limit = true.
        assert!(pool.check_rate_limit("nonexistent"));
    }

    // --- Budget enforcement tests ---

    #[test]
    fn per_task_budget_within_limit() {
        let pool = make_pool(
            vec![make_provider(
                "anthropic",
                ProviderKind::Anthropic,
                vec!["claude-sonnet-4-5"],
                10,
            )],
            Some(BudgetConfig {
                daily_limit_usd: None,
                monthly_limit_usd: None,
                per_task_limit_usd: Some(0.50),
            }),
        );
        assert!(pool.check_task_budget(0.30));
    }

    #[test]
    fn per_task_budget_exceeded() {
        let pool = make_pool(
            vec![make_provider(
                "anthropic",
                ProviderKind::Anthropic,
                vec!["claude-sonnet-4-5"],
                10,
            )],
            Some(BudgetConfig {
                daily_limit_usd: None,
                monthly_limit_usd: None,
                per_task_limit_usd: Some(0.50),
            }),
        );
        assert!(!pool.check_task_budget(0.75));
    }

    #[test]
    fn per_task_budget_no_limit() {
        let pool = make_pool(
            vec![make_provider(
                "anthropic",
                ProviderKind::Anthropic,
                vec!["claude-sonnet-4-5"],
                10,
            )],
            None,
        );
        // No budget at all — any cost is fine.
        assert!(pool.check_task_budget(1000.0));
    }

    #[test]
    fn budget_tracker_increments() {
        let pool = make_pool(
            vec![make_provider(
                "anthropic",
                ProviderKind::Anthropic,
                vec!["claude-sonnet-4-5"],
                10,
            )],
            Some(BudgetConfig {
                daily_limit_usd: Some(10.0),
                monthly_limit_usd: Some(100.0),
                per_task_limit_usd: None,
            }),
        );
        pool.record_usage("anthropic", 1000, 2.0);
        pool.record_usage("anthropic", 500, 1.0);

        let alerts = pool.get_budget_alerts();
        let daily = alerts.daily.unwrap();
        assert!((daily.spent_usd - 3.0).abs() < 0.001);
        assert_eq!(daily.limit_usd, 10.0);
        assert_eq!(daily.level, BudgetAlertLevel::Ok);

        let monthly = alerts.monthly.unwrap();
        assert!((monthly.spent_usd - 3.0).abs() < 0.001);
    }

    #[test]
    fn budget_alert_warning_at_80_percent() {
        let pool = make_pool(
            vec![make_provider(
                "anthropic",
                ProviderKind::Anthropic,
                vec!["claude-sonnet-4-5"],
                10,
            )],
            Some(BudgetConfig {
                daily_limit_usd: Some(10.0),
                monthly_limit_usd: None,
                per_task_limit_usd: None,
            }),
        );
        pool.record_usage("anthropic", 5000, 8.5); // 85% of $10

        let alerts = pool.get_budget_alerts();
        let daily = alerts.daily.unwrap();
        assert_eq!(daily.level, BudgetAlertLevel::Warning);
        assert!(daily.percent_used >= 80.0);
    }

    #[test]
    fn budget_alert_exceeded_at_100_percent() {
        let pool = make_pool(
            vec![make_provider(
                "anthropic",
                ProviderKind::Anthropic,
                vec!["claude-sonnet-4-5"],
                10,
            )],
            Some(BudgetConfig {
                daily_limit_usd: Some(10.0),
                monthly_limit_usd: None,
                per_task_limit_usd: None,
            }),
        );
        pool.record_usage("anthropic", 10000, 12.0); // 120% of $10

        let alerts = pool.get_budget_alerts();
        let daily = alerts.daily.unwrap();
        assert_eq!(daily.level, BudgetAlertLevel::Exceeded);
    }

    #[test]
    fn budget_alerts_no_config() {
        let pool = make_pool(
            vec![make_provider(
                "anthropic",
                ProviderKind::Anthropic,
                vec!["claude-sonnet-4-5"],
                10,
            )],
            None,
        );
        let alerts = pool.get_budget_alerts();
        assert!(alerts.daily.is_none());
        assert!(alerts.monthly.is_none());
        assert!(alerts.per_task_limit_usd.is_none());
    }

    #[test]
    fn update_budget_changes_limits() {
        let mut pool = make_pool(
            vec![make_provider(
                "anthropic",
                ProviderKind::Anthropic,
                vec!["claude-sonnet-4-5"],
                10,
            )],
            None,
        );
        // No budget initially.
        assert!(pool.check_budget());

        // Set a tight budget.
        pool.update_budget(Some(BudgetConfig {
            daily_limit_usd: Some(1.0),
            monthly_limit_usd: None,
            per_task_limit_usd: None,
        }));
        pool.record_usage("anthropic", 10000, 2.0);
        assert!(!pool.check_budget());
    }

    #[test]
    fn daily_reset_clears_daily_counter() {
        let pool = make_pool(
            vec![make_provider(
                "anthropic",
                ProviderKind::Anthropic,
                vec!["claude-sonnet-4-5"],
                10,
            )],
            Some(BudgetConfig {
                daily_limit_usd: Some(5.0),
                monthly_limit_usd: None,
                per_task_limit_usd: None,
            }),
        );
        pool.record_usage("anthropic", 10000, 6.0);
        assert!(!pool.check_budget()); // Over daily limit.

        // Simulate daily reset by backdating the period start.
        pool.budget_tracker
            .daily_period_start
            .store(now_epoch_secs() - SECONDS_PER_DAY - 1, Ordering::Relaxed);

        // After reset, daily counter should be 0 again.
        assert!(pool.check_budget());
    }

    #[test]
    fn budget_alert_level_serialization() {
        let json = serde_json::to_string(&BudgetAlertLevel::Warning).unwrap();
        assert_eq!(json, "\"Warning\"");
        let parsed: BudgetAlertLevel = serde_json::from_str("\"Exceeded\"").unwrap();
        assert_eq!(parsed, BudgetAlertLevel::Exceeded);
    }

    // --- Per-task budget enforcement tests ---

    #[test]
    fn task_budget_by_id_within_limit() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: None,
                monthly_limit_usd: None,
                per_task_limit_usd: Some(1.0),
            }),
        );
        // Record some usage for task-1
        pool.record_usage_detailed("anthropic", 500, 200, 300, 0.40, Some(("task-1", "hello", "claude-sonnet-4-5")));
        assert!(pool.check_task_budget_by_id("task-1"));
    }

    #[test]
    fn task_budget_by_id_exceeded() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: None,
                monthly_limit_usd: None,
                per_task_limit_usd: Some(1.0),
            }),
        );
        // Record usage that exceeds per-task limit
        pool.record_usage_detailed("anthropic", 5000, 2000, 3000, 0.60, Some(("task-1", "hello", "claude-sonnet-4-5")));
        pool.record_usage_detailed("anthropic", 5000, 2000, 3000, 0.60, Some(("task-1", "hello", "claude-sonnet-4-5")));
        assert!(!pool.check_task_budget_by_id("task-1"));
    }

    #[test]
    fn task_budget_by_id_no_limit() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            None,
        );
        // No budget configured — always passes.
        pool.record_usage_detailed("anthropic", 50000, 20000, 30000, 100.0, Some(("task-1", "hello", "claude-sonnet-4-5")));
        assert!(pool.check_task_budget_by_id("task-1"));
    }

    #[test]
    fn task_budget_by_id_unknown_task() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: None,
                monthly_limit_usd: None,
                per_task_limit_usd: Some(1.0),
            }),
        );
        // Unknown task has 0 spend — within limit.
        assert!(pool.check_task_budget_by_id("nonexistent-task"));
    }

    #[test]
    fn task_budget_independent_per_task() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: None,
                monthly_limit_usd: None,
                per_task_limit_usd: Some(1.0),
            }),
        );
        // task-1 exceeds budget
        pool.record_usage_detailed("anthropic", 5000, 2000, 3000, 1.50, Some(("task-1", "hello", "claude-sonnet-4-5")));
        // task-2 is within budget
        pool.record_usage_detailed("anthropic", 500, 200, 300, 0.30, Some(("task-2", "world", "claude-sonnet-4-5")));

        assert!(!pool.check_task_budget_by_id("task-1"));
        assert!(pool.check_task_budget_by_id("task-2"));
    }

    #[test]
    fn get_task_spend_usd_accumulates() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            None,
        );
        pool.record_usage_detailed("anthropic", 1000, 400, 600, 0.25, Some(("task-1", "hello", "claude-sonnet-4-5")));
        pool.record_usage_detailed("anthropic", 1000, 400, 600, 0.35, Some(("task-1", "hello", "claude-sonnet-4-5")));
        let spend = pool.get_task_spend_usd("task-1");
        assert!((spend - 0.60).abs() < 0.001);
    }

    #[test]
    fn clear_task_cost_removes_tracking() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: None,
                monthly_limit_usd: None,
                per_task_limit_usd: Some(1.0),
            }),
        );
        pool.record_usage_detailed("anthropic", 5000, 2000, 3000, 1.50, Some(("task-1", "hello", "claude-sonnet-4-5")));
        assert!(!pool.check_task_budget_by_id("task-1"));

        pool.clear_task_cost("task-1");
        assert_eq!(pool.get_task_spend_usd("task-1"), 0.0);
        assert!(pool.check_task_budget_by_id("task-1"));
    }

    #[test]
    fn select_provider_for_task_within_budget() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: Some(100.0),
                monthly_limit_usd: None,
                per_task_limit_usd: Some(1.0),
            }),
        );
        pool.record_usage_detailed("anthropic", 500, 200, 300, 0.30, Some(("task-1", "hello", "claude-sonnet-4-5")));
        let result = pool.select_provider_for_task("claude-sonnet-4-5", "task-1");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "anthropic");
    }

    #[test]
    fn select_provider_for_task_exceeds_per_task_budget() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: Some(100.0),
                monthly_limit_usd: None,
                per_task_limit_usd: Some(1.0),
            }),
        );
        pool.record_usage_detailed("anthropic", 5000, 2000, 3000, 1.50, Some(("task-1", "hello", "claude-sonnet-4-5")));
        let result = pool.select_provider_for_task("claude-sonnet-4-5", "task-1");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("per-task budget"));
    }

    // --- Per-task budget alert tests ---

    #[test]
    fn budget_alerts_include_per_task_warning() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: None,
                monthly_limit_usd: None,
                per_task_limit_usd: Some(1.0),
            }),
        );
        // 85% of $1.00 per-task limit
        pool.record_usage_detailed("anthropic", 5000, 2000, 3000, 0.85, Some(("task-1", "hello", "claude-sonnet-4-5")));
        let alerts = pool.get_budget_alerts();
        assert_eq!(alerts.per_task.len(), 1);
        assert_eq!(alerts.per_task[0].task_id, "task-1");
        assert_eq!(alerts.per_task[0].detail.level, BudgetAlertLevel::Warning);
    }

    #[test]
    fn budget_alerts_include_per_task_exceeded() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: None,
                monthly_limit_usd: None,
                per_task_limit_usd: Some(1.0),
            }),
        );
        pool.record_usage_detailed("anthropic", 5000, 2000, 3000, 1.50, Some(("task-1", "hello", "claude-sonnet-4-5")));
        let alerts = pool.get_budget_alerts();
        assert_eq!(alerts.per_task.len(), 1);
        assert_eq!(alerts.per_task[0].detail.level, BudgetAlertLevel::Exceeded);
    }

    #[test]
    fn budget_alerts_no_per_task_when_ok() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: None,
                monthly_limit_usd: None,
                per_task_limit_usd: Some(1.0),
            }),
        );
        // 30% of $1.00 — should not trigger alert
        pool.record_usage_detailed("anthropic", 500, 200, 300, 0.30, Some(("task-1", "hello", "claude-sonnet-4-5")));
        let alerts = pool.get_budget_alerts();
        assert!(alerts.per_task.is_empty());
    }

    #[test]
    fn monthly_reset_clears_monthly_counter() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: None,
                monthly_limit_usd: Some(50.0),
                per_task_limit_usd: None,
            }),
        );
        pool.record_usage("anthropic", 10000, 60.0);
        assert!(!pool.check_budget()); // Over monthly limit.

        // Simulate monthly reset by backdating the period start.
        pool.budget_tracker
            .monthly_period_start
            .store(now_epoch_secs() - SECONDS_PER_MONTH - 1, Ordering::Relaxed);

        // After reset, monthly counter should be 0 again.
        assert!(pool.check_budget());
    }

    #[test]
    fn budget_info_includes_per_task_alert_text() {
        let pool = make_pool(
            vec![make_provider("anthropic", ProviderKind::Anthropic, vec!["claude-sonnet-4-5"], 10)],
            Some(BudgetConfig {
                daily_limit_usd: Some(10.0),
                monthly_limit_usd: None,
                per_task_limit_usd: None,
            }),
        );
        pool.record_usage("anthropic", 5000, 8.5);
        let info = pool.get_budget_info();
        assert!(!info.alerts.is_empty());
        assert!(info.alerts[0].contains("80%") || info.alerts[0].contains("Daily"));
    }
}
