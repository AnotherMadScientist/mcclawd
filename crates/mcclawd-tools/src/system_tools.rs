//! System agent tools — builtin tools for UI control and app management.
//!
//! These tools are used by the persistent system agent (always-on, accessible from every page).
//! Each tool returns structured JSON that the frontend interprets as UI actions.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;

// ----------------------------------------------------------------
// Error
// ----------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
#[error("System tool error: {0}")]
pub struct SystemToolError(String);

// ----------------------------------------------------------------
// NavigateTo — navigate the UI to a given path
// ----------------------------------------------------------------

#[derive(Deserialize)]
pub struct NavigateToArgs {
    /// Target path, e.g. "/tasks", "/config/skills", "/workspace"
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NavigateTo;

impl Tool for NavigateTo {
    const NAME: &'static str = "navigate_to";
    type Error = SystemToolError;
    type Args = NavigateToArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "navigate_to".to_string(),
            description: "Navigate the McClawd UI to a specific page. Available paths: /tasks (task list), /tasks/{id} (task detail), /workspace (workspace editor), /config/secrets (secrets management), /config/skills (skills browser), /config/mcp (MCP servers), /config (general config).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The UI route path to navigate to (e.g. '/tasks', '/config/skills')"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok(json!({
            "action": "navigate",
            "path": args.path
        })
        .to_string())
    }
}

// ----------------------------------------------------------------
// CreateTask — create a new agent task
// ----------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateTaskArgs {
    /// The prompt/instruction for the new task
    pub prompt: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CreateTask;

impl Tool for CreateTask {
    const NAME: &'static str = "create_task";
    type Error = SystemToolError;
    type Args = CreateTaskArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "create_task".to_string(),
            description: "Create a new agent task with the given prompt. The task will be queued and executed by the agent engine.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "The task prompt/instruction for the agent"
                    }
                },
                "required": ["prompt"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok(json!({
            "action": "create_task",
            "prompt": args.prompt
        })
        .to_string())
    }
}

// ----------------------------------------------------------------
// InstallSkill — install a skill from ClawHub
// ----------------------------------------------------------------

#[derive(Deserialize)]
pub struct InstallSkillArgs {
    /// The skill name to install (e.g. "web-scraper")
    pub name: String,
    /// Optional version constraint
    pub version: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InstallSkill;

impl Tool for InstallSkill {
    const NAME: &'static str = "install_skill";
    type Error = SystemToolError;
    type Args = InstallSkillArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "install_skill".to_string(),
            description: "Install a skill from ClawHub by name. Optionally specify a version."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The skill name to install from ClawHub"
                    },
                    "version": {
                        "type": "string",
                        "description": "Optional version to install (default: latest)"
                    }
                },
                "required": ["name"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok(json!({
            "action": "install_skill",
            "name": args.name,
            "version": args.version
        })
        .to_string())
    }
}

// ----------------------------------------------------------------
// UninstallSkill — remove an installed skill
// ----------------------------------------------------------------

#[derive(Deserialize)]
pub struct UninstallSkillArgs {
    /// The skill name to uninstall
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UninstallSkill;

impl Tool for UninstallSkill {
    const NAME: &'static str = "uninstall_skill";
    type Error = SystemToolError;
    type Args = UninstallSkillArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "uninstall_skill".to_string(),
            description: "Uninstall an installed skill by name.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The skill name to uninstall"
                    }
                },
                "required": ["name"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok(json!({
            "action": "uninstall_skill",
            "name": args.name
        })
        .to_string())
    }
}

// ----------------------------------------------------------------
// ListSkills — list installed skills
// ----------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListSkillsArgs {}

#[derive(Serialize, Deserialize, Clone)]
pub struct ListSkills;

impl Tool for ListSkills {
    const NAME: &'static str = "list_skills";
    type Error = SystemToolError;
    type Args = ListSkillsArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "list_skills".to_string(),
            description: "List all currently installed skills and navigate to the skills page."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok(json!({
            "action": "list_skills"
        })
        .to_string())
    }
}

// ----------------------------------------------------------------
// ManageSecret — create/update/delete secrets
// ----------------------------------------------------------------

