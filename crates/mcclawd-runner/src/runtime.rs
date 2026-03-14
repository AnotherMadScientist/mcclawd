//! ContainerRuntime trait — runtime-agnostic abstraction over container engines.
//!
//! Docker is the Phase 1 implementation. Firecracker and WASM backends can be
//! added later by implementing this trait.

use async_trait::async_trait;
use std::collections::HashMap;

/// Runtime-agnostic handle to a running container.
///
/// Returned by `ContainerRuntime::start` and consumed by `stop` / `health`.
/// Backends store whatever they need (container ID, VM ID, process handle, etc.)
/// inside the opaque `id` field plus optional `metadata`.
#[derive(Debug, Clone)]
pub struct ContainerHandle {
    /// Opaque identifier for the running container/VM/process.
    /// For Docker this is the container ID, for Firecracker the VM ID, etc.
    pub id: String,

    /// Human-readable name (e.g. "mcclawd-runner-abc123").
    pub name: String,

    /// Arbitrary key-value metadata the backend may attach.
    /// Docker uses this for network info, port mappings, etc.
    pub metadata: HashMap<String, String>,
}

/// Configuration passed to `ContainerRuntime::start`.
///
/// This is a runtime-agnostic subset of resource limits and mount points.
/// Each backend maps these to its own configuration format.
#[derive(Debug, Clone)]
pub struct StartConfig {
    /// Container/VM name.
    pub name: String,

    /// Environment variables to set inside the container.
    pub env: Vec<String>,

    /// Bind mounts: (host_path, container_path, read_only).
    pub mounts: Vec<MountSpec>,

    /// Memory limit in bytes. `None` means no limit.
    pub memory_limit: Option<i64>,

    /// CPU limit in nano-CPUs (Docker convention). `None` means no limit.
    pub cpu_limit: Option<i64>,

    /// Max PIDs. `None` means no limit.
    pub pids_limit: Option<i64>,

    /// Network name/mode (e.g. "mcclawd_default", "host", "none").
    pub network: String,

    /// Working directory inside the container.
    pub working_dir: String,

    /// Extra security options (e.g. "no-new-privileges").
    pub security_opts: Vec<String>,
}

/// A single mount specification.
#[derive(Debug, Clone)]
pub struct MountSpec {
    /// Path on the host (or empty for tmpfs).
    pub source: String,

    /// Path inside the container.
    pub target: String,

    /// Whether the mount is read-only.
    pub read_only: bool,

    /// Mount type.
    pub mount_type: MountType,
}

/// Supported mount types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountType {
    /// Bind-mount a host directory.
    Bind,
    /// In-memory tmpfs.
    Tmpfs,
}

/// Trait abstracting over container runtimes (Docker, Firecracker, WASM, etc.).
///
/// Each method is async and returns `anyhow::Result` for uniform error handling.
/// Implementors must be `Send + Sync` so they can be shared across tasks.
#[async_trait]
pub trait ContainerRuntime: Send + Sync {
    /// Build an image from a base image and a list of shell commands (install steps).
    ///
    /// `hash` is a pre-computed cache key — if an image with this hash already exists,
    /// the build can be skipped. Returns the image identifier (e.g. tag or digest).
    async fn build(&self, base: &str, steps: &[String], hash: &str) -> anyhow::Result<String>;

    /// Start a container from the given image with the provided configuration.
    ///
    /// Returns a `ContainerHandle` that can be used to stop, query, or clean up.
    async fn start(&self, image_id: &str, config: &StartConfig) -> anyhow::Result<ContainerHandle>;

    /// Stop and remove a running container.
    async fn stop(&self, handle: &ContainerHandle) -> anyhow::Result<()>;

    /// Check if the container is still running/healthy.
    async fn health(&self, handle: &ContainerHandle) -> anyhow::Result<bool>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_handle_debug() {
        let handle = ContainerHandle {
            id: "abc123".to_string(),
            name: "test-container".to_string(),
            metadata: HashMap::new(),
        };
        let debug = format!("{handle:?}");
        assert!(debug.contains("abc123"));
        assert!(debug.contains("test-container"));
    }

    #[test]
    fn mount_type_eq() {
        assert_eq!(MountType::Bind, MountType::Bind);
        assert_eq!(MountType::Tmpfs, MountType::Tmpfs);
        assert_ne!(MountType::Bind, MountType::Tmpfs);
    }

    #[test]
    fn start_config_defaults() {
        let config = StartConfig {
            name: "test".to_string(),
            env: vec!["FOO=bar".to_string()],
            mounts: vec![],
            memory_limit: Some(512 * 1024 * 1024),
            cpu_limit: None,
            pids_limit: Some(256),
            network: "bridge".to_string(),
            working_dir: "/workspace".to_string(),
            security_opts: vec!["no-new-privileges".to_string()],
        };
        assert_eq!(config.name, "test");
        assert_eq!(config.memory_limit, Some(512 * 1024 * 1024));
        assert!(config.cpu_limit.is_none());
    }
}
