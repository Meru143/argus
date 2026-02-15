//! Repository structure mapping via tree-sitter and PageRank ranking.
//!
//! Generates a compressed, ranked map of codebase symbols (classes, functions,
//! signatures) optimized for LLM token efficiency. Uses tree-sitter for AST
//! parsing, petgraph for PageRank, and the `ignore` crate for file walking.
