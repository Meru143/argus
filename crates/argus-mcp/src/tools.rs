//! Tool implementations for the Argus MCP server.
//!
//! Five tools are exposed: `analyze_diff`, `search_codebase`, `get_repo_map`,
//! `get_hotspots`, and `get_history`. Each delegates to the appropriate Argus
//! crate and returns JSON via `CallToolResult`.

use std::path::PathBuf;

use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_router, ErrorData as McpError,
};
use serde::{Deserialize, Serialize};

/// MCP server exposing Argus analysis tools.
///
/// # Examples
///
/// ```
/// use argus_mcp::tools::ArgusServer;
/// use std::path::PathBuf;
///
/// let server = ArgusServer::new(PathBuf::from("."));
/// ```
#[derive(Clone)]
pub struct ArgusServer {
    pub(crate) repo_path: PathBuf,
    pub(crate) tool_router: ToolRouter<Self>,
}

// --- Parameter structs ---

/// Parameters for the `analyze_diff` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeDiffParams {
    /// Unified diff text (git diff output).
    pub diff: String,
    /// Focus area: "all" (default), "security", "bugs", "style".
    pub focus: Option<String>,
}

/// Parameters for the `search_codebase` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchCodebaseParams {
    /// Search query (natural language or code).
    pub query: String,
    /// Repository path (default: server's configured path).
    pub path: Option<String>,
    /// Maximum results (default: 10).
    pub limit: Option<usize>,
}

/// Parameters for the `get_repo_map` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetRepoMapParams {
    /// Repository path (default: server's configured path).
    pub path: Option<String>,
    /// Token budget for the map (default: 2000).
    pub max_tokens: Option<usize>,
}

/// Parameters for the `get_hotspots` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetHotspotsParams {
    /// Repository path (default: server's configured path).
    pub path: Option<String>,
    /// Analyze last N days (default: 180).
    pub since_days: Option<u64>,
    /// Maximum results (default: 20).
    pub limit: Option<usize>,
}

/// Parameters for the `get_history` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetHistoryParams {
    /// Repository path (default: server's configured path).
    pub path: Option<String>,
    /// Analysis type: "coupling", "ownership", or "all" (default: "all").
    pub analysis: Option<String>,
    /// Analyze last N days (default: 180).
    pub since_days: Option<u64>,
    /// Minimum coupling degree to report (default: 0.3).
    pub min_coupling: Option<f64>,
}

