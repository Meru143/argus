# Argus

> Your coding agent shouldn't grade its own homework.

[![CI](https://github.com/Meru143/argus/actions/workflows/ci.yml/badge.svg)](https://github.com/Meru143/argus/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/argus-ai.svg)](https://crates.io/crates/argus-ai)
[![npm version](https://img.shields.io/npm/v/argus-ai.svg)](https://www.npmjs.com/package/argus-ai)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![GitHub stars](https://img.shields.io/github/stars/Meru143/argus)](https://github.com/Meru143/argus)

Argus is a local-first, modular AI code review platform. One binary, six tools, zero lock-in. It combines structural analysis, semantic search, git history intelligence, and LLM-powered reviews to catch what your copilot misses.

## Why Argus?

- **Independent review** — your AI agent wrote the code, a different AI reviews it. No self-grading.
- **Full codebase context** — reviews use structural maps, semantic search, git history, and cross-file analysis. Not just the diff.
- **Zero lock-in** — works with OpenAI, Anthropic, or Gemini. Switch providers in one line. **Gemini free tier = zero cost.**
- **One binary, six tools** — map, diff, search, history, review, MCP server. Composable Unix-style subcommands.

## Get Started in 60 Seconds

```bash
# 1. Install via npm
npx argus-ai init          # creates .argus.toml

# 2. Set your key (Gemini, Anthropic, or OpenAI)
export GEMINI_API_KEY="your-key"

# 3. Review your changes
git diff HEAD~1 | npx argus-ai review --repo .
```

## Install

### npm (Recommended)

```bash
npx argus-ai --help
# or
npm install -g argus-ai
```

### Cargo

```bash
cargo install argus-ai
```

### From Source

```bash
cargo install --path .
```

## Subcommands

### `review` — AI Code Review
Run a context-aware review on any diff or PR.

```bash
# Review local changes
git diff main | argus review --repo .

# Review a GitHub PR (posts comments back to GitHub)
argus review --pr owner/repo#42 --post-comments
```

### `describe` — PR Descriptions
Generate structured, conventional-commit PR descriptions from your changes.

```bash
# Generate description for staged changes
argus describe

# Generate for a specific PR
argus describe --pr owner/repo#42
```

### `map` — Codebase Structure
Generate a ranked map of your codebase structure (tree-sitter + PageRank).

```bash
argus map --path . --max-tokens 2048
```

### `search` — Semantic Search
Hybrid code search using embeddings (Voyage/Gemini/OpenAI) + keywords.

```bash
argus search "auth middleware" --path . --limit 5
```

### `history` — Git Intelligence
Detect hotspots, temporal coupling, and bus factor risks.

```bash
argus history --path . --analysis hotspots --since 90
```

### `diff` — Risk Scoring
Analyze diffs for risk based on size, complexity, and diffusion.

```bash
git diff | argus diff
```

### `mcp` — MCP Server
Connect Argus to Cursor, Windsurf, or Claude Code.

```bash
argus mcp --path /absolute/path/to/repo
```

### `doctor` — Diagnostics
Check your environment, API keys, and configuration.

```bash
argus doctor
```

## GitHub Action

Add automated reviews to your PRs:

```yaml
name: Argus Review
on: [pull_request]
permissions:
  pull-requests: write
  contents: read
jobs:
  review:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - name: Install Argus
        run: npm install -g argus-ai
      - name: Run Review
        env:
          GEMINI_API_KEY: ${{ secrets.GEMINI_API_KEY }}
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          argus-ai review \
            --diff origin/${{ github.base_ref }}..HEAD \
            --pr ${{ github.repository }}#${{ github.event.pull_request.number }} \
            --post-comments \
            --fail-on bug
```

## MCP Setup

<details>
<summary><strong>Claude Code</strong></summary>

Add to `~/.mcp.json` or project `.mcp.json`:

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
</details>

<details>
<summary><strong>Cursor / Windsurf</strong></summary>

Add to generic MCP settings:

```json
{
  "argus": {
    "command": "argus",
    "args": ["mcp", "--path", "."]
  }
}
```
</details>

## Configuration

Run `argus init` to generate a `.argus.toml`:

```toml
[review]
# max_comments = 5
# min_confidence = 90
# skip_patterns = ["*.lock", "*.min.js", "vendor/**"]
```

## Custom Rules

Argus supports natural language custom rules. Create a file at `.argus/rules.md` (or `.argus/rules/**/*.md`) to guide the AI reviewer.

**Example `.argus/rules.md`:**
```markdown
- Always suggest using `anyhow::Result` instead of `Result<T, Box<dyn Error>>`.
- Flag usage of `unwrap()` in production code; suggest `expect()` or error handling.
- Ensure all public functions have doc comments.
```

### LLM Providers

| Provider | Config | Model | Env Variable |
|----------|--------|-------|-------------|
| Gemini | `provider = "gemini"` | `gemini-2.0-flash` | `GEMINI_API_KEY` |
| OpenAI | `provider = "openai"` | `gpt-4o` | `OPENAI_API_KEY` |
| Anthropic | `provider = "anthropic"` | `claude-sonnet-4-5` | `ANTHROPIC_API_KEY` |
| Ollama | `provider = "ollama"` | `llama3` | (None) |

### Embedding Providers

| Provider | Config | Model | Env Variable |
|----------|--------|-------|-------------|
| Gemini | `provider = "gemini"` | `text-embedding-004` | `GEMINI_API_KEY` |
| Voyage | `provider = "voyage"` | `voyage-code-3` | `VOYAGE_API_KEY` |
| OpenAI | `provider = "openai"` | `text-embedding-3-small` | `OPENAI_API_KEY` |

**Zero-cost setup:** Use Gemini for both LLM and embeddings with a [free API key](https://aistudio.google.com/apikey).

```toml
[llm]
provider = "gemini"

[embedding]
provider = "gemini"
```

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `GEMINI_API_KEY` | Gemini LLM + embeddings |
| `OPENAI_API_KEY` | OpenAI LLM + embeddings |
| `ANTHROPIC_API_KEY` | Anthropic LLM |
| `VOYAGE_API_KEY` | Voyage embeddings |
| `GITHUB_TOKEN` | GitHub PR integration |

## Architecture

```
                    ┌─────────────┐
                    │   argus     │
                    └──────┬──────┘
                           │
          ┌────────────────┼────────────────┐
          ▼                ▼                ▼
  ┌───────────────┐ ┌───────────┐ ┌──────────────┐
  │ argus-review  │ │ argus-mcp │ │  subcommands │
  └───────┬───────┘ └─────┬─────┘ └───────┬──────┘
          │               │               │
    ┌─────┴─────┬─────────┘               │
    ▼           ▼           ▼             ▼
┌─────────┐ ┌─────────┐ ┌──────────┐ ┌──────────┐
│ repomap │ │difflens │ │ codelens │ │ gitpulse │
└─────────┘ └─────────┘ └──────────┘ └──────────┘
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

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
