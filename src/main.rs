use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result};
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
    config: Option<PathBuf>,

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
    Diff {
        /// Read diff from file instead of stdin
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Search the codebase semantically
    Search,
    /// Analyze git history for hotspots and patterns
    History,
    /// Run an AI-powered code review
    Review {
        /// GitHub PR to review (format: owner/repo#123)
        #[arg(long)]
        pr: Option<String>,
        /// Read diff from file instead of stdin
        #[arg(long)]
        file: Option<PathBuf>,
        /// Post comments to GitHub PR
        #[arg(long)]
        post_comments: bool,
    },
    /// Start the MCP server for IDE integration
    Mcp,
}

fn read_diff_input(file: &Option<PathBuf>) -> Result<String> {
    match file {
        Some(path) => {
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
        }
        None => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .context("reading stdin")?;
            Ok(input)
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = match &cli.config {
        Some(path) => argus_core::ArgusConfig::from_file(path)?,
        None => {
            let default_path = std::path::Path::new(".argus.toml");
            if default_path.exists() {
                argus_core::ArgusConfig::from_file(default_path)?
            } else {
                argus_core::ArgusConfig::default()
            }
        }
    };

    if cli.verbose {
        eprintln!("format: {}", cli.format);
    }

    match cli.command {
        Command::Map => {
            anyhow::bail!("map subcommand not yet implemented")
        }
        Command::Diff { ref file } => {
            let input = read_diff_input(file)?;
            let diffs = argus_difflens::parser::parse_unified_diff(&input)?;
            let report = argus_difflens::risk::compute_risk(&diffs);

            match cli.format {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                }
                OutputFormat::Markdown => {
                    print!("{}", report.to_markdown());
                }
                OutputFormat::Text => {
                    print!("{report}");
                }
            }
        }
        Command::Search => {
            anyhow::bail!("search subcommand not yet implemented")
        }
        Command::History => {
            anyhow::bail!("history subcommand not yet implemented")
        }
        Command::Review {
            ref pr,
            ref file,
            post_comments,
        } => {
            let diff_input = if let Some(pr_ref) = pr {
                let (owner, repo, pr_number) = argus_review::github::parse_pr_reference(pr_ref)?;
                let github = argus_review::github::GitHubClient::new(None)?;
                github.get_pr_diff(&owner, &repo, pr_number).await?
            } else {
                read_diff_input(file)?
            };

            let diffs = argus_difflens::parser::parse_unified_diff(&diff_input)?;

            let llm_client = argus_review::llm::LlmClient::new(&config.llm)?;
            let pipeline =
                argus_review::pipeline::ReviewPipeline::new(llm_client, config.review.clone());
            let result = pipeline.review(&diffs).await?;

            match cli.format {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                OutputFormat::Markdown => {
                    print!("{}", result.to_markdown());
                }
                OutputFormat::Text => {
                    print!("{result}");
                }
            }

            if post_comments {
                let Some(pr_ref) = pr else {
                    anyhow::bail!("--post-comments requires --pr");
                };
                let (owner, repo, pr_number) = argus_review::github::parse_pr_reference(pr_ref)?;
                let github = argus_review::github::GitHubClient::new(None)?;
                github
                    .post_review(&owner, &repo, pr_number, &result.comments)
                    .await?;
                eprintln!("Posted {} comments to {pr_ref}", result.comments.len());
            }
        }
        Command::Mcp => {
            anyhow::bail!("mcp subcommand not yet implemented")
        }
    }

    Ok(())
}
