use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Workspace {
    pub name: String,
    pub soul: Option<String>,
    pub agents: Option<String>,
    pub user: Option<String>,
    pub path: PathBuf,
}

pub struct WorkspaceLoader {
    base_dir: PathBuf,
}

impl WorkspaceLoader {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn load(&self, name: &str) -> mcclawd_core::Result<Workspace> {
        let ws_path = self.base_dir.join(name);
        if !ws_path.exists() {
            return Err(mcclawd_core::McclawdError::Config(format!(
                "Workspace '{}' not found at {}",
                name,
                ws_path.display()
            )));
        }

        Ok(Workspace {
            name: name.to_string(),
            soul: read_optional(&ws_path.join("SOUL.md")),
            agents: read_optional(&ws_path.join("AGENTS.md")),
            user: read_optional(&ws_path.join("USER.md")),
            path: ws_path,
        })
    }

    pub fn scaffold(&self, name: &str) -> mcclawd_core::Result<PathBuf> {
        let ws_path = self.base_dir.join(name);
        std::fs::create_dir_all(&ws_path)?;

        let soul = "# Soul\n\nYou are McClawd, a security-focused AI assistant.\n\n\
            ## Personality\n- Direct and technical.\n- When uncertain, say so.\n\n\
            ## Rules\n- Never execute destructive operations without confirmation.\n\
            - Never store secrets in plaintext.\n";

        let agents = "# Agents\n\n## Default Skills\n- memory-management\n\n\
            ## Available Agents\n\n### default\n- **Specialty:** General purpose\n\
            - **Model:** claude-sonnet-4-5\n";

        let user = "# User\n\n## Preferences\n- Concise responses\n";

        std::fs::write(ws_path.join("SOUL.md"), soul)?;
        std::fs::write(ws_path.join("AGENTS.md"), agents)?;
        std::fs::write(ws_path.join("USER.md"), user)?;

        Ok(ws_path)
    }
}

fn read_optional(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}
