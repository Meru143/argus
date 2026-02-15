# argus-ai

AI-powered code review tool — multi-provider, self-hosted, open-source.

```bash
npx argus-ai review --file my.diff
```

## What is Argus?

Argus is an AI code review tool built in Rust. It parses diffs, scores risk, builds repository context, and sends intelligent review requests to your LLM of choice.

- **Multi-provider LLM**: OpenAI, Anthropic, Gemini
- **Multi-provider embeddings**: Voyage, Gemini, OpenAI
- **Zero-cost path**: Use Gemini for both LLM and embeddings (free tier, no credit card)
- **MCP server**: 5 tools for IDE integration
- **GitHub integration**: Inline PR comments, CI gating

## Quick Start

```bash
# Review a diff
git diff HEAD~3 | npx argus-ai review --file -

# Review with repo context
npx argus-ai review --file my.diff --repo .

# Generate repo map
npx argus-ai map --path .

# Analyze git history
npx argus-ai history --path . --analysis hotspots
```

## Configuration

```bash
npx argus-ai init  # creates .argus.toml
```

See the [full documentation](https://github.com/Meru143/argus) for provider setup.

## License

MIT
