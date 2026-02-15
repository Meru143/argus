# Argus — AI Code Review Platform

> Your coding agent shouldn't grade its own homework.

Argus is a local-first, modular AI code review platform. One binary, six tools,
zero lock-in. It combines structural analysis, semantic search, git history
intelligence, and LLM-powered reviews to catch what your copilot misses.

## Features

- **`argus map`** — Structural codebase overview (tree-sitter + PageRank ranking)
- **`argus diff`** — Risk scoring for code changes (size, complexity, diffusion)
- **`argus search`** — Semantic + keyword hybrid code search (Voyage/Gemini/OpenAI embeddings)
- **`argus history`** — Hotspot detection, temporal coupling, bus factor analysis
- **`argus review`** — AI-powered code review with context from all modules
- **`argus mcp`** — MCP server for IDE integration (Cursor, Windsurf, Claude Code)

## Quick Start

### Install from source

```bash
cargo install --git https://github.com/Meru143/argus
```

### Download pre-built binary

```bash
# Linux (x86_64)
curl -sSL https://github.com/Meru143/argus/releases/latest/download/argus-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv argus /usr/local/bin/

# macOS (Apple Silicon)
curl -sSL https://github.com/Meru143/argus/releases/latest/download/argus-aarch64-apple-darwin.tar.gz | tar xz
sudo mv argus /usr/local/bin/
```

### Initialize configuration

```bash
argus init  # creates .argus.toml with sensible defaults
```

## Usage

### Codebase Map

Generate a ranked overview of your codebase structure:

```bash
argus map --path . --max-tokens 2048
argus map --path . --focus src/auth.rs --format json
```

### Diff Analysis

Compute risk scores for a set of changes:

```bash
git diff main..HEAD | argus diff
argus diff --file changes.patch --format markdown
```

### Semantic Search

Search your codebase with natural language queries:

```bash
# First, index the repository
argus search --path . --index

# Then search
argus search "authentication middleware" --path . --limit 5
```

### Git History Analysis

Detect hotspots, temporal coupling, and knowledge silos:

```bash
argus history --path . --analysis hotspots --since 90
argus history --path . --analysis coupling --min-coupling 0.5
argus history --path . --analysis ownership
argus history --path . --analysis all --format json
```

### AI Code Review

Run an AI-powered review on a diff:

```bash
# Review from stdin
git diff main..HEAD | argus review --repo .

# Review a GitHub PR
argus review --pr owner/repo#42

# Review and post comments back to the PR
argus review --pr owner/repo#42 --post-comments

# Fail CI if bugs are found
argus review --pr owner/repo#42 --fail-on bug

# Include low-severity suggestions
argus review --pr owner/repo#42 --include-suggestions

# Debug noise reduction — see which comments were filtered and why
argus review --pr owner/repo#42 --show-filtered
```

### MCP Server

Start the MCP server for IDE integration:

```bash
argus mcp --path /your/project
```

## GitHub Action

Add automated AI code review to your pull requests:

```yaml
name: Argus Code Review
on:
  pull_request:
    types: [opened, synchronize]

permissions:
  pull-requests: write
  contents: read

jobs:
  review:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install Argus
        run: |
          curl -sSL https://github.com/Meru143/argus/releases/latest/download/argus-x86_64-unknown-linux-gnu.tar.gz | tar xz
          sudo mv argus /usr/local/bin/

      - name: Run Review
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
          # Or use ANTHROPIC_API_KEY / GEMINI_API_KEY
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          argus review \
            --diff origin/${{ github.base_ref }}..HEAD \
            --pr ${{ github.repository }}#${{ github.event.pull_request.number }} \
            --post-comments \
            --fail-on bug
```

## MCP Setup

### Claude Code

Add to your project's `.mcp.json`:

```json
{
  "mcpServers": {
    "argus": {
      "command": "argus",
      "args": ["mcp", "--path", "/absolute/path/to/repo"]
    }
  }
}
```

### Cursor

Add to Cursor settings (`Settings > MCP Servers`):

```json
{
  "argus": {
    "command": "argus",
    "args": ["mcp", "--path", "."]
  }
}
```

