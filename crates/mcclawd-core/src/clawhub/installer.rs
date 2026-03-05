//! Skill downloader and installer — handles both local and registry installs.

use super::client::{ClawHubClient, ClawHubSkillMeta};
use crate::skill_parser::parse_skill_md;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
}

impl SkillInstaller {
    /// Create a new installer with a ClawHub client and a local skills directory.
    pub fn new(client: ClawHubClient, skills_dir: PathBuf) -> Self {
        Self { client, skills_dir }
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

        // Idempotent: return existing install info if already present
        if dest.exists() {
            if let Ok(Some(existing)) = self.read_installed_info(&meta.name) {
                return Ok(existing);
            }
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
