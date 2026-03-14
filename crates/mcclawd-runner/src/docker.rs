//! Docker implementation of the `ContainerRuntime` trait.
//!
//! Wraps the `bollard` Docker client to provide container lifecycle management.
//! Extracted from `mcclawd-api/src/sandbox/container.rs` to sit behind the
//! runtime-agnostic `ContainerRuntime` trait.

use async_trait::async_trait;
use bollard::container::{Config, CreateContainerOptions, RemoveContainerOptions, StopContainerOptions};
use bollard::image::BuildImageOptions;
use bollard::models::{HostConfig, Mount, MountTypeEnum};
use bollard::Docker;
use futures::StreamExt;
use std::collections::HashMap;

use crate::runtime::{ContainerHandle, ContainerRuntime, MountSpec, MountType, StartConfig};

/// Docker-based container runtime using the local Docker daemon.
pub struct DockerRuntime {
    docker: Docker,
}

impl DockerRuntime {
    /// Connect to the local Docker daemon using default settings.
    pub fn new() -> anyhow::Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(Self { docker })
    }

    /// Create from an existing bollard Docker client.
    pub fn from_client(docker: Docker) -> Self {
        Self { docker }
    }

    /// Check if a Docker image exists locally.
    async fn image_exists(&self, tag: &str) -> bool {
        self.docker.inspect_image(tag).await.is_ok()
    }

    /// Create a tar archive containing a Dockerfile for `docker build`.
    fn create_build_context(dockerfile: &str) -> anyhow::Result<Vec<u8>> {
        let mut header = tar::Header::new_gnu();
        header.set_path("Dockerfile")?;
        header.set_size(dockerfile.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();

        let mut archive = tar::Builder::new(Vec::new());
        archive.append(&header, dockerfile.as_bytes())?;
        Ok(archive.into_inner()?)
    }

    /// Convert our runtime-agnostic `MountSpec` list into bollard `Mount` objects.
    fn to_bollard_mounts(mounts: &[MountSpec]) -> Vec<Mount> {
        mounts
            .iter()
            .map(|m| {
                let typ = match m.mount_type {
                    MountType::Bind => Some(MountTypeEnum::BIND),
                    MountType::Tmpfs => Some(MountTypeEnum::TMPFS),
                };
                let source = if m.source.is_empty() {
                    None
                } else {
                    Some(m.source.clone())
                };
                Mount {
                    target: Some(m.target.clone()),
                    source,
                    typ,
                    read_only: Some(m.read_only),
                    ..Default::default()
                }
            })
            .collect()
    }
}

