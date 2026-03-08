//! Skill downloader and installer — handles both local and registry installs.

use super::client::{ClawHubClient, ClawHubSkillMeta};
use super::dep_resolver::DepResolver;
use crate::skill_parser::parse_skill_md;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A skill that has an update available in the registry (Gap 5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUpdate {
    pub name: String,
    pub installed_version: String,
    pub latest_version: String,
}

/// Metadata about an installed skill (written to .installed.json in skill dir).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSkillInfo {
    pub name: String,
    pub version: String,
    pub source: SkillSource,
    pub installed_at: DateTime<Utc>,
}

/// Where a skill was installed from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SkillSource {
    Local(PathBuf),
    Registry { registry_url: String },
}

/// Installs, upgrades, and manages skills on disk.
pub struct SkillInstaller {
    client: ClawHubClient,
    skills_dir: PathBuf,
    /// Version pins: skill name -> pinned version string. Pinned skills skip update checks.
    pinned_versions: HashMap<String, String>,
}

impl SkillInstaller {
    /// Create a new installer with a ClawHub client and a local skills directory.
    pub fn new(client: ClawHubClient, skills_dir: PathBuf) -> Self {
        Self {
            client,
            skills_dir,
            pinned_versions: HashMap::new(),
        }
    }

    /// Set version pins from config (Gap 5).
    pub fn with_pinned_versions(mut self, pins: HashMap<String, String>) -> Self {
        self.pinned_versions = pins;
        self
    }

