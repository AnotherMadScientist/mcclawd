//! Provider pool with budget controls and multi-provider selection.
//!
//! Manages multiple LLM providers (Anthropic, OpenAI, Ollama) with:
//! - Priority-based selection
//! - Budget limits (daily, monthly, per-task)
//! - Rate limit tracking
//! - Usage accumulation and reporting

pub mod pool;

pub use pool::{
    BudgetConfig, ProviderEntry, ProviderKind, ProviderPool, ProviderPoolConfig, ProviderUsage,
    UsageRecord, UsageSummary,
};
