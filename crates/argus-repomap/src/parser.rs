use std::path::PathBuf;

use argus_core::ArgusError;
use tree_sitter::{Node, Parser};

use crate::walker::{Language, SourceFile};

/// A symbol extracted from source code via tree-sitter.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use argus_repomap::parser::{Symbol, SymbolKind};
///
/// let sym = Symbol {
///     name: "main".into(),
///     kind: SymbolKind::Function,
///     file: PathBuf::from("src/main.rs"),
///     line: 1,
///     signature: "fn main()".into(),
///     token_cost: 2,
/// };
/// assert_eq!(sym.kind, SymbolKind::Function);
/// ```
#[derive(Debug, Clone)]
pub struct Symbol {
    /// Symbol name (e.g. function name, struct name).
    pub name: String,
    /// What kind of symbol this is.
    pub kind: SymbolKind,
    /// File path (relative to repo root).
    pub file: PathBuf,
    /// Line number where the symbol starts (1-indexed).
    pub line: u32,
    /// Human-readable signature (e.g. `fn process(input: &str) -> Result<Output>`).
    pub signature: String,
    /// Estimated token cost for including this symbol in context.
    pub token_cost: usize,
}

/// Classification of extracted symbols.
///
/// # Examples
///
/// ```
/// use argus_repomap::parser::SymbolKind;
///
/// let kind = SymbolKind::Function;
/// assert_eq!(format!("{kind:?}"), "Function");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Impl,
    Class,
    Interface,
    Module,
}

/// A reference from one symbol to another.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use argus_repomap::parser::Reference;
///
/// let reference = Reference {
///     from_file: PathBuf::from("src/main.rs"),
///     from_symbol: Some("main".into()),
///     to_name: "Config".into(),
///     line: 5,
/// };
/// assert_eq!(reference.to_name, "Config");
/// ```
#[derive(Debug, Clone)]
pub struct Reference {
    /// File containing the reference.
    pub from_file: PathBuf,
    /// Enclosing symbol name, if any.
    pub from_symbol: Option<String>,
    /// Referenced identifier name.
    pub to_name: String,
    /// Line where the reference occurs.
    pub line: u32,
}

/// Extract all symbols from a source file using tree-sitter.
///
/// Returns an empty vec for unparseable files. Tree-sitter is error-tolerant,
/// so partial results are returned even for files with syntax errors.
///
/// # Errors
///
/// Returns [`ArgusError::Parse`] if the language grammar cannot be loaded.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use argus_repomap::walker::{Language, SourceFile};
/// use argus_repomap::parser::extract_symbols;
///
/// let file = SourceFile {
///     path: PathBuf::from("example.rs"),
///     language: Language::Rust,
///     content: "fn hello() {}".to_string(),
/// };
/// let symbols = extract_symbols(&file).unwrap();
/// assert_eq!(symbols.len(), 1);
/// assert_eq!(symbols[0].name, "hello");
/// ```
pub fn extract_symbols(file: &SourceFile) -> Result<Vec<Symbol>, ArgusError> {
    let Some(ts_language) = file.language.tree_sitter_language() else {
        return Ok(Vec::new());
    };

    let mut parser = Parser::new();
    parser
        .set_language(&ts_language)
        .map_err(|e| ArgusError::Parse(format!("failed to set language: {e}")))?;

    let Some(tree) = parser.parse(&file.content, None) else {
        return Ok(Vec::new());
    };

    let mut symbols = Vec::new();
    let source = file.content.as_bytes();
    collect_symbols(
        tree.root_node(),
        source,
        &file.path,
        file.language,
        false,
        &mut symbols,
    );

    Ok(symbols)
}

