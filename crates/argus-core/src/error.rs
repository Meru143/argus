use std::path::PathBuf;

/// Errors that can occur across the Argus platform.
///
/// Each variant wraps a specific error domain. Library crates use this type
/// directly; the binary crate converts to `anyhow::Error` at the boundary.
///
/// # Examples
///
/// ```
/// use argus_core::ArgusError;
///
/// let err = ArgusError::Config("missing API key".into());
/// assert!(err.to_string().contains("missing API key"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum ArgusError {
    /// Filesystem I/O failure.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Invalid or missing configuration.
    #[error("configuration error: {0}")]
    Config(String),

    /// Git operation failure.
    #[error("git error: {0}")]
    Git(String),

    /// Source code parsing failure.
    #[error("parse error: {0}")]
    Parse(String),

    /// LLM API or response error.
    #[error("LLM error: {0}")]
    Llm(String),

    /// JSON serialization / deserialization failure.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// TOML deserialization failure.
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    /// A required file was not found.
    #[error("file not found: {}", .0.display())]
    FileNotFound(PathBuf),
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
        assert_eq!(err.to_string(), "configuration error: bad value");
    }

    #[test]
    fn file_not_found_shows_path() {
        let err = ArgusError::FileNotFound(PathBuf::from("/tmp/missing.rs"));
        assert!(err.to_string().contains("/tmp/missing.rs"));
    }
}
