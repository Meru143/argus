use anyhow::Result;
use clap::{Parser, Subcommand};

use argus_core::OutputFormat;

#[derive(Parser)]
#[command(
    name = "argus",
    version,
    about = "AI-powered code review platform",
    long_about = "Argus validates AI-generated code — your coding agent shouldn't grade its own homework.\n\n\
                   Composable subcommands for codebase mapping, diff analysis, semantic search,\n\
                   git history intelligence, AI reviews, and MCP server integration."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Path to configuration file (default: .argus.toml)
    #[arg(long, global = true)]
    config: Option<std::path::PathBuf>,

    /// Output format
    #[arg(long, global = true, default_value = "text")]
    format: OutputFormat,

    /// Enable verbose output
    #[arg(long, short, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a ranked map of the codebase structure
    Map,
    /// Analyze diffs and compute risk scores
    Diff,
    /// Search the codebase semantically
    Search,
    /// Analyze git history for hotspots and patterns
    History,
    /// Run an AI-powered code review
    Review,
    /// Start the MCP server for IDE integration
    Mcp,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let _config = match &cli.config {
        Some(path) => argus_core::ArgusConfig::from_file(path)?,
        None => argus_core::ArgusConfig::default(),
    };

    if cli.verbose {
        eprintln!("format: {}", cli.format);
    }

    match cli.command {
        Command::Map => {
            anyhow::bail!("map subcommand not yet implemented")
        }
        Command::Diff => {
            anyhow::bail!("diff subcommand not yet implemented")
        }
        Command::Search => {
            anyhow::bail!("search subcommand not yet implemented")
        }
        Command::History => {
            anyhow::bail!("history subcommand not yet implemented")
        }
        Command::Review => {
            anyhow::bail!("review subcommand not yet implemented")
        }
        Command::Mcp => {
            anyhow::bail!("mcp subcommand not yet implemented")
        }
    }
}
