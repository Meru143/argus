//! AST-aware code chunking using tree-sitter.
//!
//! Extracts semantic code chunks (functions, methods, structs, enums, impl blocks)
//! from source files. Each chunk includes enriched context headers for better
//! embedding quality.

use std::path::{Path, PathBuf};

use argus_core::ArgusError;
use argus_repomap::walker::Language;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tree_sitter::{Node, Parser};

/// A semantic code chunk extracted from a source file.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use argus_codelens::chunker::CodeChunk;
///
/// let chunk = CodeChunk {
///     file_path: PathBuf::from("src/main.rs"),
///     start_line: 1,
///     end_line: 5,
///     entity_name: "main".into(),
///     entity_type: "function".into(),
///     language: "rust".into(),
///     content: "fn main() {}".into(),
///     context_header: "# File: src/main.rs\n# Type: function\n# Name: main".into(),
///     content_hash: "abc123".into(),
/// };
/// assert_eq!(chunk.entity_name, "main");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeChunk {
    /// Path to the source file (relative to repo root).
    pub file_path: PathBuf,
    /// First line of the chunk (1-indexed).
    pub start_line: u32,
    /// Last line of the chunk (1-indexed).
    pub end_line: u32,
    /// Entity name (e.g. `"process_payment"`).
    pub entity_name: String,
    /// Entity type (e.g. `"function"`, `"method"`, `"struct"`, `"impl"`).
    pub entity_type: String,
    /// Programming language.
    pub language: String,
    /// Raw code content.
    pub content: String,
    /// Enriched text for embedding (context header + content).
    pub context_header: String,
    /// SHA-256 of `content`, for dedup/caching.
    pub content_hash: String,
}

/// Extract semantic chunks from a source file using tree-sitter.
///
/// Reuses the `Language` enum and tree-sitter setup from `argus-repomap`.
///
/// # Errors
///
/// Returns [`ArgusError::Parse`] if the language grammar cannot be loaded.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use argus_repomap::walker::Language;
/// use argus_codelens::chunker::chunk_file;
///
/// let chunks = chunk_file(
///     Path::new("example.rs"),
///     "fn hello() { println!(\"hi\"); }",
///     Language::Rust,
/// ).unwrap();
/// assert_eq!(chunks.len(), 1);
/// assert_eq!(chunks[0].entity_name, "hello");
/// ```
pub fn chunk_file(
    path: &Path,
    content: &str,
    language: Language,
) -> Result<Vec<CodeChunk>, ArgusError> {
    let Some(ts_language) = language.tree_sitter_language() else {
        return Ok(Vec::new());
    };

    let mut parser = Parser::new();
    parser
        .set_language(&ts_language)
        .map_err(|e| ArgusError::Parse(format!("failed to set language: {e}")))?;

    let Some(tree) = parser.parse(content, None) else {
        return Ok(Vec::new());
    };

    let source = content.as_bytes();
    let mut chunks = Vec::new();
    let lang_str = language_str(language);

    match language {
        Language::Rust => {
            collect_rust_chunks(tree.root_node(), source, path, lang_str, None, &mut chunks)
        }
        Language::Python => {
            collect_python_chunks(tree.root_node(), source, path, lang_str, None, &mut chunks)
        }
        Language::TypeScript | Language::JavaScript => {
            collect_js_ts_chunks(tree.root_node(), source, path, lang_str, None, &mut chunks);
        }
        Language::Go => {
            collect_go_chunks(tree.root_node(), source, path, lang_str, None, &mut chunks)
        }
        Language::Java => {
            collect_java_chunks(tree.root_node(), source, path, lang_str, None, &mut chunks)
        }
        Language::C | Language::Cpp => {
            collect_c_cpp_chunks(tree.root_node(), source, path, lang_str, None, &mut chunks)
        }
        Language::Ruby => {
            collect_ruby_chunks(tree.root_node(), source, path, lang_str, None, &mut chunks)
        }
        Language::Unknown => {}
    }

    Ok(chunks)
}

