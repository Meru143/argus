use std::io::IsTerminal;
use std::io::Read;
use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use miette::{Context, IntoDiagnostic, Result};

use argus_core::{OutputFormat, Severity};

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

    /// When to use colors
    #[arg(long, global = true, default_value = "auto")]
    color: ColorChoice,
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
    /// Analyze git history for hotspots, coupling, and ownership
    History {
        /// Repository path (default: current directory)
        #[arg(long, default_value = ".")]
        path: PathBuf,

        /// Analysis type
        #[arg(long, default_value = "all")]
        analysis: HistoryAnalysis,

        /// Time range in days (default: 180)
        #[arg(long, default_value = "180")]
        since: u64,

        /// Maximum results to show (default: 20)
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Minimum coupling degree to show (default: 0.3)
        #[arg(long, default_value = "0.3")]
        min_coupling: f64,
    },
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
        /// Exit with non-zero code if findings of this severity or higher are found
        #[arg(long)]
        fail_on: Option<Severity>,
        /// Show comments that were filtered out, with reasons
        #[arg(long)]
        show_filtered: bool,
        /// Apply suggested patches to the working tree
        #[arg(long)]
        apply_patches: bool,
    },
    /// Start the MCP server for IDE integration
    Mcp {
        /// Repository path (default: current directory)
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Create a default .argus.toml configuration file
    Init,
    /// Check your Argus setup and environment
    Doctor,
    /// Generate shell completion scripts
    #[command(hide = true)]
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Clone, ValueEnum)]
enum HistoryAnalysis {
    /// Detect high-churn hotspots
    Hotspots,
    /// Detect temporal coupling between files
    Coupling,
    /// Analyze knowledge silos and bus factor
    Ownership,
    /// Run all analyses
    All,
}

#[derive(Clone, PartialEq, Eq, ValueEnum)]
enum ColorChoice {
    /// Auto-detect based on terminal
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}

fn read_diff_input(file: &Option<PathBuf>) -> Result<String> {
    match file {
        Some(path) => std::fs::read_to_string(path)
            .into_diagnostic()
            .wrap_err(format!("reading {}", path.display())),
        None => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .into_diagnostic()
                .wrap_err("reading stdin")?;
            Ok(input)
        }
    }
}

#[derive(serde::Serialize)]
struct CheckResult {
    name: &'static str,
    status: &'static str,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

impl CheckResult {
    fn pass(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: "pass",
            detail: detail.into(),
            hint: None,
        }
    }

    fn fail(name: &'static str, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            name,
            status: "fail",
            detail: detail.into(),
            hint: Some(hint.into()),
        }
    }

    fn info(name: &'static str, detail: impl Into<String>) -> Self {
        Self {
            name,
            status: "info",
            detail: detail.into(),
            hint: None,
        }
    }

    fn symbol(&self) -> &'static str {
        match self.status {
            "pass" => "\u{2713}",
            "fail" => "\u{2717}",
            _ => "~",
        }
    }

    fn colored_symbol(&self) -> String {
        match self.status {
            "pass" => "\x1b[32m\u{2713}\x1b[0m".into(),
            "fail" => "\x1b[31m\u{2717}\x1b[0m".into(),
            _ => "\x1b[33m~\x1b[0m".into(),
        }
    }
}

