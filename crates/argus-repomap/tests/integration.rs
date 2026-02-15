//! Integration test: walk → parse → rank → format on the argus repo itself.

use std::path::Path;

use argus_core::OutputFormat;

#[test]
fn end_to_end_on_argus_repo() {
    // Use the argus repo root (two levels up from this crate)
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    // Step 1: Walk
    let files = argus_repomap::walker::walk_repo(repo_root).unwrap();
    assert!(
        !files.is_empty(),
        "should find source files in the argus repo"
    );

    // Should find Rust files at minimum
    let rust_count = files
        .iter()
        .filter(|f| f.language == argus_repomap::walker::Language::Rust)
        .count();
    assert!(
        rust_count > 5,
        "should find multiple Rust files: {rust_count}"
    );

    // Step 2: Parse all files
    let mut all_symbols = Vec::new();
    let mut all_references = Vec::new();
    for file in &files {
        let symbols = argus_repomap::parser::extract_symbols(file).unwrap();
        let references = argus_repomap::parser::extract_references(file).unwrap();
        all_symbols.extend(symbols);
        all_references.extend(references);
    }
    assert!(
        all_symbols.len() > 10,
        "should extract many symbols: {}",
        all_symbols.len()
    );

    // Verify known symbols exist
    let names: Vec<&str> = all_symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"ArgusError"),
        "should find ArgusError: first 20 names = {:?}",
        &names[..names.len().min(20)]
    );

    // Step 3: Build graph and rank
    let mut graph = argus_repomap::graph::SymbolGraph::build(all_symbols, all_references);
    graph.compute_pagerank();
    let ranked = graph.ranked_symbols();
    assert!(!ranked.is_empty());
    assert!(ranked[0].rank > 0.0, "top symbol should have positive rank");

    // Step 4: Budget
    let selected = argus_repomap::budget::fit_to_budget(&ranked, 500);
    assert!(
        !selected.is_empty(),
        "should select some symbols within budget"
    );

    // Step 5: Format outputs
    let tree = argus_repomap::output::format_tree(&selected);
    assert!(!tree.is_empty(), "tree output should not be empty");
    assert!(tree.contains(".rs"), "tree should contain .rs file paths");

    let json = argus_repomap::output::format_json(&selected).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_array(), "JSON should be an array");

    let md = argus_repomap::output::format_markdown(&selected);
    assert!(md.contains("# Repository Map"));

    // Step 6: Use the generate_map convenience function
    let map = argus_repomap::generate_map(repo_root, 500, &[], OutputFormat::Text).unwrap();
    assert!(!map.is_empty());
}

#[test]
fn generate_map_with_focus_files() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let focus = vec![std::path::PathBuf::from("src/main.rs")];
    let map = argus_repomap::generate_map(repo_root, 500, &focus, OutputFormat::Text).unwrap();
    assert!(!map.is_empty());
}

#[test]
fn generate_map_json_is_valid() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let json = argus_repomap::generate_map(repo_root, 500, &[], OutputFormat::Json).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_array());
}