/// Chunk all files in a repository.
///
/// # Errors
///
/// Returns [`ArgusError`] if file walking or parsing fails.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use argus_codelens::chunker::chunk_repo;
///
/// let chunks = chunk_repo(Path::new(".")).unwrap();
/// println!("Found {} chunks", chunks.len());
/// ```
pub fn chunk_repo(root: &Path) -> Result<Vec<CodeChunk>, ArgusError> {
    let files = argus_repomap::walker::walk_repo(root)?;
    let mut all_chunks = Vec::new();

    for file in &files {
        let chunks = chunk_file(&file.path, &file.content, file.language)?;
        all_chunks.extend(chunks);
    }

    Ok(all_chunks)
}

fn language_str(lang: Language) -> &'static str {
    match lang {
        Language::Rust => "rust",
        Language::Python => "python",
        Language::TypeScript => "typescript",
        Language::JavaScript => "javascript",
        Language::Go => "go",
        Language::Java => "java",
        Language::C => "c",
        Language::Cpp => "cpp",
        Language::Ruby => "ruby",
        Language::Unknown => "unknown",
    }
}

fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
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

fn has_child_kind(node: &Node, kind: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return true;
        }
    }
    false
}

fn extract_signature(node: &Node, source: &[u8]) -> String {
    let text = node_text(node, source);
    let sig = if let Some(pos) = text.find('{') {
        &text[..pos]
    } else if let Some(pos) = text.find(':') {
        &text[..pos]
    } else {
        &text
    };
    sig.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn build_context_header(
    file_path: &Path,
    entity_type: &str,
    entity_name: &str,
    scope: Option<&str>,
    signature: &str,
) -> String {
    let mut header = format!(
        "# File: {}\n# Type: {}\n# Name: {}",
        file_path.display(),
        entity_type,
        entity_name,
    );
    if let Some(scope) = scope {
        header.push_str(&format!("\n# Scope: {scope}"));
    }
    if !signature.is_empty() {
        header.push_str(&format!("\n# Signature: {signature}"));
    }
    header
}

fn make_chunk(
    file_path: &Path,
    node: &Node,
    source: &[u8],
    entity_name: &str,
    entity_type: &str,
    language: &str,
    scope: Option<&str>,
) -> CodeChunk {
    let content = node_text(node, source);
    let signature = extract_signature(node, source);
    let context_header =
        build_context_header(file_path, entity_type, entity_name, scope, &signature);
    let content_hash = compute_hash(&content);

    CodeChunk {
        file_path: file_path.to_path_buf(),
        start_line: node.start_position().row as u32 + 1,
        end_line: node.end_position().row as u32 + 1,
        entity_name: entity_name.to_string(),
        entity_type: entity_type.to_string(),
        language: language.to_string(),
        content,
        context_header,
        content_hash,
    }
}

fn collect_rust_chunks(
    node: Node,
    source: &[u8],
    file_path: &Path,
    language: &str,
    impl_name: Option<&str>,
    chunks: &mut Vec<CodeChunk>,
) {
    let kind_str = node.kind();

    match kind_str {
        "function_item" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let entity_type = if impl_name.is_some() {
                    "method"
                } else {
                    "function"
                };
                let scope = impl_name.map(|n| format!("impl {n}"));
                chunks.push(make_chunk(
                    file_path,
                    &node,
                    source,
                    &name,
                    entity_type,
                    language,
                    scope.as_deref(),
                ));
            }
        }
        "struct_item" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                chunks.push(make_chunk(
                    file_path, &node, source, &name, "struct", language, None,
                ));
            }
        }
        "enum_item" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                chunks.push(make_chunk(
                    file_path, &node, source, &name, "enum", language, None,
                ));
            }
        }
        "trait_item" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                chunks.push(make_chunk(
                    file_path, &node, source, &name, "trait", language, None,
                ));
            }
        }
        "impl_item" => {
            let type_name = find_child_text(&node, "type_identifier", source);
            // Recurse into impl body to chunk each method separately
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_rust_chunks(
                    child,
                    source,
                    file_path,
                    language,
                    type_name.as_deref(),
                    chunks,
                );
            }
            return;
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_rust_chunks(child, source, file_path, language, impl_name, chunks);
    }
}

