//! McpPorter — builds on-demand Docker images for agent skill-sets and orchestrates
//! the full agent execution environment (network, image, gateway, tool filtering).
//!
//! Named after the MCPorter concept (TypeScript MCP bridge), adapted for McClawd's
//! Docker-first, Rust-native architecture.

use bollard::image::ListImagesOptions;
use bollard::Docker;
use mcclawd_core::config::McclawdConfig;
use mcclawd_core::skills::LoadedSkill;
use mcclawd_core::tool_resolver::{ResolvedToolSet, ToolResolver};
use std::collections::HashMap;

use super::mcp_lifecycle::McpLifecycleManager;
use crate::sandbox::AgentEnvironment;

/// Orchestrates Docker image builds, caching, and agent environment preparation.
pub struct McpPorter {
    docker: Docker,
    lifecycle: McpLifecycleManager,
}

impl McpPorter {
    pub fn new(lifecycle: McpLifecycleManager) -> anyhow::Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker, lifecycle })
    }

    /// Prepare the full agent environment for a task:
    ///
    /// 1. Ensure Docker network exists
    /// 2. Check image cache → build if miss
    /// 3. Ensure AgentGateway is on network
    /// 4. Return `AgentEnvironment` with all container config
    pub async fn prepare_environment(
        &self,
        tool_set: &ResolvedToolSet,
        config: &McclawdConfig,
    ) -> anyhow::Result<AgentEnvironment> {
        let network = &config.sandbox.network;

        // 1. Ensure Docker network exists
        self.lifecycle.ensure_network_exists(network).await?;

        // 2. Build or reuse cached image
        let image = self
            .build_or_cache_image(tool_set, &config.sandbox.base_image)
            .await?;

        // 3. Ensure AgentGateway container is on the network
        // AgentGateway container name follows docker-compose convention
        if let Err(e) = self
            .lifecycle
            .ensure_on_network("agentgateway", network)
            .await
        {
            tracing::warn!("Could not connect AgentGateway to network: {e}");
        }

        // 4. Ensure required MCP server containers are on the network
        for server_name in &tool_set.required_servers {
            let container_name = format!("mcclawd-mcp-{server_name}");
            if let Err(e) = self
                .lifecycle
                .ensure_on_network(&container_name, network)
                .await
            {
                tracing::warn!(server = server_name, "Could not connect MCP server to network: {e}");
            }
        }

        let allowed_tools: Vec<String> = tool_set.allowed_tools.iter().cloned().collect();

        Ok(AgentEnvironment {
            image,
            network: network.clone(),
            gateway_url: format!("http://agentgateway:3000"),
            allowed_tools,
            skill_context: tool_set.skill_context.clone(),
        })
    }

    /// Build agent Docker image from skill install_steps, or return cached tag.
    ///
    /// Cache key: image tag "mcclawd-agent:{hash}" where hash is computed from
    /// base_image + sorted install_steps.
    async fn build_or_cache_image(
        &self,
        tool_set: &ResolvedToolSet,
        base_image: &str,
    ) -> anyhow::Result<String> {
        let image_tag = format!("mcclawd-agent:{}", tool_set.image_hash);

        // Check if image already exists (cache hit)
        if self.image_exists(&image_tag).await? {
            tracing::info!(image = image_tag, "Agent image cache hit");
            return Ok(image_tag);
        }

        // Cache miss — build the image
        tracing::info!(
            image = image_tag,
            steps = tool_set.install_steps.len(),
            "Building agent image (cache miss)"
        );

        let dockerfile = Self::generate_dockerfile(base_image, &tool_set.install_steps);
        self.build_image(&image_tag, &dockerfile).await?;

        Ok(image_tag)
    }

    /// Check if a Docker image exists locally.
    async fn image_exists(&self, image_tag: &str) -> anyhow::Result<bool> {
        let mut filters = HashMap::new();
        filters.insert("reference".to_string(), vec![image_tag.to_string()]);

        let images = self
            .docker
            .list_images(Some(ListImagesOptions {
                filters,
                ..Default::default()
            }))
            .await?;

        Ok(!images.is_empty())
    }

    /// Generate a Dockerfile from base image + install steps.
    fn generate_dockerfile(base_image: &str, install_steps: &[String]) -> String {
        let mut lines = vec![
            format!("FROM {base_image}"),
            "WORKDIR /workspace".to_string(),
        ];

        for step in install_steps {
            lines.push(format!("RUN {step}"));
        }

        lines.push("ENTRYPOINT [\"mc\", \"run\"]".to_string());
        lines.join("\n")
    }

    /// Build a Docker image from a Dockerfile string.
    async fn build_image(&self, tag: &str, dockerfile: &str) -> anyhow::Result<()> {
        use bollard::image::BuildImageOptions;
        use futures::StreamExt;
        use std::io::Write;
        use tar::Builder as TarBuilder;

        // Create a tar archive containing just the Dockerfile
        let mut tar_buf = Vec::new();
        {
            let mut tar = TarBuilder::new(&mut tar_buf);
            let dockerfile_bytes = dockerfile.as_bytes();
            let mut header = tar::Header::new_gnu();
            header.set_path("Dockerfile")?;
            header.set_size(dockerfile_bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append(&header, dockerfile_bytes)?;
            tar.finish()?;
        }

        let build_opts = BuildImageOptions {
            t: tag.to_string(),
            rm: true,
            forcerm: true,
            ..Default::default()
        };

        let mut build_stream = self
            .docker
            .build_image(build_opts, None, Some(tar_buf.into()));

        while let Some(result) = build_stream.next().await {
            match result {
                Ok(info) => {
                    if let Some(stream) = &info.stream {
                        let trimmed = stream.trim();
                        if !trimmed.is_empty() {
                            tracing::debug!(image = tag, "{}", trimmed);
                        }
                    }
                    if let Some(error) = &info.error {
                        anyhow::bail!("Docker build error for {tag}: {error}");
                    }
                }
                Err(e) => {
                    anyhow::bail!("Docker build failed for {tag}: {e}");
                }
            }
        }

        tracing::info!(image = tag, "Agent image built successfully");
        Ok(())
    }

    /// Prepare a full-skill environment for the system agent.
    ///
    /// Resolves ALL installed skills into a single image.
    pub async fn prepare_system_agent_environment(
        &self,
        all_skills: &HashMap<String, LoadedSkill>,
        mcp_servers: &[mcclawd_core::config::McpServerConfig],
        config: &McclawdConfig,
    ) -> anyhow::Result<AgentEnvironment> {
        if all_skills.is_empty() {
            // No skills installed — use base image with all tools allowed
            let network = &config.sandbox.network;
            self.lifecycle.ensure_network_exists(network).await?;
            return Ok(AgentEnvironment {
                image: config.sandbox.base_image.clone(),
                network: network.clone(),
                gateway_url: "http://agentgateway:3000".to_string(),
                allowed_tools: vec!["*".to_string()],
                skill_context: String::new(),
            });
        }

        let skill_names: Vec<String> = all_skills.keys().cloned().collect();
        let tool_set = ToolResolver::resolve(&skill_names, all_skills, mcp_servers, &config.sandbox.base_image)?;
        self.prepare_environment(&tool_set, config).await
    }
}
