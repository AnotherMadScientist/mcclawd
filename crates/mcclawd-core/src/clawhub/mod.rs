//! ClawHub registry client and skill installer.
//!
//! Provides [`ClawHubClient`] for querying the ClawHub skill registry
//! and [`SkillInstaller`] for downloading, installing, upgrading, and
//! uninstalling skills.

pub mod client;
pub mod installer;

pub use client::{ClawHubClient, ClawHubSearchResult, ClawHubSkillMeta};
pub use installer::{InstalledSkillInfo, SkillInstaller, SkillSource};
