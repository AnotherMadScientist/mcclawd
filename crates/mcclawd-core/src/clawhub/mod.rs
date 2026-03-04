//! ClawHub registry client, skill installer, and local catalog cache.
//!
//! Provides [`ClawHubClient`] for querying the ClawHub skill registry,
//! [`SkillInstaller`] for downloading, installing, upgrading, and
//! uninstalling skills, and [`ClawHubCache`] for local file-based
//! caching of the skill catalog.

pub mod cache;
pub mod client;
pub mod installer;

pub use cache::{CacheStats, CachedCatalog, CachedSearchResult, ClawHubCache};
pub use client::{ClawHubClient, ClawHubSearchResult, ClawHubSkillMeta};
pub use installer::{InstalledSkillInfo, SkillInstaller, SkillSource};