fn collect_python_chunks(
    node: Node,
    source: &[u8],
    file_path: &Path,
    language: &str,
    class_name: Option<&str>,
    chunks: &mut Vec<CodeChunk>,
) {
    let kind_str = node.kind();

    match kind_str {
        "function_definition" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let entity_type = if class_name.is_some() {
                    "method"
                } else {
                    "function"
                };
                let scope = class_name.map(|n| format!("class {n}"));
                chunks.push(make_chunk(
                    file_path,
                    &node,
                    source,
                    &name,
                    entity_type,
                    language,
                    scope.as_deref(),
                ));
            }
        }
        "class_definition" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                chunks.push(make_chunk(
                    file_path, &node, source, &name, "class", language, None,
                ));
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    collect_python_chunks(child, source, file_path, language, Some(&name), chunks);
                }
                return;
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_python_chunks(child, source, file_path, language, class_name, chunks);
    }
}

fn collect_js_ts_chunks(
    node: Node,
    source: &[u8],
    file_path: &Path,
    language: &str,
    class_name: Option<&str>,
    chunks: &mut Vec<CodeChunk>,
) {
    let kind_str = node.kind();

    match kind_str {
        "function_declaration" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                chunks.push(make_chunk(
                    file_path, &node, source, &name, "function", language, None,
                ));
            }
        }
        "class_declaration" => {
            let name = find_child_text(&node, "type_identifier", source)
                .or_else(|| find_child_text(&node, "identifier", source));
            if let Some(name) = name {
                chunks.push(make_chunk(
                    file_path, &node, source, &name, "class", language, None,
                ));
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    collect_js_ts_chunks(child, source, file_path, language, Some(&name), chunks);
                }
                return;
            }
        }
        "method_definition" => {
            if let Some(name) = find_child_text(&node, "property_identifier", source) {
                let scope = class_name.map(|n| format!("class {n}"));
                chunks.push(make_chunk(
                    file_path,
                    &node,
                    source,
                    &name,
                    "method",
                    language,
                    scope.as_deref(),
                ));
            }
        }
        "lexical_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "variable_declarator" {
                    let has_arrow = has_child_kind(&child, "arrow_function");
                    if has_arrow {
                        if let Some(name) = find_child_text(&child, "identifier", source) {
                            chunks.push(make_chunk(
                                file_path, &node, source, &name, "function", language, None,
                            ));
                        }
                    }
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_js_ts_chunks(child, source, file_path, language, class_name, chunks);
    }
}

fn collect_go_chunks(
    node: Node,
    source: &[u8],
    file_path: &Path,
    language: &str,
    _scope: Option<&str>,
    chunks: &mut Vec<CodeChunk>,
) {
    let kind_str = node.kind();

    match kind_str {
        "function_declaration" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                chunks.push(make_chunk(
                    file_path, &node, source, &name, "function", language, None,
                ));
            }
        }
        "method_declaration" => {
            if let Some(name) = find_child_text(&node, "field_identifier", source) {
                chunks.push(make_chunk(
                    file_path, &node, source, &name, "method", language, None,
                ));
            }
        }
        "type_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "type_spec" {
                    if let Some(name) = find_child_text(&child, "type_identifier", source) {
                        let has_struct = has_child_kind(&child, "struct_type");
                        let has_interface = has_child_kind(&child, "interface_type");
                        let entity_type = if has_struct {
                            "struct"
                        } else if has_interface {
                            "interface"
                        } else {
                            continue;
                        };
                        chunks.push(make_chunk(
                            file_path,
                            &child,
                            source,
                            &name,
                            entity_type,
                            language,
                            None,
                        ));
                    }
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_go_chunks(child, source, file_path, language, None, chunks);
    }
}