### Windsurf

Add to `.windsurfrules` or MCP config:

```json
{
  "mcpServers": {
    "argus": {
      "command": "argus",
      "args": ["mcp", "--path", "."]
    }
  }
}
```

## Configuration

Create `.argus.toml` in your project root (or run `argus init`):

```toml
[review]
# max_comments = 5
# min_confidence = 90
# skip_patterns = ["*.lock", "*.min.js", "vendor/**"]
# include_suggestions = false

[history]
# since_days = 180
```

### LLM Providers

```toml
# OpenAI (default)
[llm]
provider = "openai"
model = "gpt-4o"

# Anthropic
[llm]
provider = "anthropic"
model = "claude-sonnet-4-5"

# Gemini (free tier available)
[llm]
provider = "gemini"
model = "gemini-2.0-flash"
```

### Embedding Providers

```toml
# Voyage (default)
[embedding]
provider = "voyage"
model = "voyage-code-3"

# Gemini
[embedding]
provider = "gemini"
model = "text-embedding-004"

# OpenAI
[embedding]
provider = "openai"
model = "text-embedding-3-small"
```

### Zero-Cost Setup

Use Gemini for both LLM and embeddings with a free API key:

```toml
[llm]
provider = "gemini"
model = "gemini-2.0-flash"

[embedding]
provider = "gemini"
model = "text-embedding-004"
```

```bash
export GEMINI_API_KEY="your-key"
argus review --pr owner/repo#42
```

### Environment Variables

| Variable | Provider |
|----------|----------|
| `OPENAI_API_KEY` | OpenAI LLM + embeddings |
| `ANTHROPIC_API_KEY` | Anthropic LLM |
| `GEMINI_API_KEY` | Gemini LLM + embeddings |
| `VOYAGE_API_KEY` | Voyage embeddings |
| `GITHUB_TOKEN` | GitHub PR integration |

## Architecture

```
                    ┌─────────────┐
                    │   argus     │  CLI binary
                    │  (clap)     │
                    └──────┬──────┘
                           │
          ┌────────────────┼────────────────┐
          │                │                │
          ▼                ▼                ▼
  ┌───────────────┐ ┌───────────┐ ┌──────────────┐
  │ argus-review  │ │ argus-mcp │ │  (subcommands │
  │ (pipeline,    │ │ (rmcp,    │ │   map, diff,  │
  │  llm, github) │ │  stdio)   │ │   search,     │
  └───────┬───────┘ └─────┬─────┘ │   history)    │
          │               │       └───────┬────────┘
    ┌─────┴─────┬─────────┘               │
    │           │                          │
    ▼           ▼           ▼              ▼
┌─────────┐ ┌─────────┐ ┌──────────┐ ┌──────────┐
│ repomap │ │difflens │ │ codelens │ │ gitpulse │
│(tree-   │ │(diff    │ │(semantic │ │(git2,    │
│ sitter, │ │ parse,  │ │ search,  │ │ hotspots,│
│ page-   │ │ risk)   │ │ AST      │ │ coupling,│
│ rank)   │ │         │ │ chunks)  │ │ bus      │
└────┬────┘ └────┬────┘ └────┬─────┘ │ factor)  │
     │           │           │       └────┬─────┘
     └───────────┴───────────┴────────────┘
                         │
                    ┌────┴────┐
                    │  core   │  shared types,
                    │         │  config, errors
                    └─────────┘
```

**Crate dependency order:**
```
core (no internal deps)
  ├── repomap (core)
  ├── difflens (core)
  ├── gitpulse (core)
  ├── codelens (core, repomap)
  └── review (core, repomap, difflens, codelens, gitpulse)
        └── mcp (core, review)
```

## Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/my-feature`
3. Make your changes and add tests
4. Run the test suite: `cargo test --workspace`
5. Run lints: `cargo clippy --workspace -- -D warnings`
6. Format: `cargo fmt`
7. Commit with conventional commits: `feat:`, `fix:`, `docs:`, etc.
8. Open a pull request

## License

MIT