/// Extract references (identifiers referring to other symbols) from a source file.
///
/// # Errors
///
/// Returns [`ArgusError::Parse`] if the language grammar cannot be loaded.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use argus_repomap::walker::{Language, SourceFile};
/// use argus_repomap::parser::extract_references;
///
/// let file = SourceFile {
///     path: PathBuf::from("example.rs"),
///     language: Language::Rust,
///     content: "fn main() { hello(); }".to_string(),
/// };
/// let refs = extract_references(&file).unwrap();
/// assert!(refs.iter().any(|r| r.to_name == "hello"));
/// ```
pub fn extract_references(file: &SourceFile) -> Result<Vec<Reference>, ArgusError> {
    let Some(ts_language) = file.language.tree_sitter_language() else {
        return Ok(Vec::new());
    };

    let mut parser = Parser::new();
    parser
        .set_language(&ts_language)
        .map_err(|e| ArgusError::Parse(format!("failed to set language: {e}")))?;

    let Some(tree) = parser.parse(&file.content, None) else {
        return Ok(Vec::new());
    };

    let mut refs = Vec::new();
    collect_references(
        tree.root_node(),
        file.content.as_bytes(),
        &file.path,
        &None,
        &mut refs,
    );

    Ok(refs)
}

fn collect_symbols(
    node: Node,
    source: &[u8],
    file: &PathBuf,
    language: Language,
    inside_impl: bool,
    symbols: &mut Vec<Symbol>,
) {
    match language {
        Language::Rust => collect_rust_symbols(node, source, file, inside_impl, symbols),
        Language::Python => collect_python_symbols(node, source, file, false, symbols),
        Language::TypeScript | Language::JavaScript => {
            collect_js_ts_symbols(node, source, file, false, symbols);
        }
        Language::Go => collect_go_symbols(node, source, file, symbols),
        Language::Java => collect_java_symbols(node, source, file, false, symbols),
        Language::C => collect_c_symbols(node, source, file, symbols),
        Language::Cpp => collect_cpp_symbols(node, source, file, false, symbols),
        Language::Ruby => collect_ruby_symbols(node, source, file, false, symbols),
        Language::Php => collect_php_symbols(node, source, file, false, symbols),
        Language::Kotlin => collect_kotlin_symbols(node, source, file, false, symbols),
        Language::Swift => collect_swift_symbols(node, source, file, false, symbols),
        Language::Unknown => {}
    }
}

fn collect_rust_symbols(
    node: Node,
    source: &[u8],
    file: &PathBuf,
    inside_impl: bool,
    symbols: &mut Vec<Symbol>,
) {
    let kind_str = node.kind();

    match kind_str {
        "function_item" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let sig = extract_signature(&node, source);
                let kind = if inside_impl {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                symbols.push(Symbol {
                    name,
                    kind,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "struct_item" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Struct,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "enum_item" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Enum,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "trait_item" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Trait,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "impl_item" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name: name.clone(),
                    kind: SymbolKind::Impl,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            // Recurse into impl body to find methods
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_rust_symbols(child, source, file, true, symbols);
            }
            return; // Don't recurse again below
        }
        _ => {}
    }

    // Recurse into children (except for impl which we already handled)
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_rust_symbols(child, source, file, inside_impl, symbols);
    }
}

