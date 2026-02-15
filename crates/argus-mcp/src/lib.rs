//! MCP server interface exposing Argus tools to IDEs and agents.
//!
//! Implements a Model Context Protocol server using rmcp that exposes
//! `analyze_diff`, `search_codebase`, `get_repo_map`, `get_hotspots`,
//! and `get_history` tools over stdio transport for integration with
//! Cursor, Windsurf, Claude Desktop, and VS Code Copilot.
//!
//! # Examples
//!
//! ```no_run
//! use std::path::PathBuf;
//!
//! # async fn example() -> Result<(), argus_core::ArgusError> {
//! argus_mcp::server::run_server(PathBuf::from(".")).await?;
//! # Ok(())
//! # }
//! ```

pub mod server;
pub mod tools;
