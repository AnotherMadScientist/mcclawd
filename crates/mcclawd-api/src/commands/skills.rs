use mcclawd_core::clawhub::client::ClawHubClient;
use mcclawd_core::clawhub::installer::SkillInstaller;
use mcclawd_core::config::McclawdConfig;
use mcclawd_core::skill_loader::SkillLoader;
use mcclawd_core::skill_parser::parse_skill_md;
use std::path::PathBuf;

/// Load config and build a SkillInstaller from it.
fn make_installer() -> anyhow::Result<SkillInstaller> {
    let config = McclawdConfig::default();
    let client = ClawHubClient::new(&config.skills.clawhub_api);
    let skills_dir = config.skills.managed_dir.clone();
    Ok(SkillInstaller::new(client, skills_dir))
}

/// mc skills list — list all installed skills.
pub async fn list() -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    let loader = SkillLoader::new(root);
    let skills = loader.discover_all()?;

    if skills.is_empty() {
        println!("No skills installed.");
        println!("Install skills with: mc skills install <name>");
        return Ok(());
    }

    println!("{:<20} {:<10} {}", "NAME", "VERSION", "DESCRIPTION");
    println!("{}", "-".repeat(60));
    for skill in &skills {
        println!(
            "{:<20} {:<10} {}",
            skill.name,
            skill.version,
            truncate(&skill.description, 40)
        );
    }
    println!("\n{} skill(s) installed.", skills.len());
    Ok(())
}

/// mc skills info <name> — show detailed skill info.
pub async fn info(name: &str) -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    let skill_path = root
        .join(".mcclawd")
        .join("skills")
        .join(name)
        .join("SKILL.md");

    if !skill_path.exists() {
        anyhow::bail!("Skill '{}' not found at {}", name, skill_path.display());
    }

    let content = std::fs::read_to_string(&skill_path)?;
    let skill = parse_skill_md(&content)?;

    println!("Name:        {}", skill.name);
    println!("Version:     {}", skill.version);
    println!("Author:      {}", skill.author);
    println!("Description: {}", skill.description);

    if !skill.mcp_tools.is_empty() {
        println!("\nMCP Tools:");
        for tool in &skill.mcp_tools {
            println!("  - {tool}");
        }
    }
    if !skill.install_steps.is_empty() {
        println!("\nInstall Steps:");
        for step in &skill.install_steps {
            println!("  $ {step}");
        }
    }
    if !skill.context.is_empty() {
        println!("\nContext:");
        println!("  {}", skill.context);
    }
    Ok(())
}

/// mc skills search <query> — search the ClawHub registry.
pub async fn search(query: &str) -> anyhow::Result<()> {
    let config = McclawdConfig::default();
    let client = ClawHubClient::new(&config.skills.clawhub_api);
    let results = client.search(query, 0).await?;

    if results.skills.is_empty() {
        println!("No skills found for '{}'", query);
        return Ok(());
    }

    println!(
        "{:<25} {:<10} {:<15} {}",
        "NAME", "VERSION", "AUTHOR", "DESCRIPTION"
    );
    println!("{}", "-".repeat(70));
    for skill in &results.skills {
        println!(
            "{:<25} {:<10} {:<15} {}",
            skill.name,
            skill.version,
            skill.author,
            truncate(&skill.description, 30)
        );
    }
    println!(
        "\n{} of {} results shown.",
        results.skills.len(),
        results.total
    );
    Ok(())
}

/// mc skills install <source> — install from local path or registry.
/// Detects local path vs registry name. Supports name@version syntax.
pub async fn install(source: &str) -> anyhow::Result<()> {
    let source_path = PathBuf::from(source);

    if source_path.exists() && source_path.join("SKILL.md").exists() {
        // Local install path
        let installer = make_installer()?;
        let info = installer.install_from_local(&source_path)?;
        println!(
            "Installed skill '{}' v{} from local path",
            info.name, info.version
        );
    } else {
        // Registry install: parse name[@version]
        let (name, version) = parse_skill_ref(source);
        let installer = make_installer()?;
        let info = installer
            .install_from_registry(name, version)
            .await?;
        println!(
            "Installed skill '{}' v{} from ClawHub registry",
            info.name, info.version
        );
    }

    Ok(())
}

/// mc skills upgrade <name> — upgrade to latest version.
pub async fn upgrade(name: &str) -> anyhow::Result<()> {
    let installer = make_installer()?;
    let info = installer.upgrade(name).await?;
    println!(
        "Upgraded skill '{}' to v{}",
        info.name, info.version
    );
    Ok(())
}

/// mc skills uninstall <name> — remove an installed skill.
pub async fn uninstall(name: &str) -> anyhow::Result<()> {
    let installer = make_installer()?;
    installer.uninstall(name)?;
    println!("Uninstalled skill '{}'", name);
    Ok(())
}

/// Parse "name@version" or "name" into (name, Option<version>).
fn parse_skill_ref(input: &str) -> (&str, Option<&str>) {
    if let Some(idx) = input.rfind('@') {
        let name = &input[..idx];
        let version = &input[idx + 1..];
        if !name.is_empty() && !version.is_empty() {
            return (name, Some(version));
        }
    }
    (input, None)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_ref_name_only() {
        let (name, version) = parse_skill_ref("code-review");
        assert_eq!(name, "code-review");
        assert_eq!(version, None);
    }

    #[test]
    fn test_parse_skill_ref_with_version() {
        let (name, version) = parse_skill_ref("code-review@1.2.0");
        assert_eq!(name, "code-review");
        assert_eq!(version, Some("1.2.0"));
    }

    #[test]
    fn test_parse_skill_ref_trailing_at() {
        let (name, version) = parse_skill_ref("code-review@");
        assert_eq!(name, "code-review@");
        assert_eq!(version, None);
    }

    #[test]
    fn test_parse_skill_ref_leading_at() {
        let (name, version) = parse_skill_ref("@1.0.0");
        assert_eq!(name, "@1.0.0");
        assert_eq!(version, None);
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate("hello world foo bar", 10), "hello w...");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate("hello", 5), "hello");
    }
}
