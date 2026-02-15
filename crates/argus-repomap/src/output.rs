use std::collections::BTreeMap;
use std::fmt::Write;

use argus_core::ArgusError;
use serde::Serialize;

use crate::graph::SymbolNode;
use crate::parser::SymbolKind;

/// JSON-serializable representation of a symbol for output.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SymbolOutput {
    name: String,
    kind: String,
    file: String,
    line: u32,
    signature: String,
    rank: f64,
    token_cost: usize,
}

/// Generate an ASCII tree representation of the repo map.
///
/// Groups symbols by file (alphabetically), within each file orders by line
/// number, and uses box-drawing characters for the tree structure.
///
/// # Examples
///
/// ```
/// use argus_repomap::output::format_tree;
///
/// let output = format_tree(&[]);
/// assert!(output.is_empty());
/// ```
pub fn format_tree(symbols: &[&SymbolNode]) -> String {
    if symbols.is_empty() {
        return String::new();
    }

    // Group symbols by file path
    let mut by_file: BTreeMap<String, Vec<&SymbolNode>> = BTreeMap::new();
    for sym in symbols {
        let key = sym.symbol.file.display().to_string();
        by_file.entry(key).or_default().push(sym);
    }

    // Sort within each file by line number
    for group in by_file.values_mut() {
        group.sort_by_key(|s| s.symbol.line);
    }

    let mut out = String::new();
    let file_count = by_file.len();

    for (file_idx, (file_path, file_symbols)) in by_file.iter().enumerate() {
        let is_last_file = file_idx == file_count - 1;
        let file_prefix = if is_last_file {
            "\u{2514}\u{2500}\u{2500} "
        } else {
            "\u{251c}\u{2500}\u{2500} "
        };
        let _ = writeln!(out, "{file_prefix}{file_path}");

        let child_prefix = if is_last_file { "    " } else { "\u{2502}   " };
        let sym_count = file_symbols.len();

        for (sym_idx, sym) in file_symbols.iter().enumerate() {
            let is_last_sym = sym_idx == sym_count - 1;
            let sym_prefix = if is_last_sym {
                "\u{2514}\u{2500}\u{2500} "
            } else {
                "\u{251c}\u{2500}\u{2500} "
            };

            let kind_label = kind_label(sym.symbol.kind);
            let sig = truncate_signature(&sym.symbol.signature, 80);

            let _ = writeln!(out, "{child_prefix}{sym_prefix}{kind_label} {sig}");
        }
    }

    out
}

/// Generate JSON output for the symbols.
///
/// # Errors
///
/// Returns [`ArgusError::Serialization`] if serialization fails.
///
/// # Examples
///
/// ```
/// use argus_repomap::output::format_json;
///
/// let json = format_json(&[]).unwrap();
/// assert!(json.contains("[]"));
/// ```
pub fn format_json(symbols: &[&SymbolNode]) -> Result<String, ArgusError> {
    let output: Vec<SymbolOutput> = symbols
        .iter()
        .map(|s| SymbolOutput {
            name: s.symbol.name.clone(),
            kind: format!("{:?}", s.symbol.kind),
            file: s.symbol.file.display().to_string(),
            line: s.symbol.line,
            signature: s.symbol.signature.clone(),
            rank: s.rank,
            token_cost: s.symbol.token_cost,
        })
        .collect();

    serde_json::to_string_pretty(&output).map_err(ArgusError::from)
}

/// Generate Markdown output for the symbols.
///
/// # Examples
///
/// ```
/// use argus_repomap::output::format_markdown;
///
/// let md = format_markdown(&[]);
/// assert!(md.is_empty());
/// ```
pub fn format_markdown(symbols: &[&SymbolNode]) -> String {
    if symbols.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("# Repository Map\n\n");

    // Group symbols by file
    let mut by_file: BTreeMap<String, Vec<&SymbolNode>> = BTreeMap::new();
    for sym in symbols {
        let key = sym.symbol.file.display().to_string();
        by_file.entry(key).or_default().push(sym);
    }

    for group in by_file.values_mut() {
        group.sort_by_key(|s| s.symbol.line);
    }

    for (file_path, file_symbols) in &by_file {
        let _ = writeln!(out, "## `{file_path}`\n");
        for sym in file_symbols {
            let kind = kind_label(sym.symbol.kind);
            let _ = writeln!(out, "- **{kind}** `{}`", sym.symbol.signature);
        }
        out.push('\n');
    }

    out
}

fn kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "fn",
        SymbolKind::Method => "method",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Impl => "impl",
        SymbolKind::Class => "class",
        SymbolKind::Interface => "interface",
        SymbolKind::Module => "mod",
    }
}

fn truncate_signature(sig: &str, max_len: usize) -> &str {
    if sig.len() <= max_len {
        return sig;
    }
    // Find a safe UTF-8 boundary
    let mut end = max_len;
    while end > 0 && !sig.is_char_boundary(end) {
        end -= 1;
    }
    &sig[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{Symbol, SymbolKind};
    use std::path::PathBuf;

    fn make_node(name: &str, file: &str, line: u32, kind: SymbolKind) -> SymbolNode {
        SymbolNode {
            symbol: Symbol {
                name: name.to_string(),
                kind,
                file: PathBuf::from(file),
                line,
                signature: format!("fn {name}()"),
                token_cost: 5,
            },
            rank: 1.0,
        }
    }

    #[test]
    fn format_tree_multiple_files() {
        let nodes = vec![
            make_node("main", "src/main.rs", 1, SymbolKind::Function),
            make_node("Config", "src/config.rs", 1, SymbolKind::Struct),
            make_node("from_file", "src/config.rs", 10, SymbolKind::Function),
        ];
        let refs: Vec<&SymbolNode> = nodes.iter().collect();

        let tree = format_tree(&refs);
        assert!(tree.contains("src/config.rs"), "should contain config.rs");
        assert!(tree.contains("src/main.rs"), "should contain main.rs");
        assert!(tree.contains("fn fn main()"), "should contain main symbol");
        assert!(tree.contains("struct fn Config()"), "should contain Config");
    }

    #[test]
    fn format_tree_single_file() {
        let nodes = vec![make_node("run", "app.rs", 1, SymbolKind::Function)];
        let refs: Vec<&SymbolNode> = nodes.iter().collect();

        let tree = format_tree(&refs);
        assert!(tree.contains("app.rs"));
        assert!(tree.contains("fn fn run()"));
    }

    #[test]
    fn format_tree_empty() {
        let tree = format_tree(&[]);
        assert!(tree.is_empty());
    }

    #[test]
    fn format_json_output() {
        let nodes = vec![make_node("test", "t.rs", 1, SymbolKind::Function)];
        let refs: Vec<&SymbolNode> = nodes.iter().collect();

        let json = format_json(&refs).unwrap();
        assert!(json.contains("\"name\": \"test\""));
        assert!(json.contains("\"kind\": \"Function\""));
        // Verify valid JSON
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn format_markdown_output() {
        let nodes = vec![
            make_node("main", "src/main.rs", 1, SymbolKind::Function),
            make_node("Config", "src/config.rs", 1, SymbolKind::Struct),
        ];
        let refs: Vec<&SymbolNode> = nodes.iter().collect();

        let md = format_markdown(&refs);
        assert!(md.contains("# Repository Map"));
        assert!(md.contains("## `src/config.rs`"));
        assert!(md.contains("**struct**"));
    }
}
