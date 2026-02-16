//! MCP server setup and lifecycle.
//!
//! Provides [`run_server`] which starts the stdio-based MCP server,
//! registering all Argus tools and blocking until the client disconnects.

use std::path::PathBuf;

use argus_core::ArgusError;
use rmcp::{model::*, tool_handler, transport::stdio, ServerHandler, ServiceExt};

use crate::tools::ArgusServer;

const SERVER_INSTRUCTIONS: &str = "\
Argus is a code review and analysis platform. Use these tools to understand codebases:\n\
- analyze_diff: Review code changes for bugs, security issues, and quality\n\
- search_codebase: Find related code using semantic or keyword search\n\
- get_repo_map: Get a structural overview of the codebase\n\
- get_hotspots: Find files with high churn and complexity (bug-prone)\n\
- get_history: Get git history metrics for specific files or the whole project";

#[tool_handler]
impl ServerHandler for ArgusServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "argus".to_string(),
                title: Some("Argus Code Review".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: Some("AI-powered code review and analysis platform".to_string()),
                icons: None,
                website_url: None,
            },
            instructions: Some(SERVER_INSTRUCTIONS.to_string()),
        }
    }
}

/// Start the MCP server on stdio transport.
///
/// This is called by the `argus mcp` CLI subcommand. It blocks until
/// the client closes stdin.
///
/// # Errors
///
/// Returns [`ArgusError`] if the server fails to initialize or encounters
/// a transport error.
///
/// # Examples
///
/// ```no_run
/// use std::path::PathBuf;
///
/// # async fn example() -> Result<(), argus_core::ArgusError> {
/// argus_mcp::server::run_server(PathBuf::from(".")).await?;
/// # Ok(())
/// # }
/// ```
pub async fn run_server(repo_path: PathBuf) -> Result<(), ArgusError> {
    let server = ArgusServer::new(repo_path);
    let service = server
        .serve(stdio())
        .await
        .map_err(|e| ArgusError::Config(format!("MCP server failed to start: {e}")))?;

    service
        .waiting()
        .await
        .map_err(|e| ArgusError::Config(format!("MCP server error: {e}")))?;

    Ok(())
}
