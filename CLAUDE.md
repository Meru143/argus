# Argus

AI-powered code review platform. One binary, multiple subcommands.
Validates AI-generated code — "your coding agent shouldn't grade its own homework."

## Architecture

Cargo workspace with internal crates (all under `crates/`):
- `argus-core` — shared types (`FileNode`, `DiffHunk`, `RiskScore`), config, error handling
- `argus-repomap` — repository structure mapping via tree-sitter + PageRank ranking
- `argus-difflens` — diff parsing, complexity scoring, risk analysis
- `argus-codelens` — code intelligence, AST-aware chunking, semantic search
- `argus-gitpulse` — git history analysis: hotspots, temporal coupling, knowledge silos (git2)
- `argus-review` — AI review orchestration combining insights from all modules
- `argus-mcp` — MCP server interface exposing tools to IDEs/agents (rmcp)

Binary: `argus` with subcommands (`map`, `diff`, `search`, `history`, `review`, `mcp`, `init`).

## Build & Test

```
cargo build                    # build all
cargo test                     # test all
cargo test -p argus-core       # test specific crate
cargo clippy -- -D warnings    # lint (treat warnings as errors)
cargo fmt                      # format
cargo doc --no-deps            # generate docs
```

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` (derive) | CLI parsing |
| `thiserror` | Library error types |
| `anyhow` | Binary error handling |
| `serde` / `serde_json` | Serialization |
| `tokio` | Async runtime |
| `tree-sitter` + language grammars | AST parsing |
| `git2` | Git operations (libgit2 bindings) |
| `petgraph` | Dependency/call graphs |
| `reqwest` | HTTP client (LLM API calls) |
| `octocrab` | GitHub API |
| `rmcp` | MCP server SDK |

## Conventions

- Error handling: `thiserror` for library crate errors, `anyhow` for the binary crate only
- CLI: `clap` with derive macros, not builder pattern
- Async: `tokio` multi-threaded runtime
- Serialization: `serde` with `#[serde(rename_all = "camelCase")]` for JSON output
- Follow the rust-style skill for code patterns (for-loops, let-else, newtypes, no wildcards)
- Each crate exposes a clean public API via `lib.rs` — minimize `pub` surface
- Tests go in `#[cfg(test)] mod tests` for unit, `tests/` directory for integration
- All public items must have rustdoc with `# Examples` section

## Crate Dependency Order

```
core (no internal deps)
  ├── repomap (depends on core)
  ├── difflens (depends on core)
  ├── codelens (depends on core, repomap)
  ├── gitpulse (depends on core)
  └── review (depends on core, repomap, difflens, codelens, gitpulse)
        └── mcp (depends on core, review)
```

Build order: core → repomap/difflens/gitpulse (parallel) → codelens → review → mcp → binary

## Output Format

All subcommands support `--format json|text|markdown` (default: text).
JSON output uses camelCase keys. Text output is human-readable tables/summaries.

## Decisions

- **git2 default-features disabled**: `git2 = { version = "0.19", default-features = false }` to avoid OpenSSL/SSH dependency since only local repo access is needed
- **Hotspot scoring formula**: Tornhill methodology — `score = norm(revisions) * 0.5 + norm(relative_churn) * 0.3 + norm(current_loc) * 0.2`; only files still on disk are included
- **Temporal coupling normalization**: Pairs normalized via lexicographic ordering so (A,B) == (B,A); coupling ratio = co_changes / min(changes_a, changes_b)
- **Bus factor algorithm**: Iteratively removes top contributor until >50% of files lose all significant authors (>10% ratio); knowledge silo threshold = dominant_author_ratio > 0.80
- **History context in review pipeline**: `build_history_context()` integrates hotspots/coupling/ownership into the LLM prompt for behaviorally-informed reviews
- **diff.foreach() for line counts**: Uses git2's `diff.foreach()` with line-level callbacks rather than `diff.stats()` to get per-file added/deleted counts
- **MCP server via rmcp**: Uses `#[tool_router]` + `#[tool_handler]` macros for tool registration; `ServerHandler` trait for server metadata; stdio transport only
- **MCP tool error messages teach**: Errors include guidance on what to try next (e.g., "Set VOYAGE_API_KEY env var")
- **MCP repo_path resolution**: Each tool accepts optional `path` param, falls back to server's configured `--path` from CLI
- **MCP search_codebase Send workaround**: `HybridSearch` is `Send` but not `Sync` (rusqlite `RefCell`), so `search_codebase` uses `spawn_blocking` + `Handle::block_on` to avoid holding `&HybridSearch` across await points in a `Send` future
- **`--fail-on` severity threshold**: `Severity::meets_threshold()` uses a rank ordering (Bug=0, Warning=1, Suggestion=2, Info=3); a finding meets the threshold if its rank <= the threshold's rank
- **`--post-comments` severity-based event**: PR reviews with any Bug-level finding use `REQUEST_CHANGES` event; otherwise `COMMENT`
- **`argus init` config template**: Uses commented-out TOML keys so the generated file is valid TOML but shows available options
- **Release workflow**: Uses `softprops/action-gh-release` with cross-compilation matrix (5 targets) rather than cargo-dist's built-in CI, for simplicity and control
- **`Severity` implements `FromStr`**: Allows clap to parse `--fail-on bug` without adding clap as a dependency to argus-core
