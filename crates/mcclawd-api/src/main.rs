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
}

#[derive(Subcommand)]
enum SecretsAction {
    /// Set a secret value
    Set { key: String },
    /// Get a secret value (masked)
    Get { key: String },
    /// List all secret keys
    List,
    /// Delete a secret
    Delete { key: String },
}

#[derive(Subcommand)]
enum SkillsAction {
    /// List installed skills
    List,
    /// Show skill details
    Info { name: String },
    /// Install a skill from local path
    Install { source: String },
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("mcclawd=info".parse()?),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run { prompt, workspace } => {
            commands::run::execute(&prompt, &workspace).await?;
        }
        Commands::Secrets { action } => match action {
            SecretsAction::Set { key } => commands::secrets::set(&key).await?,
            SecretsAction::Get { key } => commands::secrets::get(&key).await?,
            SecretsAction::List => commands::secrets::list().await?,
            SecretsAction::Delete { key } => commands::secrets::delete(&key).await?,
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
        },
    }

    Ok(())
}