    /// Install a skill from the registry.
    /// Downloads tar.gz, extracts to skills_dir/{name}/, verifies SKILL.md, writes .installed.json.
    pub async fn install_from_registry(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> anyhow::Result<InstalledSkillInfo> {
        // Get skill metadata (resolves latest version if none specified)
        let meta = self.client.get_skill(name, version).await?;
        let dest = self.skills_dir.join(&meta.name);

        // Check if already installed WITH a full SKILL.md (not a stub).
        // If the existing SKILL.md is a stub (< 500 bytes or no `## ` sections),
        // re-download to get the full content.
        if dest.exists() {
            let skill_md_path = dest.join("SKILL.md");
            let is_stub = if skill_md_path.exists() {
                let content = std::fs::read_to_string(&skill_md_path).unwrap_or_default();
                content.len() < 500 || !content.contains("## ")
            } else {
                true // missing SKILL.md = treat as stub
            };
            if !is_stub {
                if let Ok(Some(existing)) = self.read_installed_info(&meta.name) {
                    return Ok(existing);
                }
            }
            // Stub or missing — remove and re-download
            let _ = std::fs::remove_dir_all(&dest);
        }

        // Download and extract
        let bytes = self
            .client
            .download_skill(&meta.name, &meta.version)
            .await?;
        self.extract_package(&bytes, &dest)?;

        // Verify SKILL.md exists and parses
        let skill_md = dest.join("SKILL.md");
        if !skill_md.exists() {
            // Clean up on failure
            let _ = std::fs::remove_dir_all(&dest);
            anyhow::bail!(
                "Downloaded skill '{}' does not contain a SKILL.md",
                meta.name
            );
        }
        let content = std::fs::read_to_string(&skill_md)?;
        let _ = parse_skill_md(&content).map_err(|e| {
            let _ = std::fs::remove_dir_all(&dest);
            anyhow::anyhow!("Invalid SKILL.md in downloaded package: {e}")
        })?;

        // Write .installed.json
        let info = InstalledSkillInfo {
            name: meta.name.clone(),
            version: meta.version.clone(),
            source: SkillSource::Registry {
                registry_url: self.client.base_url().to_string(),
            },
            installed_at: Utc::now(),
        };
        self.write_installed_info(&meta.name, &info)?;

        Ok(info)
    }

    /// Install a skill from cached metadata (no download — generates a stub SKILL.md).
    /// Used as fallback when the real registry is unreachable.
    pub fn install_from_meta(&self, meta: &ClawHubSkillMeta) -> anyhow::Result<InstalledSkillInfo> {
        let dest = self.skills_dir.join(&meta.name);
        // Idempotent: return existing install info if already present
        if dest.exists() {
            if let Ok(Some(existing)) = self.read_installed_info(&meta.name) {
                return Ok(existing);
            }
        }

        std::fs::create_dir_all(&dest)?;

        // Generate a minimal SKILL.md
        let tags_line = if meta.tags.is_empty() {
            String::new()
        } else {
            format!("tags: {}\n", meta.tags.join(", "))
        };
        let skill_md = format!(
            "---\nname: {}\nversion: {}\nauthor: {}\n{}---\n\n# {}\n\n{}\n",
            meta.name, meta.version, meta.author, tags_line, meta.name, meta.description
        );
        std::fs::write(dest.join("SKILL.md"), &skill_md)?;

        let info = InstalledSkillInfo {
            name: meta.name.clone(),
            version: meta.version.clone(),
            source: SkillSource::Registry {
                registry_url: self.client.base_url().to_string(),
            },
            installed_at: Utc::now(),
        };
        self.write_installed_info(&meta.name, &info)?;

        Ok(info)
    }

    /// Install a skill from a local path (enhanced version with .installed.json tracking).
    pub fn install_from_local(&self, source: &Path) -> anyhow::Result<InstalledSkillInfo> {
        let skill_md = source.join("SKILL.md");
        if !skill_md.exists() {
            anyhow::bail!("No SKILL.md found at {}", skill_md.display());
        }

        let content = std::fs::read_to_string(&skill_md)?;
        let skill = parse_skill_md(&content)?;

        let dest = self.skills_dir.join(&skill.name);
        if dest.exists() {
            anyhow::bail!(
                "Skill '{}' already installed at {}. Remove first.",
                skill.name,
                dest.display()
            );
        }

        copy_dir_all(source, &dest)?;

        let info = InstalledSkillInfo {
            name: skill.name.clone(),
            version: skill.version.clone(),
            source: SkillSource::Local(source.to_path_buf()),
            installed_at: Utc::now(),
        };
        self.write_installed_info(&skill.name, &info)?;

        Ok(info)
    }

    /// Check all installed skills for available updates (Gap 5).
    /// Skips pinned skills. Returns only skills where a newer version exists.
    pub async fn check_for_updates(&self) -> anyhow::Result<Vec<SkillUpdate>> {
        let installed = self.list_installed()?;
        let mut updates = Vec::new();

        for skill in installed {
            // Pinned skills are intentionally frozen
            if self.pinned_versions.contains_key(&skill.name) {
                continue;
            }
            match self.client.get_skill(&skill.name, None).await {
                Ok(latest) => {
                    if self.version_is_newer(&latest.version, &skill.version) {
                        updates.push(SkillUpdate {
                            name: skill.name,
                            installed_version: skill.version,
                            latest_version: latest.version,
                        });
                    }
                }
                Err(_) => {
                    // Skip skills that can't be checked (registry down, renamed, etc.)
                }
            }
        }

        Ok(updates)
    }

    /// Install a skill and all its dependencies in topological order (Gap 4).
    ///
    /// Downloads SKILL.md for each skill to inspect `## Dependencies`, builds the
    /// dependency graph, detects cycles, then installs leaves-first.
    /// Already-installed skills are skipped.
    pub async fn install_with_deps(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> anyhow::Result<Vec<InstalledSkillInfo>> {
        // Resolve dep graph recursively then install in topo order
        let mut dep_graph: HashMap<String, Vec<String>> = HashMap::new();
        let mut visited: HashSet<String> = HashSet::new();
        self.collect_deps(name, version, &mut dep_graph, &mut visited)
            .await?;

        let install_order = DepResolver::resolve_order(&dep_graph)?;

        let mut installed = Vec::new();
        for skill_name in &install_order {
            // Skip already-installed skills
            if self.read_installed_info(skill_name)?.is_some() {
                tracing::debug!("Skipping already-installed dependency: {skill_name}");
                continue;
            }
            let pinned = self.pinned_versions.get(skill_name).map(|s| s.as_str());
            let info = self.install_from_registry(skill_name, pinned).await?;
            tracing::info!("Installed dependency: {} v{}", info.name, info.version);
            installed.push(info);
        }

        Ok(installed)
    }

    /// Recursively collect the dependency graph for a skill by downloading its SKILL.md.
    async fn collect_deps(
        &self,
        name: &str,
        version: Option<&str>,
        graph: &mut HashMap<String, Vec<String>>,
        visited: &mut HashSet<String>,
    ) -> anyhow::Result<()> {
        if !visited.insert(name.to_string()) {
            return Ok(()); // Already processed
        }

        // Try to read SKILL.md from disk (already installed) first
        let deps = if let Some(info) = self.read_installed_info(name)? {
            let skill_md = self.skills_dir.join(&info.name).join("SKILL.md");
            if let Ok(content) = std::fs::read_to_string(&skill_md) {
                parse_skill_md(&content).map(|s| s.dependencies).unwrap_or_default()
            } else {
                vec![]
            }
        } else {
            // Download SKILL.md to inspect deps
            match self.client.download_skill_md(name, version.unwrap_or("latest")).await {
                Ok(content) => parse_skill_md(&content)
                    .map(|s| s.dependencies)
                    .unwrap_or_default(),
                Err(e) => {
                    tracing::warn!("Could not fetch SKILL.md for '{name}' to resolve deps: {e}");
                    vec![]
                }
            }
        };

        graph.insert(name.to_string(), deps.clone());

        // Recurse into dependencies
        for dep in deps {
            let dep_version = self.pinned_versions.get(&dep).map(|s| s.as_str());
            Box::pin(self.collect_deps(&dep, dep_version, graph, visited)).await?;
        }

        Ok(())
    }

    /// Compare two version strings. Returns true if `latest` is newer than `installed`.
    /// Tries semver first, falls back to plain string inequality.
    fn version_is_newer(&self, latest: &str, installed: &str) -> bool {
        use semver::Version;
        match (Version::parse(latest), Version::parse(installed)) {
            (Ok(l), Ok(i)) => l > i,
            _ => latest != installed,
        }
    }

    /// Check if a newer version is available.
    pub async fn check_upgrade(
        &self,
        name: &str,
    ) -> anyhow::Result<Option<ClawHubSkillMeta>> {
        let installed = self
            .read_installed_info(name)?
            .ok_or_else(|| anyhow::anyhow!("Skill '{}' is not installed", name))?;

        let latest = self.client.get_skill(name, None).await?;

        if latest.version != installed.version {
            Ok(Some(latest))
        } else {
            Ok(None)
        }
    }

    /// Upgrade a skill to the latest version.
    pub async fn upgrade(&self, name: &str) -> anyhow::Result<InstalledSkillInfo> {
        // Check we have a newer version
        let latest = self
            .check_upgrade(name)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Skill '{}' is already at the latest version", name))?;

        // Remove old version
        self.uninstall(name)?;

        // Install new version
        self.install_from_registry(&latest.name, Some(&latest.version))
            .await
    }

    /// Uninstall a skill by removing its directory.
    pub fn uninstall(&self, name: &str) -> anyhow::Result<()> {
        let dir = self.skills_dir.join(name);
        if !dir.exists() {
            anyhow::bail!("Skill '{}' is not installed", name);
        }
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    /// List all installed skills with their metadata.
    pub fn list_installed(&self) -> anyhow::Result<Vec<InstalledSkillInfo>> {
        if !self.skills_dir.exists() {
            return Ok(Vec::new());
        }

        let mut installed = Vec::new();
        for entry in std::fs::read_dir(&self.skills_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(info) = self.read_installed_info(&name)? {
                    installed.push(info);
                }
            }
        }
        Ok(installed)
    }

    /// Read .installed.json from a skill directory.
    pub fn read_installed_info(&self, name: &str) -> anyhow::Result<Option<InstalledSkillInfo>> {
        let path = self.skills_dir.join(name).join(".installed.json");
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&path)?;
        let info: InstalledSkillInfo = serde_json::from_str(&content)?;
        Ok(Some(info))
    }

    /// Write .installed.json to a skill directory.
    fn write_installed_info(&self, name: &str, info: &InstalledSkillInfo) -> anyhow::Result<()> {
        let path = self.skills_dir.join(name).join(".installed.json");
        let json = serde_json::to_string_pretty(info)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Extract a skill package (ZIP or tar.gz) into a destination directory.
    /// Uses a temp directory for atomic extraction — if extraction fails,
    /// the destination is not left in an inconsistent state.
    fn extract_package(&self, bytes: &[u8], dest: &Path) -> anyhow::Result<()> {
        // Extract to a temp dir first, then rename on success
        let tmp_dest = dest.with_extension("_extracting");
        if tmp_dest.exists() {
            std::fs::remove_dir_all(&tmp_dest)?;
        }
        std::fs::create_dir_all(&tmp_dest)?;

        let result = self.extract_to_dir(bytes, &tmp_dest);
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&tmp_dest);
            return result;
        }

        // Atomic move: remove existing dest if any, rename temp to dest
        if dest.exists() {
            std::fs::remove_dir_all(dest)?;
        }
        std::fs::rename(&tmp_dest, dest)?;
        Ok(())
    }

