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
    Map {
        /// Repository path (default: current directory)
        #[arg(long, default_value = ".")]
        path: PathBuf,

        /// Maximum tokens for the map (default: 1024)
        #[arg(long, default_value = "1024")]
        max_tokens: usize,

        /// Focus files (boost ranking for symbols in these files)
        #[arg(long)]
        focus: Vec<PathBuf>,
    },
    /// Analyze diffs and compute risk scores
    Diff {
        /// Read diff from file instead of stdin
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Search the codebase semantically
    Search {
        /// Search query (omit to just index or reindex)
        query: Option<String>,

        /// Repository path (default: current directory)
        #[arg(long, default_value = ".")]
        path: PathBuf,

        /// Maximum results to return (default: 10)
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Index the repository before searching
        #[arg(long)]
        index: bool,

        /// Re-index only changed files
        #[arg(long)]
        reindex: bool,
    },
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
        /// Repository path for codebase context (enables repo map)
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Additional glob patterns to skip (e.g. "*.test.ts")
        #[arg(long)]
        skip_pattern: Vec<String>,
        /// Include suggestion-level comments (default: only bug+warning)
        #[arg(long)]
        include_suggestions: bool,
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
        Command::Map {
            ref path,
            max_tokens,
            ref focus,
        } => {
            let output = argus_repomap::generate_map(path, max_tokens, focus, cli.format)?;
            print!("{output}");
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
        Command::Search {
            ref query,
            ref path,
            limit,
            index,
            reindex,
        } => {
            let index_path = path.join(".argus/index.db");

            let embedding_client =
                argus_codelens::embedding::EmbeddingClient::with_config(&config.embedding)?;

            let code_index = argus_codelens::store::CodeIndex::open(&index_path)?;
            let search = argus_codelens::search::HybridSearch::new(code_index, embedding_client);

            if index {
                eprintln!("Indexing repository at {} ...", path.display());
                let stats = search.index_repo(path).await?;
                eprintln!(
                    "Indexed {} chunks from {} files ({} bytes)",
                    stats.total_chunks, stats.total_files, stats.index_size_bytes,
                );
            }

            if reindex {
                eprintln!("Re-indexing changed files at {} ...", path.display());
                let stats = search.reindex_repo(path).await?;
                eprintln!(
                    "Index now has {} chunks from {} files ({} bytes)",
                    stats.total_chunks, stats.total_files, stats.index_size_bytes,
                );
            }

            if let Some(q) = query {
                let results = search.search(q, limit).await?;

                match cli.format {
                    OutputFormat::Json => {
                        println!("{}", serde_json::to_string_pretty(&results)?);
                    }
                    OutputFormat::Markdown => {
                        if results.is_empty() {
                            println!("No results found.");
                        } else {
                            println!("# Search Results\n");
                            for (i, r) in results.iter().enumerate() {
                                let lang = r.language.as_deref().unwrap_or("text");
                                println!(
                                    "## {}. `{}:{}–{}` (score: {:.4})\n\n```{lang}\n{}\n```\n",
                                    i + 1,
                                    r.file_path.display(),
                                    r.line_start,
                                    r.line_end,
                                    r.score,
                                    r.snippet,
                                );
                            }
                        }
                    }
                    OutputFormat::Text => {
                        if results.is_empty() {
                            println!("No results found.");
                        } else {
                            for (i, r) in results.iter().enumerate() {
                                println!(
                                    "{}. {}:{}–{} (score: {:.4})",
                                    i + 1,
                                    r.file_path.display(),
                                    r.line_start,
                                    r.line_end,
                                    r.score,
                                );
                                // Show a snippet preview (first 3 lines)
                                let preview: String = r
                                    .snippet
                                    .lines()
                                    .take(3)
                                    .map(|l| format!("   {l}"))
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                println!("{preview}\n");
                            }
                        }
                    }
                }
            } else if !index && !reindex {
                anyhow::bail!("provide a search query, or use --index / --reindex");
            }
        }
        Command::History => {
            anyhow::bail!("history subcommand not yet implemented")
        }
        Command::Review {
            ref pr,
            ref file,
            post_comments,
            ref repo,
            ref skip_pattern,
            include_suggestions,
        } => {
            let diff_input = if let Some(pr_ref) = pr {
                let (owner, repo, pr_number) = argus_review::github::parse_pr_reference(pr_ref)?;
                let github = argus_review::github::GitHubClient::new(None)?;
                github.get_pr_diff(&owner, &repo, pr_number).await?
            } else {
                read_diff_input(file)?
            };

            let diffs = argus_difflens::parser::parse_unified_diff(&diff_input)?;

            // Apply CLI overrides to review config
            let mut review_config = config.review.clone();
            if !skip_pattern.is_empty() {
                review_config
                    .skip_patterns
                    .extend(skip_pattern.iter().cloned());
            }
            if include_suggestions {
                review_config.include_suggestions = true;
                if !review_config
                    .severity_filter
                    .contains(&argus_core::Severity::Suggestion)
                {
                    review_config
                        .severity_filter
                        .push(argus_core::Severity::Suggestion);
                }
            }

            let llm_client = argus_review::llm::LlmClient::new(&config.llm)?;
            let pipeline = argus_review::pipeline::ReviewPipeline::new(llm_client, review_config);
            let result = pipeline.review(&diffs, repo.as_deref()).await?;

            // Verbose output
            if cli.verbose {
                eprintln!("--- Review Stats ---");
                eprintln!(
                    "Files reviewed: {} | Files skipped: {}",
                    result.stats.files_reviewed, result.stats.files_skipped
                );
                if !result.stats.skipped_files.is_empty() {
                    eprintln!("Skipped files:");
                    for sf in &result.stats.skipped_files {
                        eprintln!("  {} ({})", sf.path.display(), sf.reason);
                    }
                }
                let token_estimate = diff_input.len() / 4;
                eprintln!("Token estimate: ~{}", token_estimate);
                eprintln!("LLM calls: {}", result.stats.llm_calls);
                if result.stats.llm_calls > 1 {
                    eprintln!("  (diff was split into per-file calls)");
                }
                eprintln!(
                    "Comments: {} generated, {} filtered, {} deduplicated, {} final",
                    result.stats.comments_generated,
                    result.stats.comments_filtered,
                    result.stats.comments_deduplicated,
                    result.comments.len(),
                );
                eprintln!("--------------------");
            }

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