#[derive(Deserialize)]
pub struct ManageSecretArgs {
    /// The operation: "set", "get", "delete", "list"
    pub operation: String,
    /// Secret name (required for set/get/delete)
    pub name: Option<String>,
    /// Secret value (required for set)
    pub value: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ManageSecret;

impl Tool for ManageSecret {
    const NAME: &'static str = "manage_secret";
    type Error = SystemToolError;
    type Args = ManageSecretArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "manage_secret".to_string(),
            description: "Manage secrets (API keys, credentials). Operations: set (create/update), delete, list. Never reveals secret values in responses.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "operation": {
                        "type": "string",
                        "enum": ["set", "delete", "list"],
                        "description": "The operation to perform"
                    },
                    "name": {
                        "type": "string",
                        "description": "Secret name (required for set/delete)"
                    },
                    "value": {
                        "type": "string",
                        "description": "Secret value (required for set)"
                    }
                },
                "required": ["operation"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok(json!({
            "action": "manage_secret",
            "operation": args.operation,
            "name": args.name,
            "value": args.value
        })
        .to_string())
    }
}

// ----------------------------------------------------------------
// ReadWorkspace — read SOUL/AGENTS/USER files
// ----------------------------------------------------------------

#[derive(Deserialize)]
pub struct ReadWorkspaceArgs {
    /// Which file to read: "SOUL", "AGENTS", or "USER"
    pub file: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ReadWorkspace;

impl Tool for ReadWorkspace {
    const NAME: &'static str = "read_workspace";
    type Error = SystemToolError;
    type Args = ReadWorkspaceArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "read_workspace".to_string(),
            description: "Read a workspace file. Available files: SOUL (agent identity/personality), AGENTS (agent definitions and skills), USER (user preferences).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "enum": ["SOUL", "AGENTS", "USER"],
                        "description": "Which workspace file to read"
                    }
                },
                "required": ["file"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok(json!({
            "action": "read_workspace",
            "file": args.file
        })
        .to_string())
    }
}

// ----------------------------------------------------------------
// UpdateWorkspace — write SOUL/AGENTS/USER files
// ----------------------------------------------------------------

#[derive(Deserialize)]
pub struct UpdateWorkspaceArgs {
    /// Which file to update: "SOUL", "AGENTS", or "USER"
    pub file: String,
    /// New content for the file
    pub content: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UpdateWorkspace;

impl Tool for UpdateWorkspace {
    const NAME: &'static str = "update_workspace";
    type Error = SystemToolError;
    type Args = UpdateWorkspaceArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "update_workspace".to_string(),
            description: "Update a workspace file with new content. Available files: SOUL (agent identity/personality), AGENTS (agent definitions and skills), USER (user preferences).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": {
                        "type": "string",
                        "enum": ["SOUL", "AGENTS", "USER"],
                        "description": "Which workspace file to update"
                    },
                    "content": {
                        "type": "string",
                        "description": "New content for the workspace file (markdown)"
                    }
                },
                "required": ["file", "content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        Ok(json!({
            "action": "update_workspace",
            "file": args.file,
            "content": args.content
        })
        .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn navigate_to_returns_action() {
        let tool = NavigateTo;
        let result = tool
            .call(NavigateToArgs {
                path: "/config/skills".into(),
            })
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["action"], "navigate");
        assert_eq!(parsed["path"], "/config/skills");
    }

    #[tokio::test]
    async fn create_task_returns_action() {
        let tool = CreateTask;
        let result = tool
            .call(CreateTaskArgs {
                prompt: "Hello world".into(),
            })
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["action"], "create_task");
        assert_eq!(parsed["prompt"], "Hello world");
    }

    #[tokio::test]
    async fn install_skill_returns_action() {
        let tool = InstallSkill;
        let result = tool
            .call(InstallSkillArgs {
                name: "web-scraper".into(),
                version: None,
            })
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["action"], "install_skill");
        assert_eq!(parsed["name"], "web-scraper");
    }

    #[tokio::test]
    async fn manage_secret_returns_action() {
        let tool = ManageSecret;
        let result = tool
            .call(ManageSecretArgs {
                operation: "list".into(),
                name: None,
                value: None,
            })
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["action"], "manage_secret");
        assert_eq!(parsed["operation"], "list");
    }

    #[tokio::test]
    async fn read_workspace_returns_action() {
        let tool = ReadWorkspace;
        let result = tool
            .call(ReadWorkspaceArgs {
                file: "SOUL".into(),
            })
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["action"], "read_workspace");
        assert_eq!(parsed["file"], "SOUL");
    }
}
