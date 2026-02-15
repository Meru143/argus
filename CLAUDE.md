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

Binary: `argus` with subcommands (`map`, `diff`, `search`, `history`, `review`, `mcp`).

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
