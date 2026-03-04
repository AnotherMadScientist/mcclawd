use mcclawd_core::skill_loader::SkillLoader;
use mcclawd_core::skill_parser::parse_skill_md;
use std::path::PathBuf;

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

pub async fn info(name: &str) -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    let skill_path = root.join(".mcclawd").join("skills").join(name).join("SKILL.md");

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

pub async fn install(source: &str) -> anyhow::Result<()> {
    let source_path = PathBuf::from(source);
    let skill_md = source_path.join("SKILL.md");

    if !skill_md.exists() {
        anyhow::bail!("No SKILL.md found at {}", skill_md.display());
    }

    let content = std::fs::read_to_string(&skill_md)?;
    let skill = parse_skill_md(&content)?;

    let root = std::env::current_dir()?;
    let dest = root.join(".mcclawd").join("skills").join(&skill.name);

    if dest.exists() {
        anyhow::bail!(
            "Skill '{}' already installed at {}. Remove first.",
            skill.name,
            dest.display()
        );
    }

    copy_dir_all(&source_path, &dest)?;
    println!("Installed skill '{}' v{}", skill.name, skill.version);
    Ok(())
}

fn copy_dir_all(src: &PathBuf, dst: &PathBuf) -> anyhow::Result<()> {
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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}