fn run_doctor(
    config: &argus_core::ArgusConfig,
    format: OutputFormat,
    use_color: bool,
) -> Result<()> {
    let mut checks: Vec<CheckResult> = Vec::new();

    // 1. Git repository
    let mut git_root = None;
    let cwd = std::env::current_dir().into_diagnostic()?;
    let mut dir = cwd.as_path();
    loop {
        if dir.join(".git").exists() {
            git_root = Some(dir.to_path_buf());
            break;
        }
        let Some(parent) = dir.parent() else {
            break;
        };
        dir = parent;
    }
    match &git_root {
        Some(root) => checks.push(CheckResult::pass(
            "git_repository",
            format!("detected at {}", root.display()),
        )),
        None => checks.push(CheckResult::fail(
            "git_repository",
            "not a git repository",
            "run argus from inside a git repository",
        )),
    }

    // 2. Config file
    let config_path = std::path::Path::new(".argus.toml");
    if config_path.exists() {
        let rule_count = config.rules.len();
        let detail = if rule_count > 0 {
            format!(".argus.toml found ({rule_count} custom rules)")
        } else {
            ".argus.toml found".into()
        };
        checks.push(CheckResult::pass("config_file", detail));
    } else {
        checks.push(CheckResult::fail(
            "config_file",
            ".argus.toml not found",
            "run 'argus init' to create a default config",
        ));
    }

    // 3. LLM provider + API key
    let llm_provider = &config.llm.provider;
    let llm_model = &config.llm.model;
    let llm_env_var = match llm_provider.as_str() {
        "anthropic" => "ANTHROPIC_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        _ => "OPENAI_API_KEY",
    };
    checks.push(CheckResult::pass(
        "llm_provider",
        format!("{llm_provider} (model: {llm_model})"),
    ));
    if config.llm.api_key.is_some() || std::env::var(llm_env_var).is_ok() {
        checks.push(CheckResult::pass(
            "llm_api_key",
            format!("{llm_env_var} set"),
        ));
    } else {
        checks.push(CheckResult::fail(
            "llm_api_key",
            format!("{llm_env_var} not set"),
            format!("export {llm_env_var}=... or set api_key in .argus.toml"),
        ));
    }

    // 4. Embedding provider + API key
    let emb_provider = &config.embedding.provider;
    let emb_model = &config.embedding.model;
    let emb_env_var = match emb_provider.as_str() {
        "gemini" => "GEMINI_API_KEY",
        "openai" => "OPENAI_API_KEY",
        _ => "VOYAGE_API_KEY",
    };
    checks.push(CheckResult::pass(
        "embedding_provider",
        format!("{emb_provider} (model: {emb_model})"),
    ));
    if config.embedding.api_key.is_some() || std::env::var(emb_env_var).is_ok() {
        checks.push(CheckResult::pass(
            "embedding_api_key",
            format!("{emb_env_var} set"),
        ));
    } else {
        checks.push(CheckResult::fail(
            "embedding_api_key",
            format!("{emb_env_var} not set"),
            format!("export {emb_env_var}=... or set api_key in .argus.toml [embedding]"),
        ));
    }

    // 5. Search index
    let index_path = cwd.join(".argus/index.db");
    if index_path.exists() {
        let detail = match rusqlite::Connection::open_with_flags(
            &index_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) {
            Ok(conn) => {
                let count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
                    .unwrap_or(0);
                format!("exists ({count} chunks)")
            }
            Err(_) => "exists".into(),
        };
        checks.push(CheckResult::pass("search_index", detail));
    } else {
        checks.push(CheckResult::info(
            "search_index",
            "not found (run 'argus search --index' to create)",
        ));
    }

    // 6. GitHub token
    if std::env::var("GITHUB_TOKEN").is_ok() || std::env::var("GH_TOKEN").is_ok() {
        checks.push(CheckResult::pass("github_token", "GITHUB_TOKEN set"));
    } else {
        checks.push(CheckResult::fail(
            "github_token",
            "GITHUB_TOKEN not set",
            "export GITHUB_TOKEN=... (needed for --post-comments)",
        ));
    }

    // 7. Git history
    if git_root.is_some() {
        match git2::Repository::discover(&cwd) {
            Ok(repo) => {
                let mut revwalk = repo.revwalk().into_diagnostic()?;
                revwalk.push_head().into_diagnostic()?;
                let since = chrono_days_ago(180);
                let mut count = 0u64;
                for oid in revwalk {
                    let Ok(oid) = oid else { break };
                    let Ok(commit) = repo.find_commit(oid) else {
                        break;
                    };
                    if commit.time().seconds() < since {
                        break;
                    }
                    count += 1;
                }
                checks.push(CheckResult::info(
                    "git_history",
                    format!("{count} commits in last 180 days"),
                ));
            }
            Err(_) => {
                checks.push(CheckResult::info(
                    "git_history",
                    "unable to read git history",
                ));
            }
        }
    }

    // Output
    match format {
        OutputFormat::Json => {
            let version = env!("CARGO_PKG_VERSION");
            let json = serde_json::json!({
                "version": version,
                "checks": checks,
            });
            println!("{}", serde_json::to_string_pretty(&json).into_diagnostic()?);
        }
        _ => {
            let version = env!("CARGO_PKG_VERSION");
            println!("Argus v{version} — Environment Check\n");

            for check in &checks {
                let sym = if use_color {
                    check.colored_symbol()
                } else {
                    check.symbol().to_string()
                };
                // Pad the name for alignment
                let label = check.name.replace('_', " ");
                println!("  {sym} {label:<20} {}", check.detail);
                if let Some(hint) = &check.hint {
                    println!("    hint: {hint}");
                }
            }

            let passed = checks.iter().filter(|c| c.status == "pass").count();
            let failed = checks.iter().filter(|c| c.status == "fail").count();
            let info = checks.iter().filter(|c| c.status == "info").count();
            println!("\n{passed} checks passed, {failed} failed, {info} info");
        }
    }

    Ok(())
}

