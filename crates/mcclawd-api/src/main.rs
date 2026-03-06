use clap::{Parser, Subcommand};

mod commands;
mod sandbox;
mod supervisor;
mod server;

#[derive(Parser)]
#[command(name = "mc", version, about = "McClawd Agent Platform")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a single agent task
    Run {
        /// The prompt/task to execute
        prompt: String,
        /// Workspace to use
        #[arg(short, long, default_value = "default")]
        workspace: String,
        /// Run as a coordinated swarm instead of a single agent
        #[arg(long)]
        swarm: bool,
    },
    /// Manage encrypted secrets
    Secrets {
        #[command(subcommand)]
        action: SecretsAction,
    },
    /// Manage agent workspaces
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },
    /// Start the web server
    Serve {
        #[arg(short, long, default_value = "9090")]
        port: u16,
    },
    /// Manage ClawHub skills
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
    /// Import config from external platforms
    Import {
        #[command(subcommand)]
        action: ImportAction,
    },
}

#[derive(Subcommand)]
enum SecretsAction {
    /// Set a secret value
    Set {
        key: String,
        /// Value to set (if omitted, prompts interactively)
        #[arg(long)]
        value: Option<String>,
    },
    /// Get a secret value (masked)
    Get { key: String },
    /// List all secret keys
    List,
    /// Delete a secret
    Delete { key: String },
    /// Initialize vault, import .env keys, and seed API keys
    Init {
        /// Path to .env file (default: .env in current directory)
        #[arg(short, long)]
        env_file: Option<String>,
        /// Non-interactive mode: skip confirmation prompts (default: yes to all)
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Reset vault completely (deletes vault.key + secrets.enc). Requires confirmation.
    Reset {
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum SkillsAction {
    /// List installed skills
    List,
    /// Show skill details
    Info { name: String },
    /// Install a skill from local path or ClawHub registry (name[@version])
    Install { source: String },
    /// Search the ClawHub registry for skills
    Search { query: String },
    /// Upgrade an installed skill to the latest version
    Upgrade { name: String },
    /// Uninstall a skill by name
    Uninstall { name: String },
    /// Check all installed skills for available updates
    CheckUpdates,
}

#[derive(Subcommand)]
enum WorkspaceAction {
    /// Initialize a new workspace with template files
    Init {
        #[arg(default_value = "default")]
        name: String,
    },
    /// List all workspaces
    List,
}

#[derive(Subcommand)]
enum ImportAction {
    /// Import OpenClaw config (openclaw.json / .mcp.json)
    Openclaw {
        /// Path to openclaw.json (defaults to ~/.openclaw/openclaw.json)
        path: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file before anything reads env vars (auto-seed API keys, database_url, etc.)
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("mcclawd=info".parse()?),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            prompt,
            workspace,
            swarm,
        } => {
            commands::run::execute(&prompt, &workspace, swarm).await?;
        }
        Commands::Secrets { action } => match action {
            SecretsAction::Set { key, value } => commands::secrets::set(&key, value.as_deref()).await?,
            SecretsAction::Get { key } => commands::secrets::get(&key).await?,
            SecretsAction::List => commands::secrets::list().await?,
            SecretsAction::Delete { key } => commands::secrets::delete(&key).await?,
            SecretsAction::Init { env_file, yes } => commands::secrets::init(env_file.as_deref(), yes).await?,
            SecretsAction::Reset { yes } => commands::secrets::reset(yes).await?,
        },
        Commands::Workspace { action } => match action {
            WorkspaceAction::Init { name } => commands::workspace::init(&name).await?,
            WorkspaceAction::List => commands::workspace::list().await?,
        },
        Commands::Serve { port } => {
            commands::serve::execute(port).await?;
        }
        Commands::Skills { action } => match action {
            SkillsAction::List => commands::skills::list().await?,
            SkillsAction::Info { name } => commands::skills::info(&name).await?,
            SkillsAction::Install { source } => commands::skills::install(&source).await?,
            SkillsAction::Search { query } => commands::skills::search(&query).await?,
            SkillsAction::Upgrade { name } => commands::skills::upgrade(&name).await?,
            SkillsAction::Uninstall { name } => commands::skills::uninstall(&name).await?,
            SkillsAction::CheckUpdates => commands::skills::check_updates().await?,
        },
        Commands::Import { action } => match action {
            ImportAction::Openclaw { path } => {
                commands::import::import_openclaw(path.as_deref()).await?
            }
        },
    }

    Ok(())
}
