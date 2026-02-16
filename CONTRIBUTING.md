# Contributing to Argus

Thanks for your interest in contributing to Argus!

## Getting Started

```bash
git clone https://github.com/Meru143/argus.git
cd argus
cargo build --workspace
cargo test --workspace
```

## Development

### Prerequisites
- Rust stable (latest)
- Git

### Workflow

1. Fork the repo and create a feature branch: `git checkout -b feat/my-feature`
2. Make your changes and add tests
3. Run checks:
   ```bash
   cargo fmt --all
   cargo clippy --workspace -- -D warnings
   cargo test --workspace
   ```
4. Commit with [conventional commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `test:`, `chore:`
5. Open a pull request against `main`

### Project Structure

```
crates/
  argus-core/      — shared types, config, errors
  argus-repomap/   — tree-sitter + PageRank codebase mapping
  argus-difflens/  — diff parsing + risk scoring
  argus-codelens/  — semantic search + AST chunking
  argus-gitpulse/  — git history analysis
  argus-review/    — LLM review pipeline
  argus-mcp/       — MCP server
src/main.rs        — CLI binary
```

Crate dependency order: `core` → `repomap/difflens/gitpulse/codelens` → `review` → `mcp`

### Testing

Tests live alongside the code they test. Integration tests are in `tests/`.

```bash
# Run all tests
cargo test --workspace

# Run a specific crate's tests
cargo test -p argus-review
```

## Reporting Issues

- Use GitHub Issues
- Include: what you did, what you expected, what happened, and your OS/Rust version
- For review quality issues, include the diff you reviewed (if possible)

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
