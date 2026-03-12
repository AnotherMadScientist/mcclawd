//! MCP server lifecycle management — Docker container operations for individual MCP servers.

use bollard::container::{
    Config, CreateContainerOptions, ListContainersOptions, RemoveContainerOptions,
    StopContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::HostConfig;
use bollard::network::ConnectNetworkOptions;
use bollard::Docker;
use futures::StreamExt;
use mcclawd_core::config::McpServerConfig;
use mcclawd_core::secrets::SecretBackend;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Status of an MCP server container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub container_id: Option<String>,
    pub running: bool,
    pub image: String,
    pub port: u16,
}

/// Manages Docker containers for individual MCP servers.
#[derive(Clone)]
pub struct McpLifecycleManager {
    docker: Docker,
    /// Shared secret backend for resolving `${SECRET_NAME}` tokens in MCP server env vars.
    /// Points to the same RwLock as AppState.secrets — automatically available once vault is unlocked.
    secrets: Arc<RwLock<Option<Arc<dyn SecretBackend>>>>,
}

impl McpLifecycleManager {
    pub fn new() -> anyhow::Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self {
            docker,
            secrets: Arc::new(RwLock::new(None)),
        })
    }

    /// Create with a shared secrets reference (same Arc as AppState.secrets).
    pub fn with_shared_secrets(
        secrets: Arc<RwLock<Option<Arc<dyn SecretBackend>>>>,
    ) -> anyhow::Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker, secrets })
    }

    /// Container naming convention for MCP servers.
    fn container_name(server_name: &str) -> String {
        format!("mcclawd-mcp-{server_name}")
    }

    /// Check if Docker is reachable.
    pub async fn health_check(&self) -> bool {
        self.docker.ping().await.is_ok()
    }

    /// Pull the image and start a new container for an MCP server.
    pub async fn start_server(&self, config: &McpServerConfig) -> anyhow::Result<String> {
        let container_name = Self::container_name(&config.name);

        // Pull image (best-effort — may already exist locally)
        let pull_opts = CreateImageOptions {
            from_image: config.image.clone(),
            ..Default::default()
        };
        let mut pull_stream = self.docker.create_image(Some(pull_opts), None, None);
        while let Some(result) = pull_stream.next().await {
            match result {
                Ok(info) => {
                    tracing::debug!(server = %config.name, ?info, "Pulling image");
                }
                Err(e) => {
                    tracing::warn!(server = %config.name, error = %e, "Image pull warning (may use local)");
                }
            }
        }

        // Build port binding: expose server port
        let port_binding = format!("{}/tcp", config.port);
        let mut port_bindings = std::collections::HashMap::new();
        port_bindings.insert(
            port_binding.clone(),
            Some(vec![bollard::models::PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(config.port.to_string()),
            }]),
        );

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            ..Default::default()
        };

        // Resolve ${SECRET_NAME} tokens in env vars if a secret backend is available
        let env: Vec<String> = {
            let guard = self.secrets.read().await;
            if let Some(ref backend) = *guard {
                mcclawd_core::secrets::resolve_secret_tokens(&config.env, backend.as_ref())
                    .await
                    .unwrap_or_else(|e| {
                        tracing::warn!(server = %config.name, error = %e, "Failed to resolve secret tokens in env — using raw values");
                        config.env.clone()
                    })
            } else {
                config.env.clone()
            }
        };

        let container_config = Config {
            image: Some(config.image.clone()),
            env: Some(env.iter().map(|s| s.clone()).collect()),
            host_config: Some(host_config),
            ..Default::default()
        };

        let create_opts = CreateContainerOptions {
            name: container_name.clone(),
            ..Default::default()
        };

        let response = self
            .docker
            .create_container(Some(create_opts), container_config)
            .await?;

        self.docker
            .start_container::<&str>(&response.id, None)
            .await?;

        tracing::info!(
            server = %config.name,
            container_id = %response.id,
            "MCP server container started"
        );

        Ok(response.id)
    }

    /// Stop an MCP server container.
    pub async fn stop_server(&self, name: &str) -> anyhow::Result<()> {
        let container_name = Self::container_name(name);
        self.docker
            .stop_container(
                &container_name,
                Some(StopContainerOptions { t: 10 }),
            )
            .await?;
        tracing::info!(server = %name, "MCP server container stopped");
        Ok(())
    }

    /// Remove an MCP server container (must be stopped first, or use force).
    pub async fn remove_server(&self, name: &str) -> anyhow::Result<()> {
        let container_name = Self::container_name(name);
        self.docker
            .remove_container(
                &container_name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await?;
        tracing::info!(server = %name, "MCP server container removed");
        Ok(())
    }

    /// Restart an MCP server container (stop + start).
    pub async fn restart_server(&self, config: &McpServerConfig) -> anyhow::Result<()> {
        let container_name = Self::container_name(&config.name);

        // Stop (ignore error if not running)
        let _ = self
            .docker
            .stop_container(&container_name, Some(StopContainerOptions { t: 10 }))
            .await;

        // Remove old container (ignore error if not found)
        let _ = self
            .docker
            .remove_container(
                &container_name,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await;

        // Start fresh
        self.start_server(config).await?;
        Ok(())
    }

    /// Ensure a Docker network exists, creating it if needed.
    pub async fn ensure_network_exists(&self, network: &str) -> anyhow::Result<()> {
        use bollard::network::CreateNetworkOptions;

        match self.docker.inspect_network::<&str>(network, None).await {
            Ok(_) => {
                tracing::debug!(network, "Docker network already exists");
                Ok(())
            }
            Err(_) => {
                let opts = CreateNetworkOptions {
                    name: network.to_string(),
                    driver: "bridge".to_string(),
                    ..Default::default()
                };
                self.docker.create_network(opts).await?;
                tracing::info!(network, "Created Docker network");
                Ok(())
            }
        }
    }

    /// Ensure a container is connected to a Docker network. Idempotent.
    pub async fn ensure_on_network(
        &self,
        container_name: &str,
        network: &str,
    ) -> anyhow::Result<()> {
        let opts = ConnectNetworkOptions {
            container: container_name.to_string(),
            ..Default::default()
        };
        match self.docker.connect_network(network, opts).await {
            Ok(()) => {
                tracing::info!(container = container_name, network, "Connected container to network");
                Ok(())
            }
            Err(e) => {
                // "already connected" is not an error
                let msg = e.to_string();
                if msg.contains("already") || msg.contains("endpoint with name") {
                    tracing::debug!(container = container_name, network, "Container already on network");
                    Ok(())
                } else {
                    Err(e.into())
                }
            }
        }
    }

    /// Get a reference to the underlying Docker client.
    pub fn docker(&self) -> &Docker {
        &self.docker
    }

    /// Get the status of an MCP server container.
    pub async fn server_status(&self, name: &str, config: &McpServerConfig) -> McpServerStatus {
        let container_name = Self::container_name(name);

        let mut filters = std::collections::HashMap::new();
        filters.insert("name".to_string(), vec![container_name]);

        let containers = self
            .docker
            .list_containers(Some(ListContainersOptions {
                all: true,
                filters,
                ..Default::default()
            }))
            .await
            .unwrap_or_default();

        if let Some(container) = containers.first() {
            let running = container
                .state
                .as_deref()
                .map(|s| s == "running")
                .unwrap_or(false);
            McpServerStatus {
                name: name.to_string(),
                container_id: container.id.clone(),
                running,
                image: config.image.clone(),
                port: config.port,
            }
        } else {
            McpServerStatus {
                name: name.to_string(),
                container_id: None,
                running: false,
                image: config.image.clone(),
                port: config.port,
            }
        }
    }
}
