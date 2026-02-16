use std::path::{Path, PathBuf};

use argus_core::ArgusError;

/// Maximum file size to process (1 MB).
const MAX_FILE_SIZE: u64 = 1_048_576;

/// Number of bytes to check for binary detection.
const BINARY_CHECK_SIZE: usize = 8192;

/// A source file discovered during repository walking.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use argus_repomap::walker::{Language, SourceFile};
///
/// let file = SourceFile {
///     path: PathBuf::from("src/main.rs"),
///     language: Language::Rust,
///     content: "fn main() {}".to_string(),
/// };
/// assert_eq!(file.language, Language::Rust);
/// ```
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// Path relative to the repository root.
    pub path: PathBuf,
    /// Detected programming language.
    pub language: Language,
    /// Full file content.
    pub content: String,
}

/// Programming language detected from file extension.
///
/// # Examples
///
/// ```
/// use argus_repomap::walker::Language;
///
/// assert_eq!(Language::from_extension("rs"), Language::Rust);
/// assert_eq!(Language::from_extension("py"), Language::Python);
/// assert_eq!(Language::from_extension("java"), Language::Java);
/// assert_eq!(Language::from_extension("c"), Language::C);
/// assert_eq!(Language::from_extension("cpp"), Language::Cpp);
/// assert_eq!(Language::from_extension("rb"), Language::Ruby);
/// assert_eq!(Language::from_extension("php"), Language::Php);
/// assert_eq!(Language::from_extension("kt"), Language::Kotlin);
/// assert_eq!(Language::from_extension("swift"), Language::Swift);
/// assert_eq!(Language::from_extension("txt"), Language::Unknown);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Go,
    Java,
    C,
    Cpp,
    Ruby,
    Php,
    Kotlin,
    Swift,
    Unknown,
}

impl Language {
    /// Detect language from a file extension string (without the dot).
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "rs" => Language::Rust,
            "py" => Language::Python,
            "ts" | "tsx" => Language::TypeScript,
            "js" | "jsx" => Language::JavaScript,
            "go" => Language::Go,
            "java" => Language::Java,
            "c" | "h" => Language::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Language::Cpp,
            "rb" => Language::Ruby,
            "php" => Language::Php,
            "kt" | "kts" => Language::Kotlin,
            "swift" => Language::Swift,
            _ => Language::Unknown,
        }
    }

    /// Get the tree-sitter language grammar for this language.
    ///
    /// Returns `None` for `Language::Unknown`.
    pub fn tree_sitter_language(&self) -> Option<tree_sitter::Language> {
        match self {
            Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
            Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
            Language::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
            Language::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
            Language::Go => Some(tree_sitter_go::LANGUAGE.into()),
            Language::Java => Some(tree_sitter_java::LANGUAGE.into()),
            Language::C => Some(tree_sitter_c::LANGUAGE.into()),
            Language::Cpp => Some(tree_sitter_cpp::LANGUAGE.into()),
            Language::Ruby => Some(tree_sitter_ruby::LANGUAGE.into()),
            Language::Php => Some(tree_sitter_php::LANGUAGE_PHP.into()),
            Language::Kotlin => Some(tree_sitter_kotlin_ng::LANGUAGE.into()),
            Language::Swift => Some(tree_sitter_swift::LANGUAGE.into()),
            Language::Unknown => None,
        }
    }
}