// --- Response structs ---

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiffAnalysisResponse {
    risk_score: DiffRiskScore,
    files: Vec<DiffFileScore>,
    summary: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiffRiskScore {
    overall: f64,
    level: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiffFileScore {
    path: String,
    score: f64,
    lines_added: u32,
    lines_deleted: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoMapResponse {
    map: String,
    stats: RepoMapStats,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoMapStats {
    total_files: usize,
    total_symbols: usize,
    languages: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HotspotsResponse {
    hotspots: Vec<serde_json::Value>,
    summary: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    coupling: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ownership: Option<serde_json::Value>,
    summary: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    results: Vec<SearchResultEntry>,
    total: usize,
    indexed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchResultEntry {
    file_path: String,
    start_line: u32,
    end_line: u32,
    content: String,
    score: f64,
    language: Option<String>,
}

fn mcp_err(msg: impl Into<String>) -> McpError {
    McpError::internal_error(msg.into(), None)
}

#[tool_router]
impl ArgusServer {
    /// Create a new server with the given repository path.
    pub fn new(repo_path: PathBuf) -> Self {
        Self {
            repo_path,
            tool_router: Self::tool_router(),
        }
    }

    fn resolve_path(&self, path: &Option<String>) -> Result<PathBuf, McpError> {
        let canonical_repo_path = self.repo_path.canonicalize().map_err(|e| {
            mcp_err(format!(
                "Failed to access configured repository path {}: {e}",
                self.repo_path.display()
            ))
        })?;

        let requested_path = match path {
            Some(p) => {
                let input_path = PathBuf::from(p);
                if input_path.is_absolute() {
                    input_path
                } else {
                    canonical_repo_path.join(input_path)
                }
            }
            None => canonical_repo_path.clone(),
        };

        let canonical_requested_path = requested_path.canonicalize().map_err(|e| {
            mcp_err(format!(
                "Failed to resolve path {}: {e}",
                requested_path.display()
            ))
        })?;

        if !canonical_requested_path.starts_with(&canonical_repo_path) {
            return Err(mcp_err(format!(
                "Path {} is outside the configured repository {}",
                canonical_requested_path.display(),
                canonical_repo_path.display()
            )));
        }

        Ok(canonical_requested_path)
    }

    #[tool(
        name = "analyze_diff",
        description = "Analyze a git diff for bugs, security issues, and code quality problems. Parses the diff, computes risk scores, and returns categorized findings with file/line references. Use this when reviewing code changes or pull requests."
    )]
    pub fn analyze_diff(
        &self,
        Parameters(params): Parameters<AnalyzeDiffParams>,
    ) -> Result<CallToolResult, McpError> {
        let diffs = argus_difflens::parser::parse_unified_diff(&params.diff).map_err(|e| {
            mcp_err(format!(
                "Failed to parse diff: {e}. Ensure input is valid unified diff format (git diff output)."
            ))
        })?;

        if diffs.is_empty() {
            let json = serde_json::to_string_pretty(&serde_json::json!({
                "riskScore": { "overall": 0.0, "level": "Low" },
                "files": [],
                "summary": "No files found in diff."
            }))
            .unwrap();
            return Ok(CallToolResult::success(vec![Content::text(json)]));
        }

        let report = argus_difflens::risk::compute_risk(&diffs);

        let files: Vec<DiffFileScore> = report
            .per_file
            .iter()
            .map(|f| DiffFileScore {
                path: f.path.display().to_string(),
                score: f.score.total,
                lines_added: f.lines_added,
                lines_deleted: f.lines_deleted,
            })
            .collect();

        let response = DiffAnalysisResponse {
            risk_score: DiffRiskScore {
                overall: report.overall.total,
                level: format!("{}", report.summary.risk_level),
            },
            files,
            summary: format!(
                "{} file(s) changed: +{}/−{}. Risk level: {}.",
                report.summary.total_files,
                report.summary.total_additions,
                report.summary.total_deletions,
                report.summary.risk_level,
            ),
        };

        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "search_codebase",
        description = "Search for code in the repository using hybrid semantic + keyword search. Requires the codebase to be indexed first (will auto-index if no index exists). Use this to find related code, similar patterns, or specific symbols."
    )]
    pub async fn search_codebase(
        &self,
        Parameters(params): Parameters<SearchCodebaseParams>,
    ) -> Result<CallToolResult, McpError> {
        let repo_path = self.resolve_path(&params.path)?;
        let limit = params.limit.unwrap_or(10);
        let index_path = repo_path.join(".argus/index.db");
        let query = params.query;

        let config = argus_core::EmbeddingConfig::default();
        let embedding_client =
            argus_codelens::embedding::EmbeddingClient::with_config(&config).map_err(|e| {
                mcp_err(format!(
                "Voyage API key required. Set VOYAGE_API_KEY env var or configure in .argus.toml. Error: {e}"
            ))
            })?;

        let code_index = argus_codelens::store::CodeIndex::open(&index_path).map_err(|e| {
            mcp_err(format!(
                "Failed to open index at {}: {e}",
                index_path.display()
            ))
        })?;

        // HybridSearch is Send but not Sync (rusqlite Connection uses RefCell).
        // Move it into a blocking task and use Handle::block_on for the async parts.
        let handle = tokio::runtime::Handle::current();
        let result = tokio::task::spawn_blocking(move || {
            let search = argus_codelens::search::HybridSearch::new(code_index, embedding_client);

            handle.block_on(async {
                let stats = search.index().stats().map_err(|e| mcp_err(e.to_string()))?;
                let mut indexed = false;
                if stats.total_chunks == 0 {
                    search
                        .index_repo(&repo_path)
                        .await
                        .map_err(|e| mcp_err(format!("Failed to index repository: {e}")))?;
                    indexed = true;
                }

                let results = search
                    .search(&query, limit)
                    .await
                    .map_err(|e| mcp_err(format!("Search failed: {e}")))?;

                let entries: Vec<SearchResultEntry> = results
                    .iter()
                    .map(|r| SearchResultEntry {
                        file_path: r.file_path.display().to_string(),
                        start_line: r.line_start,
                        end_line: r.line_end,
                        content: r.snippet.clone(),
                        score: r.score,
                        language: r.language.clone(),
                    })
                    .collect();

                let total = entries.len();
                Ok::<_, McpError>(SearchResponse {
                    results: entries,
                    total,
                    indexed,
                })
            })
        })
        .await
        .map_err(|e| mcp_err(format!("Search task failed: {e}")))??;

        let json = serde_json::to_string_pretty(&result).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "get_repo_map",
        description = "Get a structural map of the codebase showing files, symbols, and their relationships. Uses tree-sitter to parse source files and PageRank to rank symbols by importance. Use this to understand codebase structure before making changes."
    )]
    pub fn get_repo_map(
        &self,
        Parameters(params): Parameters<GetRepoMapParams>,
    ) -> Result<CallToolResult, McpError> {
        let repo_path = self.resolve_path(&params.path)?;
        let max_tokens = params.max_tokens.unwrap_or(2000);

        let map = argus_repomap::generate_map(
            &repo_path,
            max_tokens,
            &[],
            argus_core::OutputFormat::Text,
        )
        .map_err(|e| mcp_err(format!("Failed to generate repo map: {e}")))?;

        let files = argus_repomap::walker::walk_repo(&repo_path)
            .map_err(|e| mcp_err(format!("Failed to walk repo: {e}")))?;

        let mut languages = std::collections::BTreeSet::new();
        let mut total_symbols = 0usize;
        for file in &files {
            languages.insert(format!("{:?}", file.language));
            let syms = argus_repomap::parser::extract_symbols(file).unwrap_or_default();
            total_symbols += syms.len();
        }

        let response = RepoMapResponse {
            map,
            stats: RepoMapStats {
                total_files: files.len(),
                total_symbols,
                languages: languages.into_iter().collect(),
            },
        };

        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "get_hotspots",
        description = "Find files with high change frequency and complexity — these are the most bug-prone areas of the codebase. Based on Adam Tornhill's \"Your Code as a Crime Scene\" methodology. Use this to identify risky areas before making changes."
    )]
    pub fn get_hotspots(
        &self,
        Parameters(params): Parameters<GetHotspotsParams>,
    ) -> Result<CallToolResult, McpError> {
        let repo_path = self.resolve_path(&params.path)?;
        let since_days = params.since_days.unwrap_or(180);
        let limit = params.limit.unwrap_or(20);

        let options = argus_gitpulse::mining::MiningOptions {
            since_days,
            ..argus_gitpulse::mining::MiningOptions::default()
        };

        let commits = argus_gitpulse::mining::mine_history(&repo_path, &options).map_err(|e| {
            mcp_err(format!(
                "Failed to mine git history: {e}. Is this a git repository?"
            ))
        })?;

        let hotspots = argus_gitpulse::hotspots::detect_hotspots(&repo_path, &commits)
            .map_err(|e| mcp_err(format!("Failed to detect hotspots: {e}")))?;

        let top: Vec<_> = hotspots.iter().take(limit).collect();
        let hotspot_values: Vec<serde_json::Value> = top
            .iter()
            .map(|h| serde_json::to_value(h).unwrap_or_default())
            .collect();

        let summary = if top.is_empty() {
            format!("No hotspots found in the last {since_days} days.")
        } else {
            let top_three: Vec<String> = top
                .iter()
                .take(3)
                .map(|h| format!("{} (score {:.2})", h.path, h.score))
                .collect();
            format!(
                "Found {} hotspot(s). Top: {}",
                top.len(),
                top_three.join(", ")
            )
        };

        let response = HotspotsResponse {
            hotspots: hotspot_values,
            summary,
        };

        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        name = "get_history",
        description = "Get git history analysis for the repository — temporal coupling (files that change together) and ownership metrics (knowledge silos, bus factor). Use this to understand code evolution and team knowledge distribution."
    )]
    pub fn get_history(
        &self,
        Parameters(params): Parameters<GetHistoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let repo_path = self.resolve_path(&params.path)?;
        let analysis = params.analysis.as_deref().unwrap_or("all");
        let since_days = params.since_days.unwrap_or(180);
        let min_coupling = params.min_coupling.unwrap_or(0.3);

        let show_coupling = matches!(analysis, "all" | "coupling");
        let show_ownership = matches!(analysis, "all" | "ownership");

        if !show_coupling && !show_ownership {
            return Err(mcp_err(
                "Invalid analysis type. Use \"coupling\", \"ownership\", or \"all\".",
            ));
        }

        let options = argus_gitpulse::mining::MiningOptions {
            since_days,
            ..argus_gitpulse::mining::MiningOptions::default()
        };

        let commits = argus_gitpulse::mining::mine_history(&repo_path, &options).map_err(|e| {
            mcp_err(format!(
                "Failed to mine git history: {e}. Is this a git repository?"
            ))
        })?;

        let mut summary_parts = Vec::new();
        summary_parts.push(format!(
            "Analyzed {} commits over last {since_days} days.",
            commits.len()
        ));

        let coupling = if show_coupling {
            let pairs = argus_gitpulse::coupling::detect_coupling(&commits, min_coupling, 3)
                .map_err(|e| mcp_err(format!("Coupling analysis failed: {e}")))?;
            summary_parts.push(format!("{} coupled pair(s) found.", pairs.len()));
            let values: Vec<serde_json::Value> = pairs
                .iter()
                .map(|p| serde_json::to_value(p).unwrap_or_default())
                .collect();
            Some(values)
        } else {
            None
        };

        let ownership = if show_ownership {
            let own = argus_gitpulse::ownership::analyze_ownership(&commits)
                .map_err(|e| mcp_err(format!("Ownership analysis failed: {e}")))?;
            summary_parts.push(format!(
                "Bus factor: {}. Knowledge silos: {}.",
                own.project_bus_factor, own.knowledge_silos
            ));
            Some(serde_json::to_value(&own).unwrap_or_default())
        } else {
            None
        };

        let response = HistoryResponse {
            coupling,
            ownership,
            summary: summary_parts.join(" "),
        };

        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn resolve_path_accepts_relative_in_repo_path() {
        let repo = tempfile::tempdir().unwrap();
        let src_dir = repo.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        let server = ArgusServer::new(repo.path().to_path_buf());
        let resolved = server.resolve_path(&Some("src".to_string())).unwrap();

        assert_eq!(resolved, src_dir.canonicalize().unwrap());
    }

    #[test]
    fn resolve_path_accepts_absolute_in_repo_path() {
        let repo = tempfile::tempdir().unwrap();
        let nested_dir = repo.path().join("nested");
        fs::create_dir_all(&nested_dir).unwrap();

        let server = ArgusServer::new(repo.path().to_path_buf());
        let resolved = server
            .resolve_path(&Some(nested_dir.display().to_string()))
            .unwrap();

        assert_eq!(resolved, nested_dir.canonicalize().unwrap());
    }

    #[test]
    fn resolve_path_rejects_parent_escape() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir_all(repo.path().join("safe")).unwrap();

        let server = ArgusServer::new(repo.path().to_path_buf());
        let err = server.resolve_path(&Some("../".to_string())).unwrap_err();

        assert!(err.message.contains("outside the configured repository"));
    }

    #[test]
    fn resolve_path_rejects_absolute_out_of_repo_path() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        let server = ArgusServer::new(repo.path().to_path_buf());
        let err = server
            .resolve_path(&Some(outside.path().display().to_string()))
            .unwrap_err();

        assert!(err.message.contains("outside the configured repository"));
    }
}
