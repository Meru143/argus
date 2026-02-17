use std::path::PathBuf;

use miette::Diagnostic;

/// Errors that can occur across the Argus platform.
///
/// Each variant wraps a specific error domain. Library crates use this type
/// directly; the binary crate converts to `miette::Report` at the boundary.
///
/// # Examples
///
/// ```
/// use argus_core::ArgusError;
///
/// let err = ArgusError::Config("missing API key".into());
/// assert!(err.to_string().contains("missing API key"));
/// ```
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum ArgusError {
    /// Filesystem I/O failure.
    #[error("IO error: {0}")]
    #[diagnostic(code(argus::io), help("Check file permissions and paths"))]
    Io(#[from] std::io::Error),

    /// Invalid or missing configuration.
    #[error("Configuration error: {0}")]
    #[diagnostic(
        code(argus::config),
        help("Run `argus init` to create a default config, or check .argus.toml syntax")
    )]
    Config(String),

    /// Git operation failure.
    #[error("Git error: {0}")]
    #[diagnostic(code(argus::git), help("Make sure you're inside a git repository"))]
    Git(String),

    /// GitHub API failure.
    #[error("GitHub API error: {0}")]
    #[diagnostic(
        code(argus::github),
        help("Check your GITHUB_TOKEN permissions and network connection")
    )]
    GitHub(String),

    /// Source code parsing failure.
    #[error("Parse error: {0}")]
    #[diagnostic(
        code(argus::parse),
        help("Check that the file is valid source code in a supported language")
    )]
    Parse(String),

    /// LLM API or response error.
    #[error("LLM error: {0}")]
    #[diagnostic(code(argus::llm), help("Check your API key and provider config in .argus.toml. Run `argus doctor` to diagnose."))]
    Llm(String),

    /// JSON serialization / deserialization failure.
    #[error("Serialization error: {0}")]
    #[diagnostic(code(argus::serde))]
    Serialization(#[from] serde_json::Error),

    /// TOML deserialization failure.
    #[error("TOML parse error: {0}")]
    #[diagnostic(
        code(argus::toml),
        help("Check .argus.toml syntax — run `argus init` to generate a fresh one")
    )]
    Toml(#[from] toml::de::Error),

    /// A required file was not found.
    #[error("File not found: {}", .0.display())]
    #[diagnostic(
        code(argus::file_not_found),
        help("Check the path exists and is readable")
    )]
    FileNotFound(PathBuf),

    /// Embedding API error.
    #[error("Embedding error: {0}")]
    #[diagnostic(
        code(argus::embedding),
        help("Check your embedding provider API key. Run `argus doctor` to diagnose.")
    )]
    Embedding(String),

    /// Database operation failure.
    #[error("Database error: {0}")]
    #[diagnostic(
        code(argus::database),
        help("Try deleting .argus/index.db and re-indexing")
    )]
    Database(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err: ArgusError = io_err.into();
        assert!(err.to_string().contains("gone"));
    }

    #[test]
    fn config_error_displays_message() {
        let err = ArgusError::Config("bad value".into());
        assert_eq!(err.to_string(), "Configuration error: bad value");
    }

    #[test]
    fn file_not_found_shows_path() {
        let err = ArgusError::FileNotFound(PathBuf::from("/tmp/missing.rs"));
        assert!(err.to_string().contains("/tmp/missing.rs"));
    }
}
