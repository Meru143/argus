//! Repository structure mapping via tree-sitter and PageRank ranking.
//!
//! Generates a compressed, ranked map of codebase symbols (classes, functions,
//! signatures) optimized for LLM token efficiency. Uses tree-sitter for AST
//! parsing, petgraph for PageRank, and the `ignore` crate for file walking.

pub mod budget;
pub mod graph;
pub mod output;
pub mod parser;
pub mod walker;

use std::path::{Path, PathBuf};

use argus_core::{ArgusError, OutputFormat};

/// Generate a ranked map of the codebase at `root`.
///
/// Walks the repository, parses source files, builds a symbol graph, runs
/// PageRank, fits to the token budget, and formats the output.
///
/// # Errors
///
/// Returns [`ArgusError`] if file walking or parsing fails.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use argus_core::OutputFormat;
/// use argus_repomap::generate_map;
///
/// let map = generate_map(Path::new("."), 1024, &[], OutputFormat::Text).unwrap();
/// println!("{map}");
/// ```
pub fn generate_map(
    root: &Path,
    max_tokens: usize,
    focus_files: &[PathBuf],
    format: OutputFormat,
) -> Result<String, ArgusError> {
    let files = walker::walk_repo(root)?;

    let mut all_symbols = Vec::new();
    let mut all_references = Vec::new();

    for file in &files {
        let symbols = parser::extract_symbols(file)?;
        let references = parser::extract_references(file)?;
        all_symbols.extend(symbols);
        all_references.extend(references);
    }

    let mut symbol_graph = graph::SymbolGraph::build(all_symbols, all_references);
    symbol_graph.compute_pagerank();

    let ranked = if focus_files.is_empty() {
        symbol_graph.ranked_symbols()
    } else {
        symbol_graph.ranked_symbols_for_files(focus_files)
    };

    let selected = budget::fit_to_budget(&ranked, max_tokens);

    match format {
        OutputFormat::Text => Ok(output::format_tree(&selected)),
        OutputFormat::Json => output::format_json(&selected),
        OutputFormat::Markdown => Ok(output::format_markdown(&selected)),
    }
}