/// Walk a repository, respecting `.gitignore`, returning parseable source files.
///
/// Skips binary files, files larger than 1 MB, and files with unknown extensions.
/// Returned paths are relative to `root`.
///
/// # Errors
///
/// Returns [`ArgusError::Io`] if the root directory cannot be read.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use argus_repomap::walker::walk_repo;
///
/// let files = walk_repo(Path::new(".")).unwrap();
/// for f in &files {
///     println!("{}: {:?}", f.path.display(), f.language);
/// }
/// ```
pub fn walk_repo(root: &Path) -> Result<Vec<SourceFile>, ArgusError> {
    let walker = ignore::WalkBuilder::new(root).build();
    let mut files = Vec::new();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let Some(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }

        let path = entry.path();

        // Check file size
        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.len() > MAX_FILE_SIZE {
            continue;
        }

        // Detect language from extension
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => continue,
        };
        let language = Language::from_extension(ext);
        if language == Language::Unknown {
            continue;
        }

        // Read content
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Check for binary content (null bytes in first 8KB)
        let check_len = content.len().min(BINARY_CHECK_SIZE);
        if content.as_bytes()[..check_len].contains(&0) {
            continue;
        }

        // Make path relative to root
        let relative = match path.strip_prefix(root) {
            Ok(r) => r.to_path_buf(),
            Err(_) => path.to_path_buf(),
        };

        files.push(SourceFile {
            path: relative,
            language,
            content,
        });
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_temp_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create source files
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(root.join("src/lib.py"), "def hello(): pass").unwrap();
        fs::write(root.join("src/app.ts"), "function run() {}").unwrap();
        fs::write(root.join("src/util.js"), "const x = 1;").unwrap();
        fs::write(root.join("src/main.go"), "package main").unwrap();
        fs::write(
            root.join("src/Main.java"),
            "public class Main { public static void main(String[] args) {} }",
        )
        .unwrap();
        fs::write(root.join("src/hello.c"), "int main() { return 0; }").unwrap();
        fs::write(root.join("src/hello.cpp"), "int main() { return 0; }").unwrap();
        fs::write(root.join("src/hello.rb"), "def hello; end").unwrap();

        // Create unknown extension file
        fs::write(root.join("README.md"), "# Hello").unwrap();
        fs::write(root.join("data.csv"), "a,b,c").unwrap();

        dir
    }

    #[test]
    fn walk_finds_known_language_files() {
        let dir = make_temp_repo();
        let files = walk_repo(dir.path()).unwrap();

        assert_eq!(files.len(), 9);

        let languages: Vec<Language> = files.iter().map(|f| f.language).collect();
        assert!(languages.contains(&Language::Rust));
        assert!(languages.contains(&Language::Python));
        assert!(languages.contains(&Language::TypeScript));
        assert!(languages.contains(&Language::JavaScript));
        assert!(languages.contains(&Language::Go));
        assert!(languages.contains(&Language::Java));
        assert!(languages.contains(&Language::C));
        assert!(languages.contains(&Language::Cpp));
        assert!(languages.contains(&Language::Ruby));
    }

    #[test]
    fn walk_respects_gitignore() {
        let dir = make_temp_repo();
        let root = dir.path();

        // The ignore crate needs a .git dir to recognize .gitignore files
        fs::create_dir_all(root.join(".git")).unwrap();

        // Create .gitignore that ignores the build dir
        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(root.join("build/output.rs"), "fn ignored() {}").unwrap();
        fs::write(root.join(".gitignore"), "build/\n").unwrap();

        let files = walk_repo(root).unwrap();
        let paths: Vec<&Path> = files.iter().map(|f| f.path.as_path()).collect();
        for p in &paths {
            assert!(
                !p.starts_with("build"),
                "gitignored file should be skipped: {}",
                p.display()
            );
        }
    }

    #[test]
    fn walk_skips_binary_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create a binary .rs file (contains null bytes)
        let mut binary_content = b"fn main() { ".to_vec();
        binary_content.push(0);
        binary_content.extend_from_slice(b" }");
        fs::write(root.join("binary.rs"), &binary_content).unwrap();

        // Create a normal .rs file
        fs::write(root.join("normal.rs"), "fn normal() {}").unwrap();

        let files = walk_repo(root).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("normal.rs"));
    }

    #[test]
    fn walk_skips_large_and_unknown_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create a file larger than 1MB
        let large_content = "x".repeat(1_048_577);
        fs::write(root.join("huge.rs"), &large_content).unwrap();

        // Create unknown extension
        fs::write(root.join("data.txt"), "hello").unwrap();

        // Create normal file
        fs::write(root.join("ok.rs"), "fn ok() {}").unwrap();

        let files = walk_repo(root).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("ok.rs"));
    }
}
