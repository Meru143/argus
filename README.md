# Argus — AI Code Review Platform

> Your coding agent shouldn't grade its own homework.

[![CI](https://github.com/Meru143/argus/actions/workflows/ci.yml/badge.svg)](https://github.com/Meru143/argus/actions/workflows/ci.yml)
[![npm version](https://img.shields.io/npm/v/argus-ai.svg)](https://www.npmjs.com/package/argus-ai)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

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

Run `argus init` to generate a `.argus.toml`.

<details>
<summary><strong>Full Configuration Example</strong></summary>

```toml
[review]
max_comments = 5
min_confidence = 90
include_suggestions = false

# Gemini (Zero Cost)
[llm]
provider = "gemini"
model = "gemini-2.0-flash"

[embedding]
provider = "gemini"
model = "text-embedding-004"

# Environment Variables:
# GEMINI_API_KEY, ANTHROPIC_API_KEY, OPENAI_API_KEY, VOYAGE_API_KEY
```
</details>

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

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT
