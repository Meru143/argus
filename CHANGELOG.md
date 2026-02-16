# Changelog

All notable changes to Argus are documented here.

## [0.3.0](https://github.com/Meru143/argus/compare/argus-ai-v0.2.2...argus-ai-v0.3.0) - 2026-02-16

### Added

- implement learning from feedback (argus feedback)
- add PHP, Kotlin, Swift tree-sitter support (9→12 languages)
- implement incremental review (--incremental)
- add PR description generation (argus describe)
- self-reflection FP filtering, indicatif progress bars, 4 new languages ([#2](https://github.com/Meru143/argus/pull/2))

### Fixed

- install argus from crates.io instead of missing binary release

### Other

- add release-plz workflow for auto-publishing

## [0.2.2] — 2026-02-16

### Added
- **Welcome screen** — `argus` with no args now shows a branded welcome screen with quick start commands
- **Contextual error hints** — user-friendly hints for common errors (missing config, no API key, not in git repo)
- **Two-tier help** — `argus -h` for brief summary, `argus --help` for full details

## [0.2.0] — 2026-02-16

### Added
- **Summary generation** — reviews now produce a high-level summary paragraph (overall risk, key themes, areas of concern)
- **Suggested patches** — each review comment includes a concrete unified diff fix
- **SARIF output** — `--format sarif` for GitHub Code Scanning integration
- **Shell completions** — `argus completions bash|zsh|fish|powershell`
- **`argus doctor`** — environment diagnostics (API keys, config, providers)
- **`--color` flag** — explicit color control (`auto|always|never`), respects `NO_COLOR`
- **Cross-file analysis** — reviews analyze related files beyond the diff for better context
- **Custom review rules** — define project-specific rules in `.argus.toml`
- **Real-time progress** — streaming output during review so you're not staring at a blank terminal
- **`--show-filtered`** — debug noise reduction by seeing which comments were filtered and why

### Changed
- Bumped workspace version to 0.2.0
- Added CI workflow (test + clippy + fmt)
- Added Cargo.toml metadata (description, repository, keywords, categories)

## [0.1.1] — 2026-02-15

### Added
- **Gemini LLM provider** — use Gemini for reviews (free tier available)
- **Anthropic provider** — Claude support via Messages API
- **Multi-provider embeddings** — Voyage, Gemini, and OpenAI embedding support
- **Thinking model support** — handle thinking blocks in Anthropic responses
- **Dimension tracking** — multi-provider safety for embedding indices
- **npm package** — `npx argus-ai` / `npm install -g argus-ai`
- **Zero-cost setup** — Gemini for both LLM and embeddings with a free API key

### Fixed
- Fail early on missing API key with clear error message
- Redact API keys from error output
- Handle corrupted dimension metadata gracefully
- Validate model-provider compatibility on embedding init

## [0.1.0] — 2026-02-14

### Added
- **`argus review`** — AI-powered code review with context from all modules
- **`argus map`** — structural codebase overview (tree-sitter + PageRank)
- **`argus diff`** — risk scoring for code changes
- **`argus search`** — semantic + keyword hybrid code search
- **`argus history`** — hotspot detection, temporal coupling, bus factor analysis
- **`argus mcp`** — MCP server for IDE integration (Cursor, Windsurf, Claude Code)
- **`argus init`** — generate `.argus.toml` with sensible defaults
- **GitHub PR integration** — `--pr owner/repo#42`, `--post-comments`, `--fail-on`
- **GitHub Action** — automated review on pull requests
- **Release workflow** — cross-platform binaries (Linux, macOS, Windows)
- **Noise reduction** — pre-LLM file filtering, complexity scoring, deduplication
- **Anti-hallucination prompts** — strict rules to prevent fabricated line numbers