fn chrono_days_ago(days: i64) -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    now - (days * 86400)
}

const DEFAULT_CONFIG: &str = r#"# Argus Configuration
# See: https://github.com/Meru143/argus

[review]
# LLM provider (OpenAI-compatible endpoint)
# api_base = "https://api.openai.com/v1"
# model = "gpt-4o"
# max_findings = 5

[review.noise]
# skip_patterns = ["*.lock", "*.min.js", "vendor/**"]
# min_confidence = 90
# include_suggestions = false

[embedding]
# provider = "voyage"
# model = "voyage-code-3"

[history]
# since_days = 180
# max_files_per_commit = 25

# Custom review rules (injected into LLM prompt)
# [[rules]]
# name = "no-unwrap"
# severity = "warning"
# description = "Do not use .unwrap() in production code"
"#;

#[tokio::main]
async fn main() -> Result<()> {
    miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .terminal_links(true)
                .build(),
        )
    }))
    .expect("miette handler");
    human_panic::setup_panic!();

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

    let use_color = match cli.color {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => std::io::stdout().is_terminal() && std::env::var("NO_COLOR").is_err(),
    };

    if cli.verbose {
        eprintln!("format: {}", cli.format);
        if !config.rules.is_empty() {
            let bugs = config.rules.iter().filter(|r| r.severity == "bug").count();
            let warnings = config
                .rules
                .iter()
                .filter(|r| r.severity == "warning")
                .count();
            let suggestions = config
                .rules
                .iter()
                .filter(|r| r.severity == "suggestion")
                .count();
            eprintln!(
                "Custom rules: {} loaded ({} bug, {} warning, {} suggestion)",
                config.rules.len(),
                bugs,
                warnings,
                suggestions,
            );
        }
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
            if cli.format == OutputFormat::Sarif {
                miette::bail!("SARIF output is only supported for the review subcommand.");
            }
            let input = read_diff_input(file)?;
            let diffs = argus_difflens::parser::parse_unified_diff(&input)?;
            let report = argus_difflens::risk::compute_risk(&diffs);

            match cli.format {
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&report).into_diagnostic()?
                    );
                }
                OutputFormat::Markdown => {
                    print!("{}", report.to_markdown());
                }
                OutputFormat::Text => {
                    print!("{report}");
                }
                OutputFormat::Sarif => unreachable!(),
            }
        }
        Command::Search {
            ref query,
            ref path,
            limit,
            index,
            reindex,
        } => {
            if cli.format == OutputFormat::Sarif {
                miette::bail!("SARIF output is only supported for the review subcommand.");
            }
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
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&results).into_diagnostic()?
                        );
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
                    OutputFormat::Sarif => unreachable!(),
                }
            } else if !index && !reindex {
                miette::bail!("provide a search query, or use --index / --reindex");
            }
        }
        Command::History {
            ref path,
            ref analysis,
            since,
            limit,
            min_coupling,
        } => {
            if cli.format == OutputFormat::Sarif {
                miette::bail!("SARIF output is only supported for the review subcommand.");
            }
            let options = argus_gitpulse::mining::MiningOptions {
                since_days: since,
                ..argus_gitpulse::mining::MiningOptions::default()
            };

            eprintln!(
                "Mining git history at {} (last {} days)...",
                path.display(),
                since
            );
            let commits = argus_gitpulse::mining::mine_history(path, &options)?;
            eprintln!("Analyzed {} commits.", commits.len());

            let show_hotspots =
                matches!(analysis, HistoryAnalysis::All | HistoryAnalysis::Hotspots);
            let show_coupling =
                matches!(analysis, HistoryAnalysis::All | HistoryAnalysis::Coupling);
            let show_ownership =
                matches!(analysis, HistoryAnalysis::All | HistoryAnalysis::Ownership);

            match cli.format {
                OutputFormat::Json => {
                    let mut json = serde_json::Map::new();
                    json.insert(
                        "commits_analyzed".into(),
                        serde_json::Value::from(commits.len()),
                    );

                    if show_hotspots {
                        let hotspots = argus_gitpulse::hotspots::detect_hotspots(path, &commits)?;
                        let top: Vec<_> = hotspots.into_iter().take(limit).collect();
                        json.insert(
                            "hotspots".into(),
                            serde_json::to_value(&top).into_diagnostic()?,
                        );
                    }
                    if show_coupling {
                        let coupling =
                            argus_gitpulse::coupling::detect_coupling(&commits, min_coupling, 3)?;
                        let top: Vec<_> = coupling.into_iter().take(limit).collect();
                        json.insert(
                            "coupling".into(),
                            serde_json::to_value(&top).into_diagnostic()?,
                        );
                    }
                    if show_ownership {
                        let ownership = argus_gitpulse::ownership::analyze_ownership(&commits)?;
                        json.insert(
                            "ownership".into(),
                            serde_json::to_value(&ownership).into_diagnostic()?,
                        );
                    }

                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::Value::Object(json))
                            .into_diagnostic()?
                    );
                }
                OutputFormat::Markdown => {
                    println!("# Git History Analysis\n");
                    println!("**Commits analyzed:** {}\n", commits.len());

                    if show_hotspots {
                        let hotspots = argus_gitpulse::hotspots::detect_hotspots(path, &commits)?;
                        println!("## Hotspots\n");
                        if hotspots.is_empty() {
                            println!("No hotspots detected.\n");
                        } else {
                            println!("| Rank | File | Score | Revisions | Churn | LoC | Authors |");
                            println!("|------|------|-------|-----------|-------|-----|---------|");
                            for (i, h) in hotspots.iter().take(limit).enumerate() {
                                println!(
                                    "| {} | `{}` | {:.2} | {} | {} | {} | {} |",
                                    i + 1,
                                    h.path,
                                    h.score,
                                    h.revisions,
                                    h.total_churn,
                                    h.current_loc,
                                    h.authors,
                                );
                            }
                            println!();
                        }
                    }

                    if show_coupling {
                        let coupling =
                            argus_gitpulse::coupling::detect_coupling(&commits, min_coupling, 3)?;
                        println!("## Temporal Coupling\n");
                        if coupling.is_empty() {
                            println!("No significant coupling detected.\n");
                        } else {
                            println!("| File A | File B | Coupling | Co-changes |");
                            println!("|--------|--------|----------|------------|");
                            for pair in coupling.iter().take(limit) {
                                println!(
                                    "| `{}` | `{}` | {:.2} | {} |",
                                    pair.file_a, pair.file_b, pair.coupling_degree, pair.co_changes,
                                );
                            }
                            println!();
                        }
                    }

                    if show_ownership {
                        let ownership = argus_gitpulse::ownership::analyze_ownership(&commits)?;
                        println!("## Ownership & Bus Factor\n");
                        println!("- **Total files:** {}", ownership.total_files);
                        println!(
                            "- **Single-author files:** {}",
                            ownership.single_author_files
                        );
                        println!("- **Knowledge silos:** {}", ownership.knowledge_silos);
                        println!(
                            "- **Project bus factor:** {}\n",
                            ownership.project_bus_factor
                        );

                        let silos: Vec<_> = ownership
                            .files
                            .iter()
                            .filter(|f| f.is_knowledge_silo)
                            .collect();
                        if !silos.is_empty() {
                            println!("### Knowledge Silos\n");
                            for f in silos.iter().take(limit) {
                                let top_author = f
                                    .authors
                                    .first()
                                    .map(|a| format!("{} ({:.0}%)", a.email, a.ratio * 100.0))
                                    .unwrap_or_default();
                                println!("- `{}`: {top_author}", f.path);
                            }
                            println!();
                        }
                    }
                }
                OutputFormat::Text => {
                    if show_hotspots {
                        let hotspots = argus_gitpulse::hotspots::detect_hotspots(path, &commits)?;
                        println!("Hotspots (top {limit}):");
                        println!("{:-<72}", "");
                        for (i, h) in hotspots.iter().take(limit).enumerate() {
                            println!(
                                "{:>2}. {:<40} score={:.2}  rev={}  churn={}  loc={}  authors={}",
                                i + 1,
                                h.path,
                                h.score,
                                h.revisions,
                                h.total_churn,
                                h.current_loc,
                                h.authors,
                            );
                        }
                        println!();
                    }

                    if show_coupling {
                        let coupling =
                            argus_gitpulse::coupling::detect_coupling(&commits, min_coupling, 3)?;
                        println!("Temporal Coupling (min coupling: {min_coupling}):");
                        println!("{:-<72}", "");
                        if coupling.is_empty() {
                            println!("  No significant coupling detected.");
                        } else {
                            for pair in coupling.iter().take(limit) {
                                println!(
                                    "  {} <-> {} (coupling={:.2}, co-changes={})",
                                    pair.file_a, pair.file_b, pair.coupling_degree, pair.co_changes,
                                );
                            }
                        }
                        println!();
                    }

                    if show_ownership {
                        let ownership = argus_gitpulse::ownership::analyze_ownership(&commits)?;
                        println!("Ownership & Bus Factor:");
                        println!("{:-<72}", "");
                        println!("  Total files:        {}", ownership.total_files);
                        println!("  Single-author:      {}", ownership.single_author_files);
                        println!("  Knowledge silos:    {}", ownership.knowledge_silos);
                        println!("  Project bus factor: {}", ownership.project_bus_factor);

                        let silos: Vec<_> = ownership
                            .files
                            .iter()
                            .filter(|f| f.is_knowledge_silo)
                            .collect();
                        if !silos.is_empty() {
                            println!("\n  Knowledge Silos:");
                            for f in silos.iter().take(limit) {
                                let top_author = f
                                    .authors
                                    .first()
                                    .map(|a| format!("{} ({:.0}%)", a.email, a.ratio * 100.0))
                                    .unwrap_or_default();
                                println!("    {}: {top_author}", f.path);
                            }
                        }
                        println!();
                    }
                }
                OutputFormat::Sarif => unreachable!(),
            }
        }
        Command::Review {
            ref pr,
            ref file,
            post_comments,
            ref repo,
            ref skip_pattern,
            include_suggestions,
            fail_on,
            show_filtered,
            apply_patches,
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
            let pipeline = argus_review::pipeline::ReviewPipeline::new(
                llm_client,
                review_config,
                config.rules.clone(),
            );
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
                if !result.stats.file_groups.is_empty() {
                    eprintln!("Cross-file grouping:");
                    for (i, group) in result.stats.file_groups.iter().enumerate() {
                        let label = if group.len() == 1 { "file" } else { "files" };
                        let names = group.join(", ");
                        eprintln!("  Group {} ({} {label}): {names}", i + 1, group.len());
                    }
                } else if result.stats.llm_calls > 1 {
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
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&result).into_diagnostic()?
                    );
                }
                OutputFormat::Markdown => {
                    print!("{}", result.to_markdown());
                }
                OutputFormat::Sarif => {
                    let sarif = argus_review::sarif::to_sarif(&result);
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&sarif).into_diagnostic()?
                    );
                }
                OutputFormat::Text => {
                    print!("{result}");
                }
            }

            if show_filtered && !result.filtered_comments.is_empty() {
                eprintln!("\n--- Filtered Comments ---");
                for fc in &result.filtered_comments {
                    let label = match fc.comment.severity {
                        argus_core::Severity::Bug => "BUG",
                        argus_core::Severity::Warning => "WARNING",
                        argus_core::Severity::Suggestion => "SUGGESTION",
                        argus_core::Severity::Info => "INFO",
                    };
                    eprintln!(
                        "FILTERED: {} | [{label}] {}:{} (confidence: {:.0}%)",
                        fc.reason,
                        fc.comment.file_path.display(),
                        fc.comment.line,
                        fc.comment.confidence,
                    );
                    eprintln!("  {}", fc.comment.message);
                }
                eprintln!("-------------------------");
            }

            if apply_patches {
                let repo_root = repo.as_deref().unwrap_or(std::path::Path::new("."));
                let patch_result = argus_review::patch::apply_patches(&result.comments, repo_root)?;
                eprintln!(
                    "{} patches applied, {} skipped",
                    patch_result.applied.len(),
                    patch_result.skipped.len(),
                );
                for ap in &patch_result.applied {
                    eprintln!("  applied: {}:{}", ap.file_path, ap.line);
                }
                for sp in &patch_result.skipped {
                    eprintln!("  skipped: {}:{} — {}", sp.file_path, sp.line, sp.reason);
                }
            }

            if post_comments {
                let Some(pr_ref) = pr else {
                    miette::bail!("--post-comments requires --pr");
                };
                let (owner, repo, pr_number) = argus_review::github::parse_pr_reference(pr_ref)?;
                let github = argus_review::github::GitHubClient::new(None)?;
                let summary = format!(
                    "Argus Code Review: {} comments ({} files reviewed)",
                    result.comments.len(),
                    result.stats.files_reviewed,
                );
                github
                    .post_review(&owner, &repo, pr_number, &result.comments, &summary)
                    .await?;
                eprintln!("Posted {} comments to {pr_ref}", result.comments.len());
            }

            if let Some(threshold) = fail_on {
                let has_findings = result
                    .comments
                    .iter()
                    .any(|c| c.severity.meets_threshold(threshold));
                if has_findings {
                    std::process::exit(1);
                }
            }
        }
        Command::Mcp { ref path } => {
            argus_mcp::server::run_server(path.clone()).await?;
        }
        Command::Init => {
            let path = std::path::Path::new(".argus.toml");
            if path.exists() {
                miette::bail!(".argus.toml already exists");
            }
            std::fs::write(path, DEFAULT_CONFIG).into_diagnostic()?;
            println!("Created .argus.toml with default configuration");
        }
        Command::Doctor => {
            run_doctor(&config, cli.format, use_color)?;
        }
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "argus", &mut std::io::stdout());
        }
    }

    Ok(())
}
