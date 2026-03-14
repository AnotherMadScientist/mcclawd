//! mcclawd-runner — agent runner binary + container runtime abstraction.
//!
//! This crate provides:
//! - `runtime` — the `ContainerRuntime` trait for pluggable container backends
//! - `docker` — Docker implementation via bollard
//! - `protocol` — JSONL protocol for agent ↔ host communication

pub mod docker;
pub mod protocol;
pub mod runtime;