#[async_trait]
impl ContainerRuntime for DockerRuntime {
    async fn build(&self, base: &str, steps: &[String], hash: &str) -> anyhow::Result<String> {
        let tag = format!("mcclawd-sandbox:{hash}");

        // Cache hit — skip build
        if self.image_exists(&tag).await {
            tracing::info!(tag = %tag, "Docker image cache hit");
            return Ok(tag);
        }

        tracing::info!(tag = %tag, steps = steps.len(), "Building Docker image");

        // Generate Dockerfile
        let mut dockerfile = format!("FROM {base}\n\n");
        for step in steps {
            dockerfile.push_str(&format!("RUN {step}\n"));
        }
        dockerfile.push_str("WORKDIR /workspace\n");

        let tar = Self::create_build_context(&dockerfile)?;

        let opts = BuildImageOptions {
            t: tag.as_str(),
            rm: true,
            ..Default::default()
        };

        let mut stream = self.docker.build_image(opts, None, Some(tar.into()));

        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    if let Some(stream_msg) = info.stream {
                        tracing::debug!("{}", stream_msg.trim());
                    }
                    if let Some(error) = info.error {
                        anyhow::bail!("Docker build error: {error}");
                    }
                }
                Err(e) => anyhow::bail!("Docker build failed: {e}"),
            }
        }

        Ok(tag)
    }

    async fn start(&self, image_id: &str, config: &StartConfig) -> anyhow::Result<ContainerHandle> {
        let mounts = Self::to_bollard_mounts(&config.mounts);

        let host_config = HostConfig {
            mounts: if mounts.is_empty() { None } else { Some(mounts) },
            memory: config.memory_limit,
            nano_cpus: config.cpu_limit,
            network_mode: Some(config.network.clone()),
            security_opt: if config.security_opts.is_empty() {
                None
            } else {
                Some(config.security_opts.clone())
            },
            pids_limit: config.pids_limit,
            ..Default::default()
        };

        let container_config = Config {
            image: Some(image_id.to_string()),
            env: if config.env.is_empty() {
                None
            } else {
                Some(config.env.clone())
            },
            host_config: Some(host_config),
            working_dir: Some(config.working_dir.clone()),
            ..Default::default()
        };

        let create_opts = CreateContainerOptions {
            name: config.name.clone(),
            platform: None,
        };

        let response = self
            .docker
            .create_container(Some(create_opts), container_config)
            .await?;

        self.docker
            .start_container::<String>(&response.id, None)
            .await?;

        tracing::info!(
            container_id = %response.id,
            name = %config.name,
            image = %image_id,
            "Docker container started"
        );

        Ok(ContainerHandle {
            id: response.id,
            name: config.name.clone(),
            metadata: HashMap::new(),
        })
    }

    async fn stop(&self, handle: &ContainerHandle) -> anyhow::Result<()> {
        // Graceful stop with 5-second timeout, then force remove
        let _ = self
            .docker
            .stop_container(&handle.id, Some(StopContainerOptions { t: 5 }))
            .await;

        self.docker
            .remove_container(
                &handle.id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await?;

        tracing::info!(
            container_id = %handle.id,
            name = %handle.name,
            "Docker container stopped and removed"
        );

        Ok(())
    }

    async fn health(&self, handle: &ContainerHandle) -> anyhow::Result<bool> {
        match self.docker.inspect_container(&handle.id, None).await {
            Ok(info) => {
                let running = info
                    .state
                    .and_then(|s| s.running)
                    .unwrap_or(false);
                Ok(running)
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::MountType;

    #[test]
    fn bollard_mount_bind() {
        let specs = vec![MountSpec {
            source: "/host/path".to_string(),
            target: "/container/path".to_string(),
            read_only: false,
            mount_type: MountType::Bind,
        }];
        let mounts = DockerRuntime::to_bollard_mounts(&specs);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].target.as_deref(), Some("/container/path"));
        assert_eq!(mounts[0].source.as_deref(), Some("/host/path"));
        assert_eq!(mounts[0].read_only, Some(false));
        assert_eq!(mounts[0].typ, Some(MountTypeEnum::BIND));
    }

    #[test]
    fn bollard_mount_tmpfs() {
        let specs = vec![MountSpec {
            source: String::new(),
            target: "/run/secrets".to_string(),
            read_only: false,
            mount_type: MountType::Tmpfs,
        }];
        let mounts = DockerRuntime::to_bollard_mounts(&specs);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].typ, Some(MountTypeEnum::TMPFS));
        assert!(mounts[0].source.is_none());
    }

    #[test]
    fn bollard_mounts_mixed() {
        let specs = vec![
            MountSpec {
                source: "/workspace".to_string(),
                target: "/workspace".to_string(),
                read_only: false,
                mount_type: MountType::Bind,
            },
            MountSpec {
                source: String::new(),
                target: "/run/secrets".to_string(),
                read_only: false,
                mount_type: MountType::Tmpfs,
            },
            MountSpec {
                source: "/host/attachments".to_string(),
                target: "/attachments".to_string(),
                read_only: true,
                mount_type: MountType::Bind,
            },
        ];
        let mounts = DockerRuntime::to_bollard_mounts(&specs);
        assert_eq!(mounts.len(), 3);
        assert_eq!(mounts[2].read_only, Some(true));
    }

    #[test]
    fn build_context_creates_valid_tar() {
        let dockerfile = "FROM ubuntu:22.04\nRUN apt-get update\n";
        let tar_bytes = DockerRuntime::create_build_context(dockerfile).unwrap();
        assert!(!tar_bytes.is_empty());

        // Verify we can read the tar archive
        let mut archive = tar::Archive::new(&tar_bytes[..]);
        let entries: Vec<_> = archive.entries().unwrap().collect();
        assert_eq!(entries.len(), 1);
    }
}
