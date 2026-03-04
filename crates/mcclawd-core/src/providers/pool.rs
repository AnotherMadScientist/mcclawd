//! Provider pool implementation with budget controls.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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

/// Provider pool managing multiple LLM providers with budget and rate controls.
pub struct ProviderPool {
    config: ProviderPoolConfig,
    usage: DashMap<String, Arc<UsageRecord>>,
}

impl ProviderPool {
    /// Create a new provider pool from configuration.
    pub fn new(config: ProviderPoolConfig) -> Self {
        let usage = DashMap::new();
        // Pre-populate usage records for all configured providers.
        for provider in &config.providers {
            usage.insert(provider.name.clone(), Arc::new(UsageRecord::new()));
        }
        Self { config, usage }
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

    /// Record usage for a provider.
    pub fn record_usage(&self, provider_name: &str, tokens: u64, cost_usd: f64) {
        let record = self
            .usage
            .entry(provider_name.to_string())
            .or_insert_with(|| Arc::new(UsageRecord::new()));
        record.tokens.fetch_add(tokens, Ordering::Relaxed);
        // Convert USD to millicents: $1.00 = 100_000 millicents.
        let millicents = (cost_usd * 100_000.0) as u64;
        record.cost_millicents.fetch_add(millicents, Ordering::Relaxed);
        record.requests.fetch_add(1, Ordering::Relaxed);
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

    /// Check if the pool is within budget limits.
    ///
    /// Returns `true` if no budget is configured or if spending is within limits.
    /// Currently checks total spending against daily_limit_usd (simplified —
    /// a production implementation would track per-day/per-month windows).
    pub fn check_budget(&self) -> bool {
        let budget = match &self.config.budget {
            Some(b) => b,
            None => return true, // No budget = no limit.
        };

        let usage = self.get_usage();

        if let Some(daily) = budget.daily_limit_usd {
            if usage.total_cost_usd >= daily {
                return false;
            }
        }

        if let Some(monthly) = budget.monthly_limit_usd {
            if usage.total_cost_usd >= monthly {
                return false;
            }
        }

        true
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
}
