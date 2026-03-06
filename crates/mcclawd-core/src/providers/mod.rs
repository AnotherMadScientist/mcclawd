//! Provider pool with budget controls and multi-provider selection.
//!
//! Manages multiple LLM providers (Anthropic, OpenAI, Ollama) with:
//! - Priority-based selection
//! - Budget limits (daily, monthly, per-task)
//! - Rate limit tracking
//! - Usage accumulation and reporting

pub mod pool;

pub use pool::{
    estimate_cost_usd, AccountCredits, BudgetAlertDetail, BudgetAlertLevel, BudgetAlerts,
    BudgetConfig, BudgetInfo, DailyUsage, DetailedUsageSummary, ModelUsageEntry, ProviderEntry,
    ProviderKind, ProviderPool, ProviderPoolConfig, ProviderUsage, TaskUsageEntry, UsageRecord,
    UsageSummary,
};