    /// Inner extraction logic — extracts ZIP or tar.gz into the given directory.
    fn extract_to_dir(&self, bytes: &[u8], dest: &Path) -> anyhow::Result<()> {
        // Try ZIP first (ClawHub serves ZIP packages)
        let cursor = std::io::Cursor::new(bytes);
        if let Ok(mut archive) = zip::ZipArchive::new(cursor) {
            for i in 0..archive.len() {
                let mut file = archive.by_index(i)?;
                let outpath = dest.join(file.mangled_name());
                if file.is_dir() {
                    std::fs::create_dir_all(&outpath)?;
                } else {
                    if let Some(parent) = outpath.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    let mut outfile = std::fs::File::create(&outpath)?;
                    std::io::copy(&mut file, &mut outfile)?;
                }
            }
            return Ok(());
        }

        // Fall back to tar.gz
        let decoder = flate2::read::GzDecoder::new(bytes);
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(dest)?;
        Ok(())
    }

    /// Get the skills directory this installer uses.
    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }
}

/// Recursively copy a directory.
fn copy_dir_all(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sample_skill_md() -> &'static str {
        "# Skill: test-skill\nversion: 1.0.0\nauthor: testauthor\n\n## Description\nA test skill.\n\n## MCP Tools\n- test_tool\n\n## Context\nTest context.\n"
    }

    fn create_skill_dir(base: &Path, name: &str) -> PathBuf {
        let dir = base.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), sample_skill_md()).unwrap();
        dir
    }

    #[test]
    fn test_installed_info_serde_roundtrip() {
        let info = InstalledSkillInfo {
            name: "code-review".to_string(),
            version: "1.0.0".to_string(),
            source: SkillSource::Registry {
                registry_url: "https://api.clawhub.com".to_string(),
            },
            installed_at: Utc::now(),
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: InstalledSkillInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, "code-review");
        assert_eq!(parsed.version, "1.0.0");
        match &parsed.source {
            SkillSource::Registry { registry_url } => {
                assert_eq!(registry_url, "https://api.clawhub.com");
            }
            _ => panic!("Expected Registry source"),
        }
    }

    #[test]
    fn test_installed_info_local_source_roundtrip() {
        let info = InstalledSkillInfo {
            name: "local-skill".to_string(),
            version: "0.1.0".to_string(),
            source: SkillSource::Local(PathBuf::from("/tmp/skills/local-skill")),
            installed_at: Utc::now(),
        };

        let json = serde_json::to_string(&info).unwrap();
        let parsed: InstalledSkillInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, "local-skill");
        match &parsed.source {
            SkillSource::Local(p) => assert_eq!(p, &PathBuf::from("/tmp/skills/local-skill")),
            _ => panic!("Expected Local source"),
        }
    }

    #[test]
    fn test_install_from_local() {
        let tmp = tempfile::tempdir().unwrap();
        let source_dir = tmp.path().join("source-skill");
        create_skill_dir(tmp.path(), "source-skill");

        let skills_dir = tmp.path().join("installed");
        fs::create_dir_all(&skills_dir).unwrap();

        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, skills_dir.clone());

        let result = installer.install_from_local(&source_dir).unwrap();
        assert_eq!(result.name, "test-skill");
        assert_eq!(result.version, "1.0.0");

        // Verify SKILL.md was copied
        assert!(skills_dir.join("test-skill").join("SKILL.md").exists());

        // Verify .installed.json was written
        assert!(skills_dir
            .join("test-skill")
            .join(".installed.json")
            .exists());
    }

    #[test]
    fn test_install_from_local_writes_installed_json() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(tmp.path(), "source-skill");

        let skills_dir = tmp.path().join("installed");
        fs::create_dir_all(&skills_dir).unwrap();

        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, skills_dir.clone());

        installer
            .install_from_local(&tmp.path().join("source-skill"))
            .unwrap();

        let info = installer.read_installed_info("test-skill").unwrap();
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.name, "test-skill");
        assert_eq!(info.version, "1.0.0");
        match &info.source {
            SkillSource::Local(_) => {}
            _ => panic!("Expected Local source"),
        }
    }

    #[test]
    fn test_install_from_local_rejects_duplicate() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(tmp.path(), "source-skill");

        let skills_dir = tmp.path().join("installed");
        fs::create_dir_all(&skills_dir).unwrap();

        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, skills_dir);

        installer
            .install_from_local(&tmp.path().join("source-skill"))
            .unwrap();

        // Second install should fail
        let result = installer.install_from_local(&tmp.path().join("source-skill"));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already installed"));
    }

    #[test]
    fn test_install_from_local_requires_skill_md() {
        let tmp = tempfile::tempdir().unwrap();
        let empty_dir = tmp.path().join("empty-skill");
        fs::create_dir_all(&empty_dir).unwrap();

        let skills_dir = tmp.path().join("installed");
        fs::create_dir_all(&skills_dir).unwrap();

        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, skills_dir);

        let result = installer.install_from_local(&empty_dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("SKILL.md"));
    }

    #[test]
    fn test_uninstall() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(tmp.path(), "source-skill");

        let skills_dir = tmp.path().join("installed");
        fs::create_dir_all(&skills_dir).unwrap();

        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, skills_dir.clone());

        installer
            .install_from_local(&tmp.path().join("source-skill"))
            .unwrap();
        assert!(skills_dir.join("test-skill").exists());

        installer.uninstall("test-skill").unwrap();
        assert!(!skills_dir.join("test-skill").exists());
    }

    #[test]
    fn test_uninstall_not_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("installed");
        fs::create_dir_all(&skills_dir).unwrap();

        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, skills_dir);

        let result = installer.uninstall("nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not installed"));
    }

    #[test]
    fn test_list_installed_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("installed");
        fs::create_dir_all(&skills_dir).unwrap();

        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, skills_dir);

        let list = installer.list_installed().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_list_installed_finds_skills() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(tmp.path(), "source-skill");

        let skills_dir = tmp.path().join("installed");
        fs::create_dir_all(&skills_dir).unwrap();

        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, skills_dir);

        installer
            .install_from_local(&tmp.path().join("source-skill"))
            .unwrap();

        let list = installer.list_installed().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-skill");
    }

    #[test]
    fn test_list_installed_nonexistent_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("does-not-exist");

        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, skills_dir);

        let list = installer.list_installed().unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_read_installed_info_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let skills_dir = tmp.path().join("installed");
        fs::create_dir_all(&skills_dir).unwrap();

        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, skills_dir);

        let info = installer.read_installed_info("nonexistent").unwrap();
        assert!(info.is_none());
    }

    #[test]
    fn test_extract_package_tar_gz() {
        let tmp = tempfile::tempdir().unwrap();

        // Create a tar.gz in memory with a SKILL.md
        let mut builder = tar::Builder::new(Vec::new());
        let skill_content = sample_skill_md();
        let mut header = tar::Header::new_gnu();
        header.set_size(skill_content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "SKILL.md", skill_content.as_bytes())
            .unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        // Compress with gzip
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        let gz_bytes = encoder.finish().unwrap();

        let dest = tmp.path().join("extracted");
        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, tmp.path().to_path_buf());
        installer.extract_package(&gz_bytes, &dest).unwrap();

        assert!(dest.join("SKILL.md").exists());
    }

    #[test]
    fn test_extract_package_zip() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_content = sample_skill_md();

        // Create a ZIP in memory with a SKILL.md
        let mut zip_buf = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut zip_buf);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("SKILL.md", options).unwrap();
            std::io::Write::write_all(&mut writer, skill_content.as_bytes()).unwrap();
            writer.finish().unwrap();
        }

        let dest = tmp.path().join("extracted-zip");
        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, tmp.path().to_path_buf());
        installer
            .extract_package(zip_buf.get_ref(), &dest)
            .unwrap();

        assert!(dest.join("SKILL.md").exists());
        let content = fs::read_to_string(dest.join("SKILL.md")).unwrap();
        assert!(content.contains("test-skill"));
    }

    #[test]
    fn test_skills_dir_accessor() {
        let client = ClawHubClient::new("https://api.clawhub.com");
        let installer = SkillInstaller::new(client, PathBuf::from("/home/user/.mcclawd/skills"));
        assert_eq!(
            installer.skills_dir(),
            Path::new("/home/user/.mcclawd/skills")
        );
    }
}