fn collect_java_chunks(
    node: Node,
    source: &[u8],
    file_path: &Path,
    language: &str,
    scope: Option<&str>,
    chunks: &mut Vec<CodeChunk>,
) {
    let kind_str = node.kind();

    match kind_str {
        "method_declaration" | "constructor_declaration" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let entity_type = if scope.is_some() {
                    "method"
                } else {
                    "function"
                };
                chunks.push(make_chunk(
                    file_path,
                    &node,
                    source,
                    &name,
                    entity_type,
                    language,
                    scope,
                ));
            }
        }
        "class_declaration" => {
            let name = find_child_text(&node, "identifier", source);
            if let Some(name) = &name {
                chunks.push(make_chunk(
                    file_path, &node, source, name, "class", language, scope,
                ));
            }
            let scope_name = name.as_deref();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_java_chunks(child, source, file_path, language, scope_name, chunks);
            }
            return;
        }
        "interface_declaration" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                chunks.push(make_chunk(
                    file_path,
                    &node,
                    source,
                    &name,
                    "interface",
                    language,
                    scope,
                ));
            }
        }
        "enum_declaration" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                chunks.push(make_chunk(
                    file_path, &node, source, &name, "enum", language, scope,
                ));
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_java_chunks(child, source, file_path, language, scope, chunks);
    }
}

fn collect_c_cpp_chunks(
    node: Node,
    source: &[u8],
    file_path: &Path,
    language: &str,
    scope: Option<&str>,
    chunks: &mut Vec<CodeChunk>,
) {
    let kind_str = node.kind();

    match kind_str {
        "function_definition" => {
            // C/C++ function names are in function_declarator -> identifier
            let name = find_nested_func_name(&node, source)
                .or_else(|| find_child_text(&node, "identifier", source));
            if let Some(name) = name {
                let entity_type = if scope.is_some() {
                    "method"
                } else {
                    "function"
                };
                chunks.push(make_chunk(
                    file_path,
                    &node,
                    source,
                    &name,
                    entity_type,
                    language,
                    scope,
                ));
            }
        }
        "class_specifier" => {
            let name = find_child_text(&node, "type_identifier", source);
            if let Some(name) = &name {
                chunks.push(make_chunk(
                    file_path, &node, source, name, "class", language, scope,
                ));
            }
            let scope_name = name.as_deref();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_c_cpp_chunks(child, source, file_path, language, scope_name, chunks);
            }
            return;
        }
        "struct_specifier" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                chunks.push(make_chunk(
                    file_path, &node, source, &name, "struct", language, scope,
                ));
            }
        }
        "enum_specifier" => {
            if let Some(name) = find_child_text(&node, "type_identifier", source) {
                chunks.push(make_chunk(
                    file_path, &node, source, &name, "enum", language, scope,
                ));
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_c_cpp_chunks(child, source, file_path, language, scope, chunks);
    }
}

fn collect_ruby_chunks(
    node: Node,
    source: &[u8],
    file_path: &Path,
    language: &str,
    scope: Option<&str>,
    chunks: &mut Vec<CodeChunk>,
) {
    let kind_str = node.kind();

    match kind_str {
        "method" => {
            if let Some(name) = find_child_text(&node, "identifier", source) {
                let entity_type = if scope.is_some() {
                    "method"
                } else {
                    "function"
                };
                chunks.push(make_chunk(
                    file_path,
                    &node,
                    source,
                    &name,
                    entity_type,
                    language,
                    scope,
                ));
            }
        }
        "class" => {
            let name = find_child_text(&node, "constant", source)
                .or_else(|| find_child_text(&node, "scope_resolution", source));
            if let Some(name) = &name {
                chunks.push(make_chunk(
                    file_path, &node, source, name, "class", language, scope,
                ));
            }
            let scope_name = name.as_deref();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_ruby_chunks(child, source, file_path, language, scope_name, chunks);
            }
            return;
        }
        "module" => {
            let name = find_child_text(&node, "constant", source);
            if let Some(name) = &name {
                chunks.push(make_chunk(
                    file_path, &node, source, name, "module", language, scope,
                ));
            }
            let scope_name = name.as_deref();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_ruby_chunks(child, source, file_path, language, scope_name, chunks);
            }
            return;
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_ruby_chunks(child, source, file_path, language, scope, chunks);
    }
}