fn collect_python_symbols(
    node: Node,
    source: &[u8],
    file: &PathBuf,
    inside_class: bool,
    symbols: &mut Vec<Symbol>,
) {
    let kind_str = node.kind();

    match kind_str {
        "function_definition" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let sig = extract_signature(&node, source);
                let kind = if inside_class {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                symbols.push(Symbol {
                    name,
                    kind,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "class_definition" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Class,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            // Recurse into class body to find methods
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_python_symbols(child, source, file, true, symbols);
            }
            return;
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_python_symbols(child, source, file, inside_class, symbols);
    }
}

fn collect_js_ts_symbols(
    node: Node,
    source: &[u8],
    file: &PathBuf,
    inside_class: bool,
    symbols: &mut Vec<Symbol>,
) {
    let kind_str = node.kind();

    match kind_str {
        "function_declaration" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Function,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "class_declaration" => {
            let name = find_child_text(&node, "type_identifier", source)
                .or_else(|| find_child_text(&node, "identifier", source));
            if let Some(name) = name {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Class,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_js_ts_symbols(child, source, file, true, symbols);
            }
            return;
        }
        "method_definition" => {
            if let Some(name) = find_child_text(&node, "property_identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Method,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "lexical_declaration" => {
            // Arrow functions assigned to const: const foo = () => {}
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "variable_declarator" {
                    let has_arrow = child_has_kind(&child, "arrow_function");
                    if has_arrow {
                        if let Some(name) = find_child_text(&child, "identifier", source) {
                            let sig = extract_signature(&node, source);
                            symbols.push(Symbol {
                                name,
                                kind: SymbolKind::Function,
                                file: file.clone(),
                                line: node.start_position().row as u32 + 1,
                                token_cost: sig.len() / 4,
                                signature: sig,
                            });
                        }
                    }
                }
            }
        }
        _ => {}
    }

    if !inside_class || kind_str != "class_declaration" {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            collect_js_ts_symbols(child, source, file, inside_class, symbols);
        }
    }
}

fn collect_go_symbols(node: Node, source: &[u8], file: &PathBuf, symbols: &mut Vec<Symbol>) {
    let kind_str = node.kind();

    match kind_str {
        "function_declaration" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Function,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "method_declaration" => {
            if let Some(name) = find_child_text(&node, "field_identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Method,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "type_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "type_spec" {
                    if let Some(name) = find_child_text(&child, "type_identifier", source) {
                        let has_struct = child_has_kind(&child, "struct_type");
                        let has_interface = child_has_kind(&child, "interface_type");
                        let kind = if has_struct {
                            SymbolKind::Struct
                        } else if has_interface {
                            SymbolKind::Interface
                        } else {
                            continue;
                        };
                        let sig = extract_signature(&child, source);
                        symbols.push(Symbol {
                            name,
                            kind,
                            file: file.clone(),
                            line: child.start_position().row as u32 + 1,
                            token_cost: sig.len() / 4,
                            signature: sig,
                        });
                    }
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_go_symbols(child, source, file, symbols);
    }
}

fn collect_java_symbols(
    node: Node,
    source: &[u8],
    file: &PathBuf,
    inside_class: bool,
    symbols: &mut Vec<Symbol>,
) {
    let kind_str = node.kind();

    match kind_str {
        "method_declaration" | "constructor_declaration" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let sig = extract_signature(&node, source);
                let kind = if inside_class {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                symbols.push(Symbol {
                    name,
                    kind,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "class_declaration" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Class,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_java_symbols(child, source, file, true, symbols);
            }
            return;
        }
        "interface_declaration" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Interface,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_java_symbols(child, source, file, true, symbols);
            }
            return;
        }
        "enum_declaration" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Enum,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_java_symbols(child, source, file, inside_class, symbols);
    }
}

fn collect_c_symbols(node: Node, source: &[u8], file: &PathBuf, symbols: &mut Vec<Symbol>) {
    let kind_str = node.kind();

    match kind_str {
        "function_definition" | "declaration" => {
            // For declarations, only match function declarations (with function_declarator)
            if kind_str == "declaration" {
                let has_func = child_has_kind(&node, "function_declarator");
                if !has_func {
                    // Skip non-function declarations, but recurse
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        collect_c_symbols(child, source, file, symbols);
                    }
                    return;
                }
            }
            // Find the function name via function_declarator -> identifier
            if let Some(name) = find_nested_function_name(&node, source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Function,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "struct_specifier" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Struct,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "enum_specifier" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Enum,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_c_symbols(child, source, file, symbols);
    }
}

fn collect_cpp_symbols(
    node: Node,
    source: &[u8],
    file: &PathBuf,
    inside_class: bool,
    symbols: &mut Vec<Symbol>,
) {
    let kind_str = node.kind();

    match kind_str {
        "function_definition" => {
            if let Some(name) = find_nested_function_name(&node, source)
                .or_else(|| find_child_text(&node, "identifier", source))
            {
                let sig = extract_signature(&node, source);
                let kind = if inside_class {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                symbols.push(Symbol {
                    name,
                    kind,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "class_specifier" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Class,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_cpp_symbols(child, source, file, true, symbols);
            }
            return;
        }
        "struct_specifier" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Struct,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "enum_specifier" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Enum,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_cpp_symbols(child, source, file, inside_class, symbols);
    }
}

fn collect_ruby_symbols(
    node: Node,
    source: &[u8],
    file: &PathBuf,
    inside_class: bool,
    symbols: &mut Vec<Symbol>,
) {
    let kind_str = node.kind();

    match kind_str {
        "method" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let sig = extract_signature(&node, source);
                let kind = if inside_class {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                symbols.push(Symbol {
                    name,
                    kind,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "class" => {
            // Ruby class names can be constant or scope_resolution
            let name = find_child_text(&node, "constant", source)
                .or_else(|| find_child_text(&node, "scope_resolution", source));
            if let Some(name) = name {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Class,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_ruby_symbols(child, source, file, true, symbols);
            }
            return;
        }
        "module" => {
            if let Some(name) = find_child_text(&node, "constant", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Module,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_ruby_symbols(child, source, file, true, symbols);
            }
            return;
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_ruby_symbols(child, source, file, inside_class, symbols);
    }
}

fn collect_php_symbols(
    node: Node,
    source: &[u8],
    file: &PathBuf,
    inside_class: bool,
    symbols: &mut Vec<Symbol>,
) {
    let kind_str = node.kind();

    match kind_str {
        "function_definition" | "method_declaration" => {
            if let Some(name) = find_child_text(&node, "name", source) {
                let sig = extract_signature(&node, source);
                let kind = if inside_class || kind_str == "method_declaration" {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                symbols.push(Symbol {
                    name,
                    kind,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "class_declaration" | "interface_declaration" | "trait_declaration" => {
            if let Some(name) = find_child_text(&node, "name", source) {
                let sig = extract_signature(&node, source);
                let kind = match kind_str {
                    "interface_declaration" => SymbolKind::Interface,
                    "trait_declaration" => SymbolKind::Trait,
                    _ => SymbolKind::Class,
                };
                symbols.push(Symbol {
                    name,
                    kind,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_php_symbols(child, source, file, true, symbols);
            }
            return;
        }
        "namespace_definition" => {
            if let Some(name) = find_child_text(&node, "namespace_name", source)
                .or_else(|| find_child_text(&node, "name", source))
            {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Module,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_php_symbols(child, source, file, inside_class, symbols);
    }
}

fn collect_kotlin_symbols(
    node: Node,
    source: &[u8],
    file: &PathBuf,
    inside_class: bool,
    symbols: &mut Vec<Symbol>,
) {
    let kind_str = node.kind();

    match kind_str {
        "function_declaration" => {
            if let Some(name) = find_child_text(&node, "simple_identifier", source) {
                let sig = extract_signature(&node, source);
                let kind = if inside_class {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                symbols.push(Symbol {
                    name,
                    kind,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "class_declaration" | "object_declaration" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source)
                .or_else(|| find_child_text(&node, "simple_identifier", source))
            {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Class,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_kotlin_symbols(child, source, file, true, symbols);
            }
            return;
        }
        "interface_declaration" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Interface,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_kotlin_symbols(child, source, file, true, symbols);
            }
            return;
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_kotlin_symbols(child, source, file, inside_class, symbols);
    }
}

fn collect_swift_symbols(
    node: Node,
    source: &[u8],
    file: &PathBuf,
    inside_class: bool,
    symbols: &mut Vec<Symbol>,
) {
    let kind_str = node.kind();

    match kind_str {
        "function_declaration" => {
            if let Some(name) = find_child_text(&node, "simple_identifier", source) {
                let sig = extract_signature(&node, source);
                let kind = if inside_class {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                symbols.push(Symbol {
                    name,
                    kind,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
        }
        "class_declaration" | "struct_declaration" | "enum_declaration" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source)
                .or_else(|| find_child_text(&node, "simple_identifier", source))
            {
                let sig = extract_signature(&node, source);
                let kind = match kind_str {
                    "struct_declaration" => SymbolKind::Struct,
                    "enum_declaration" => SymbolKind::Enum,
                    _ => SymbolKind::Class,
                };
                symbols.push(Symbol {
                    name,
                    kind,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_swift_symbols(child, source, file, true, symbols);
            }
            return;
        }
        "protocol_declaration" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source)
                .or_else(|| find_child_text(&node, "simple_identifier", source))
            {
                let sig = extract_signature(&node, source);
                symbols.push(Symbol {
                    name,
                    kind: SymbolKind::Interface,
                    file: file.clone(),
                    line: node.start_position().row as u32 + 1,
                    token_cost: sig.len() / 4,
                    signature: sig,
                });
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_swift_symbols(child, source, file, true, symbols);
            }
            return;
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_swift_symbols(child, source, file, inside_class, symbols);
    }
}

/// Find the function name from a function_declarator child node.
///
/// In C/C++, function definitions have: type function_declarator(params) body
/// The declarator contains the identifier.
fn find_nested_function_name(node: &Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_declarator" {
            return find_child_text(&child, "identifier", source)
                .or_else(|| find_child_text(&child, "field_identifier", source));
        }
    }
    None
}

fn collect_references(
    node: Node,
    source: &[u8],
    file: &PathBuf,
    enclosing: &Option<String>,
    refs: &mut Vec<Reference>,
) {
    let kind_str = node.kind();

    // Track enclosing symbol for context
    let new_enclosing = match kind_str {
        "function_item" | "function_definition" | "function_declaration" | "method_declaration" => {
            find_child_text(&node, "identifier", source)
                .or_else(|| find_child_text(&node, "field_identifier", source))
        }
        _ => None,
    };
    let current_enclosing = if new_enclosing.is_some() {
        &new_enclosing
    } else {
        enclosing
    };

    // Collect identifier references (excluding definition sites)
    if kind_str == "identifier" || kind_str == "type_identifier" {
        let parent_kind = node.parent().map(|p| p.kind().to_string());
        let is_definition = matches!(
            parent_kind.as_deref(),
            Some(
                "function_item"
                    | "function_definition"
                    | "function_declaration"
                    | "struct_item"
                    | "enum_item"
                    | "trait_item"
                    | "class_definition"
                    | "class_declaration"
                    | "method_definition"
                    | "variable_declarator"
                    | "type_spec"
            )
        );

        if !is_definition {
            let name = node_text(&node, source);
            if !name.is_empty() {
                refs.push(Reference {
                    from_file: file.clone(),
                    from_symbol: current_enclosing.clone(),
                    to_name: name,
                    line: node.start_position().row as u32 + 1,
                });
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_references(child, source, file, current_enclosing, refs);
    }
}

/// Extract the signature of a node: text from start to opening `{` or `:`.
fn extract_signature(node: &Node, source: &[u8]) -> String {
    let text = node_text(node, source);

    // Find the opening brace or colon (for Python)
    let sig = if let Some(pos) = text.find('{') {
        &text[..pos]
    } else if let Some(pos) = text.find(':') {
        // Python uses : instead of {
        &text[..pos]
    } else {
        &text
    };

    // Collapse whitespace and trim
    let collapsed: String = sig.split_whitespace().collect::<Vec<_>>().join(" ");

    collapsed
}

fn node_text(node: &Node, source: &[u8]) -> String {
    let start = node.start_byte();
    let end = node.end_byte();
    if start >= source.len() || end > source.len() {
        return String::new();
    }
    String::from_utf8_lossy(&source[start..end]).to_string()
}

fn find_child_text(node: &Node, kind: &str, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            let text = node_text(&child, source);
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn child_has_kind(node: &Node, kind: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rust_file() -> SourceFile {
        SourceFile {
            path: PathBuf::from("src/lib.rs"),
            language: Language::Rust,
            content: r#"
pub fn top_level(x: i32) -> bool {
    x > 0
}

pub struct Config {
    name: String,
    value: u32,
}

pub enum Color {
    Red,
    Green,
    Blue,
}

pub trait Drawable {
    fn draw(&self);
}

impl Config {
    pub fn new(name: String) -> Self {
        Self { name, value: 0 }
    }
}
"#
            .to_string(),
        }
    }

    fn make_python_file() -> SourceFile {
        SourceFile {
            path: PathBuf::from("app.py"),
            language: Language::Python,
            content: r#"
def standalone():
    pass

class MyClass:
    def method(self):
        pass

    def another(self, x):
        return x + 1
"#
            .to_string(),
        }
    }

    fn make_typescript_file() -> SourceFile {
        SourceFile {
            path: PathBuf::from("app.ts"),
            language: Language::TypeScript,
            content: r#"
function greet(name: string): string {
    return `Hello ${name}`;
}

class Greeter {
    sayHello() {
        console.log("hello");
    }
}

const add = (a: number, b: number) => a + b;
"#
            .to_string(),
        }
    }

    #[test]
    fn parse_rust_symbols() {
        let file = make_rust_file();
        let symbols = extract_symbols(&file).unwrap();

        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"top_level"), "missing top_level: {names:?}");
        assert!(
            names.contains(&"Config"),
            "missing Config struct: {names:?}"
        );
        assert!(names.contains(&"Color"), "missing Color enum: {names:?}");
        assert!(
            names.contains(&"Drawable"),
            "missing Drawable trait: {names:?}"
        );
        assert!(names.contains(&"new"), "missing new method: {names:?}");

        // Check kinds
        let top = symbols.iter().find(|s| s.name == "top_level").unwrap();
        assert_eq!(top.kind, SymbolKind::Function);

        let config_struct = symbols
            .iter()
            .find(|s| s.name == "Config" && s.kind == SymbolKind::Struct)
            .unwrap();
        assert_eq!(config_struct.kind, SymbolKind::Struct);

        let new_method = symbols.iter().find(|s| s.name == "new").unwrap();
        assert_eq!(new_method.kind, SymbolKind::Method);

        // Check signatures have content
        assert!(top.signature.contains("fn top_level"));
        assert!(top.token_cost > 0);
    }

    #[test]
    fn parse_python_symbols() {
        let file = make_python_file();
        let symbols = extract_symbols(&file).unwrap();

        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"standalone"),
            "missing standalone: {names:?}"
        );
        assert!(names.contains(&"MyClass"), "missing MyClass: {names:?}");
        assert!(names.contains(&"method"), "missing method: {names:?}");
        assert!(names.contains(&"another"), "missing another: {names:?}");

        let standalone = symbols.iter().find(|s| s.name == "standalone").unwrap();
        assert_eq!(standalone.kind, SymbolKind::Function);

        let method = symbols.iter().find(|s| s.name == "method").unwrap();
        assert_eq!(method.kind, SymbolKind::Method);
    }

    #[test]
    fn parse_typescript_symbols() {
        let file = make_typescript_file();
        let symbols = extract_symbols(&file).unwrap();

        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"greet"), "missing greet: {names:?}");
        assert!(names.contains(&"Greeter"), "missing Greeter: {names:?}");
        assert!(names.contains(&"sayHello"), "missing sayHello: {names:?}");
        assert!(names.contains(&"add"), "missing add arrow fn: {names:?}");
    }

    #[test]
    fn parse_empty_file() {
        let file = SourceFile {
            path: PathBuf::from("empty.rs"),
            language: Language::Rust,
            content: String::new(),
        };
        let symbols = extract_symbols(&file).unwrap();
        assert!(symbols.is_empty());
    }

    #[test]
    fn parse_file_with_syntax_errors_gives_partial_results() {
        let file = SourceFile {
            path: PathBuf::from("broken.rs"),
            language: Language::Rust,
            content: r#"
fn valid_fn() -> bool { true }

fn broken( {

struct ValidStruct {
    x: i32,
}
"#
            .to_string(),
        };
        let symbols = extract_symbols(&file).unwrap();
        // tree-sitter is error-tolerant: we should still get valid_fn and ValidStruct
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"valid_fn"),
            "should extract valid symbols despite errors: {names:?}"
        );
    }

    #[test]
    fn extract_references_finds_calls() {
        let file = SourceFile {
            path: PathBuf::from("main.rs"),
            language: Language::Rust,
            content: r#"
fn caller() {
    helper();
    let x = Config::new();
}

fn helper() {}
"#
            .to_string(),
        };
        let refs = extract_references(&file).unwrap();
        let ref_names: Vec<&str> = refs.iter().map(|r| r.to_name.as_str()).collect();
        assert!(
            ref_names.contains(&"helper"),
            "should find reference to helper: {ref_names:?}"
        );
        assert!(
            ref_names.contains(&"Config"),
            "should find reference to Config: {ref_names:?}"
        );
    }

    #[test]
    fn parse_java_file() {
        let file = SourceFile {
            path: PathBuf::from("Main.java"),
            language: Language::Java,
            content: r#"
public class Main {
    public static void main(String[] args) {
        System.out.println("Hello");
    }
    public int add(int a, int b) {
        return a + b;
    }
}

interface Runnable {
    void run();
}

enum Color { RED, GREEN, BLUE }
"#
            .to_string(),
        };
        let symbols = extract_symbols(&file).unwrap();
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Main"), "should find class Main: {names:?}");
        assert!(
            names.contains(&"main"),
            "should find method main: {names:?}"
        );
        assert!(names.contains(&"add"), "should find method add: {names:?}");
        assert!(
            names.contains(&"Runnable"),
            "should find interface Runnable: {names:?}"
        );
        assert!(
            names.contains(&"Color"),
            "should find enum Color: {names:?}"
        );
    }

    #[test]
    fn parse_c_file() {
        let file = SourceFile {
            path: PathBuf::from("main.c"),
            language: Language::C,
            content: r#"
struct Point {
    int x;
    int y;
};

enum Direction { NORTH, SOUTH, EAST, WEST };

int add(int a, int b) {
    return a + b;
}

int main() {
    return 0;
}
"#
            .to_string(),
        };
        let symbols = extract_symbols(&file).unwrap();
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"Point"),
            "should find struct Point: {names:?}"
        );
        assert!(
            names.contains(&"Direction"),
            "should find enum Direction: {names:?}"
        );
        assert!(
            names.contains(&"add"),
            "should find function add: {names:?}"
        );
        assert!(
            names.contains(&"main"),
            "should find function main: {names:?}"
        );
    }

    #[test]
    fn parse_cpp_file() {
        let file = SourceFile {
            path: PathBuf::from("main.cpp"),
            language: Language::Cpp,
            content: r#"
class Calculator {
public:
    int add(int a, int b) {
        return a + b;
    }
};

struct Point {
    int x, y;
};

int main() {
    return 0;
}
"#
            .to_string(),
        };
        let symbols = extract_symbols(&file).unwrap();
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"Calculator"),
            "should find class Calculator: {names:?}"
        );
        assert!(names.contains(&"add"), "should find method add: {names:?}");
        assert!(
            names.contains(&"Point"),
            "should find struct Point: {names:?}"
        );
        assert!(
            names.contains(&"main"),
            "should find function main: {names:?}"
        );
    }

    #[test]
    fn parse_ruby_file() {
        let file = SourceFile {
            path: PathBuf::from("app.rb"),
            language: Language::Ruby,
            content: r#"
module MyApp
  class Calculator
    def add(a, b)
      a + b
    end

    def subtract(a, b)
      a - b
    end
  end
end

def standalone_function
  puts "hello"
end
"#
            .to_string(),
        };
        let symbols = extract_symbols(&file).unwrap();
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"MyApp"),
            "should find module MyApp: {names:?}"
        );
        assert!(
            names.contains(&"Calculator"),
            "should find class Calculator: {names:?}"
        );
        assert!(names.contains(&"add"), "should find method add: {names:?}");
        assert!(
            names.contains(&"subtract"),
            "should find method subtract: {names:?}"
        );
        assert!(
            names.contains(&"standalone_function"),
            "should find function standalone_function: {names:?}"
        );
    }
}
