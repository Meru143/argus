# Argus

> **Your coding agent shouldn't grade its own homework.**

[![CI](https://github.com/Meru143/argus/actions/workflows/ci.yml/badge.svg)](https://github.com/Meru143/argus/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/argus-ai.svg)](https://crates.io/crates/argus-ai)
[![npm version](https://img.shields.io/npm/v/argus-ai.svg)](https://www.npmjs.com/package/argus-ai)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Argus** is a local-first, modular AI code review platform. It combines structural analysis, semantic search, git history intelligence, and LLM-powered reviews to catch what your copilot misses.

---

## ⚡️ The "Aha!" Moment

Most AI coding tools just generate code. Argus **audits** it.

```text
$ argus review --repo .

🔍 Analyzing 3 files...
  ├── src/auth.rs (Modified)
  ├── src/user.rs (Modified)
  └── src/main.rs (Context)

⚠️  HIGH RISK DETECTED in src/auth.rs
    • Hotspot: Top 5% most churned file
    • Bus Factor: Only 1 developer has touched this in 6 months

[src/auth.rs:42] Security Warning
> unsafe { verify_token(token) }
  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  "This `unsafe` block bypasses signature verification if the token is malformed.
   Use `verify_token_safe` instead, or wrap in a proper `Result` check."

✨ Suggestion:
- unsafe { verify_token(token) }
+ verify_token_safe(token)?
```

---

## 🚀 Why Argus?

| Feature | Argus | GitHub Copilot | Linters (Clippy/ESLint) |
| :--- | :---: | :---: | :---: |
| **Review Context** | **Deep** (Map + History + Search) | Shallow (Diff only) | None (Syntax only) |
| **Feedback Loop** | ✅ **Learns your style** | ❌ Static | ❌ Static |
| **Self-Correction** | ✅ **Validation Step** | ❌ Generates & forgets | N/A |
| **Privacy** | ✅ **Local / Zero-Retention** | ⚠️ Cloud-based | ✅ Local |
| **Cost** | 💸 **Free Tier (Gemini)** | 💰 Monthly Sub | 💸 Free |

### New in v0.4.0
- **Feedback Loop:** `argus feedback` lets you rate comments. Argus learns to stop nagging about style and focus on bugs.
- **12+ Languages:** Rust, Python, TypeScript, JavaScript, Go, Java, C, C++, Ruby, PHP, Kotlin, Swift.
- **Hotspot Awareness:** Prioritizes reviews on "high-churn" files that break often.

---

## 📦 Install

### Recommended (npm)
The fastest way to try Argus. Zero compiled dependencies.
```bash
npx argus-ai init     # Generates .argus.toml
npx argus-ai review   # Runs a review
```

### macOS / Linux (Homebrew)
```bash
brew tap Meru143/argus
brew install argus
```

### Rust (Cargo)
```bash
cargo install argus-ai
```

---

## 🛠 Usage

### 1. Code Review (`review`)
The core workflow. Reviews your staged changes or a specific branch.

```bash
# Review local changes vs main
git diff main | argus review

# Review a PR (and post comments to GitHub)
argus review --pr owner/repo#42 --post-comments
```

### 2. Feedback Loop (`feedback`)
Teach Argus what you like. Run this after a review to rate the comments.

```bash
argus feedback
# Output:
# [1/5] "Consider using Arc instead of Rc..."
# (y) Useful  (n) Noise  (s) Skip
```

### 3. PR Descriptions (`describe`)
Stop writing boilerplate. Generate semantic PR descriptions from your code changes.

```bash
argus describe --pr owner/repo#42
```

### 4. Deep Analysis (`map`, `history`)
Understand your codebase structure and risks.

```bash
# Visualize module structure & page rank
argus map --limit 20

# Find "bus factor" risks (files only one person touches)
argus history --analysis hotspots
```

---

## ⚙️ Configuration

Run `argus init` to generate a `.argus.toml`.

```toml
[review]
min_confidence = "high"   # low, medium, high
language = "en"           # Review language
skip_patterns = ["*.lock", "dist/**"]

[feedback]
learning_rate = 0.1       # How fast Argus adapts

[llm]
provider = "gemini"       # gemini, openai, anthropic, ollama
model = "gemini-2.0-flash"
```

**Free Tier:** Argus defaults to Gemini (Flash/Pro), which has a generous free tier suitable for most daily review workflows.

---

## 🧩 Integrations

### MCP (Claude Code, Cursor, Windsurf)
Argus runs as an **MCP Server**, giving your IDE agent access to its deep analysis tools.

**Claude Code:**
Add to `~/.mcp.json`:
```json
{
  "mcpServers": {
    "argus": { "command": "argus", "args": ["mcp", "--path", "/path/to/repo"] }
  }
}
```

---

## 🏗 Architecture

Argus is a **Unix-style** pipeline of six specialized tools:

1.  **`repomap`**: Builds a compact tree-sitter map of your code.
2.  **`difflens`**: Analyzes the raw diff for syntactic changes.
3.  **`gitpulse`**: Mining git history for hotspots and coupling.
4.  **`codelens`**: Semantic search and cross-reference resolution.
5.  **`review`**: The LLM brain that synthesizes all inputs.
6.  **`mcp`**: The bridge to external agents.

---

## License

MIT © [Meru Patel](https://github.com/Meru143)