/// Find the function name from a function_declarator child in C/C++.
fn find_nested_func_name(node: &Node, source: &[u8]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_declarator" {
            return find_child_text(&child, "identifier", source)
                .or_else(|| find_child_text(&child, "field_identifier", source));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_rust_file_with_functions_structs_impl() {
        let content = r#"
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

impl Config {
    pub fn new(name: String) -> Self {
        Self { name, value: 0 }
    }

    pub fn value(&self) -> u32 {
        self.value
    }
}
"#;
        let chunks = chunk_file(Path::new("src/lib.rs"), content, Language::Rust).unwrap();

        let names: Vec<&str> = chunks.iter().map(|c| c.entity_name.as_str()).collect();
        assert!(names.contains(&"top_level"), "missing top_level: {names:?}");
        assert!(
            names.contains(&"Config"),
            "missing Config struct: {names:?}"
        );
        assert!(names.contains(&"Color"), "missing Color enum: {names:?}");
        assert!(names.contains(&"new"), "missing new method: {names:?}");
        assert!(names.contains(&"value"), "missing value method: {names:?}");

        let top = chunks
            .iter()
            .find(|c| c.entity_name == "top_level")
            .unwrap();
        assert_eq!(top.entity_type, "function");
        assert_eq!(top.language, "rust");

        let new_method = chunks.iter().find(|c| c.entity_name == "new").unwrap();
        assert_eq!(new_method.entity_type, "method");
    }

    #[test]
    fn context_header_is_properly_formatted() {
        let content = "pub fn validate_token(token: &str) -> bool { true }";
        let chunks = chunk_file(Path::new("src/auth.rs"), content, Language::Rust).unwrap();
        assert_eq!(chunks.len(), 1);

        let header = &chunks[0].context_header;
        assert!(header.contains("# File: src/auth.rs"), "header: {header}");
        assert!(header.contains("# Type: function"), "header: {header}");
        assert!(
            header.contains("# Name: validate_token"),
            "header: {header}"
        );
        assert!(header.contains("# Signature:"), "header: {header}");
    }

    #[test]
    fn content_hash_is_deterministic() {
        let content = "fn hello() { println!(\"world\"); }";
        let chunks1 = chunk_file(Path::new("a.rs"), content, Language::Rust).unwrap();
        let chunks2 = chunk_file(Path::new("a.rs"), content, Language::Rust).unwrap();
        assert_eq!(chunks1[0].content_hash, chunks2[0].content_hash);
    }

    #[test]
    fn large_function_kept_as_single_chunk() {
        let body = "    let x = 1;\n".repeat(1000);
        let content = format!("fn big() {{\n{body}}}");
        let chunks = chunk_file(Path::new("big.rs"), &content, Language::Rust).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.len() > 8000);
    }

    #[test]
    fn empty_file_produces_empty_vec() {
        let chunks = chunk_file(Path::new("empty.rs"), "", Language::Rust).unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn impl_methods_have_scope() {
        let content = r#"
impl AuthService {
    pub async fn validate_token(&self, token: &str) -> bool {
        true
    }
}
"#;
        let chunks = chunk_file(Path::new("src/auth.rs"), content, Language::Rust).unwrap();
        let method = chunks
            .iter()
            .find(|c| c.entity_name == "validate_token")
            .unwrap();
        assert!(method.context_header.contains("# Scope: impl AuthService"));
    }
}
