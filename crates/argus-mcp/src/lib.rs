//! MCP server interface exposing Argus tools to IDEs and agents.
//!
//! Implements a Model Context Protocol server using rmcp that exposes
//! `analyze_diff`, `search_codebase`, and `get_repo_map` tools over
//! stdio transport for integration with Cursor, Windsurf, and Claude Desktop.
