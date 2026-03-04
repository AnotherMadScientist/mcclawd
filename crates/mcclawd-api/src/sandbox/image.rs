//! Sandbox image builder — creates layered Docker images from base + skill install steps.

use bollard::image::BuildImageOptions;
use bollard::Docker;
use futures::StreamExt;
use mcclawd_core::skills::LoadedSkill;

/// Builds sandbox Docker images with skill layers.
pub struct ImageBuilder {
    docker: Docker,
}

impl ImageBuilder {
    pub fn new(docker: Docker) -> Self {
        Self { docker }
    }

    /// Build a sandbox image for the given skills.
    ///
    /// Image tag format: `mcclawd-sandbox:<hash>` where hash is derived
    /// from the sorted skill names + versions (for caching).
    ///
    /// Returns the image tag.
    pub async fn build_image(
        &self,
        base_image: &str,
        skills: &[LoadedSkill],
    ) -> anyhow::Result<String> {
        let tag = self.image_tag(base_image, skills);

        // Check if image already exists (cache hit)
        if self.image_exists(&tag).await {
            tracing::info!(tag = %tag, "sandbox image cache hit");
            return Ok(tag);
        }

        tracing::info!(tag = %tag, skills = skills.len(), "building sandbox image");

        let dockerfile = self.generate_dockerfile(base_image, skills);
        let tar = self.create_build_context(&dockerfile)?;

        let opts = BuildImageOptions {
            t: tag.as_str(),
            rm: true,
            ..Default::default()
        };

        let mut stream = self.docker.build_image(opts, None, Some(tar.into()));

        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    if let Some(stream) = info.stream {
                        tracing::debug!("{}", stream.trim());
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

    /// Generate a Dockerfile from base image + skill install steps.
    fn generate_dockerfile(&self, base_image: &str, skills: &[LoadedSkill]) -> String {
        let mut dockerfile = format!("FROM {base_image}\n\n");

        for skill in skills {
            if !skill.install_steps.is_empty() {
                dockerfile.push_str(&format!("# Skill: {} v{}\n", skill.name, skill.version));
                for step in &skill.install_steps {
                    dockerfile.push_str(&format!("RUN {step}\n"));
                }
                dockerfile.push('\n');
            }
        }

        dockerfile.push_str("WORKDIR /workspace\n");
        dockerfile
    }

    /// Create a tar archive containing just the Dockerfile.
    fn create_build_context(&self, dockerfile: &str) -> anyhow::Result<Vec<u8>> {
        let mut header = tar::Header::new_gnu();
        header.set_path("Dockerfile")?;
        header.set_size(dockerfile.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();

        let mut archive = tar::Builder::new(Vec::new());
        archive.append(&header, dockerfile.as_bytes())?;
        Ok(archive.into_inner()?)
    }

    /// Compute deterministic image tag from base + skills.
    fn image_tag(&self, base_image: &str, skills: &[LoadedSkill]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        base_image.hash(&mut hasher);
        let mut sorted: Vec<_> = skills.iter().map(|s| (&s.name, &s.version)).collect();
        sorted.sort();
        for (name, version) in sorted {
            name.hash(&mut hasher);
            version.hash(&mut hasher);
        }
        let hash = hasher.finish();
        format!("mcclawd-sandbox:{hash:x}")
    }

    /// Check if an image exists locally.
    async fn image_exists(&self, tag: &str) -> bool {
        self.docker.inspect_image(tag).await.is_ok()
    }
}
