use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ArgusError;
use crate::types::Severity;

/// A custom review rule defined in `.argus.toml`.
///
/// Rules are injected into the LLM system prompt so the reviewer
/// checks for project-specific patterns.
///
/// # Examples
///
/// ```
/// use argus_core::Rule;
///
/// let rule = Rule {
///     name: "no-unwrap".into(),
///     severity: "warning".into(),
///     description: "Do not use .unwrap() in production code".into(),
/// };
/// assert_eq!(rule.name, "no-unwrap");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Short identifier for the rule (used in output).
    pub name: String,
    /// Severity level: "bug", "warning", or "suggestion".
    pub severity: String,
    /// Natural language instruction for the LLM.
    pub description: String,
}

/// Top-level configuration loaded from `.argus.toml`.
///
/// Supports layered resolution: CLI flags > env vars > local config > defaults.
///
/// # Examples
///
/// ```
/// use argus_core::ArgusConfig;
///
/// let config = ArgusConfig::default();
/// assert_eq!(config.review.max_comments, 5);
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArgusConfig {
    /// LLM provider settings.
    #[serde(default)]
    pub llm: LlmConfig,
    /// Review behavior settings.
    #[serde(default)]
    pub review: ReviewConfig,
    /// Embedding provider settings for semantic search.
    #[serde(default)]
    pub embedding: EmbeddingConfig,
    /// Per-path overrides for monorepo support.
    #[serde(default)]
    pub paths: HashMap<String, PathConfig>,
    /// Custom review rules injected into the LLM prompt.
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl ArgusConfig {
    /// Load configuration from a TOML file at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Io`] if the file cannot be read, or
    /// [`ArgusError::Toml`] if the content is not valid TOML.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use argus_core::ArgusConfig;
    /// use std::path::Path;
    ///
    /// let config = ArgusConfig::from_file(Path::new(".argus.toml")).unwrap();
    /// ```
    pub fn from_file(path: &Path) -> Result<Self, ArgusError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_toml(&content)
    }

    /// Parse configuration from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Toml`] if parsing fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_core::ArgusConfig;
    ///
    /// let toml = r#"
    /// [review]
    /// max_comments = 10
    /// "#;
    /// let config = ArgusConfig::from_toml(toml).unwrap();
    /// assert_eq!(config.review.max_comments, 10);
    /// ```
    pub fn from_toml(content: &str) -> Result<Self, ArgusError> {
        let config: Self = toml::from_str(content)?;
        Ok(config)
    }
}

/// LLM provider configuration.
///
/// # Examples
///
/// ```
/// use argus_core::LlmConfig;
///
/// let config = LlmConfig::default();
/// assert_eq!(config.model, "gpt-4o");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// Provider name (e.g. `"openai"`, `"anthropic"`, `"ollama"`).
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Model identifier.
    #[serde(default = "default_model")]
    pub model: String,
    /// API key for the provider.
    pub api_key: Option<String>,
    /// Custom base URL for API requests.
    pub base_url: Option<String>,
    /// Maximum input tokens to send per request.
    pub max_input_tokens: Option<usize>,
}

fn default_provider() -> String {
    "openai".into()
}

fn default_model() -> String {
    "gpt-4o".into()
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
            api_key: None,
            base_url: None,
            max_input_tokens: None,
        }
    }
}

/// Review behavior configuration.
///
/// # Examples
///
/// ```
/// use argus_core::ReviewConfig;
///
/// let config = ReviewConfig::default();
/// assert_eq!(config.min_confidence, 90.0);
/// assert_eq!(config.max_comments, 5);
/// assert!(!config.include_suggestions);
/// assert_eq!(config.max_diff_tokens, 4000);
/// assert!(config.cross_file);
/// assert!(config.self_reflection);
/// assert_eq!(config.self_reflection_score_threshold, 7);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewConfig {
    /// Maximum number of comments per review (default: 5).
    #[serde(default = "default_max_comments")]
    pub max_comments: usize,
    /// Minimum LLM confidence to include a comment (default: 90.0).
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f64,
    /// Only show comments at these severity levels.
    #[serde(default = "default_severity_filter")]
    pub severity_filter: Vec<Severity>,
    /// Additional glob patterns to skip before sending to LLM.
    #[serde(default)]
    pub skip_patterns: Vec<String>,
    /// Additional file extensions to skip before sending to LLM.
    #[serde(default)]
    pub skip_extensions: Vec<String>,
    /// Token threshold for splitting diff into per-file LLM calls (default: 4000).
    #[serde(default = "default_max_diff_tokens")]
    pub max_diff_tokens: usize,
    /// Include suggestion-level comments (default: false).
    #[serde(default)]
    pub include_suggestions: bool,
    /// Group related files for cross-file analysis when splitting diffs (default: true).
    #[serde(default = "default_cross_file")]
    pub cross_file: bool,
    /// Enable self-reflection pass to filter false positives (default: true).
    ///
    /// When enabled, a second LLM call evaluates the initial review comments
    /// and filters out low-quality ones (style nits, speculative issues, etc.).
    #[serde(default = "default_self_reflection")]
    pub self_reflection: bool,
    /// Minimum score (1-10) a comment must receive during self-reflection to be kept (default: 7).
    #[serde(default = "default_self_reflection_score_threshold")]
    pub self_reflection_score_threshold: u8,
}

fn default_max_comments() -> usize {
    5
}

fn default_min_confidence() -> f64 {
    90.0
}

fn default_severity_filter() -> Vec<Severity> {
    vec![Severity::Bug, Severity::Warning]
}

fn default_max_diff_tokens() -> usize {
    64000
}

fn default_cross_file() -> bool {
    true
}

fn default_self_reflection() -> bool {
    true
}

fn default_self_reflection_score_threshold() -> u8 {
    7
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            max_comments: default_max_comments(),
            min_confidence: default_min_confidence(),
            severity_filter: default_severity_filter(),
            skip_patterns: Vec::new(),
            skip_extensions: Vec::new(),
            max_diff_tokens: default_max_diff_tokens(),
            include_suggestions: false,
            cross_file: default_cross_file(),
            self_reflection: default_self_reflection(),
            self_reflection_score_threshold: default_self_reflection_score_threshold(),
        }
    }
}

/// Per-path configuration for monorepo support.
///
/// # Examples
///
/// ```
/// use argus_core::PathConfig;
///
/// let config = PathConfig {
///     instructions: Some("Focus on auth flows".into()),
///     context_boundary: true,
/// };
/// assert!(config.context_boundary);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathConfig {
    /// Custom review instructions for this path.
    pub instructions: Option<String>,
    /// When `true`, prevent cross-boundary context leaking.
    #[serde(default)]
    pub context_boundary: bool,
}

/// Configuration for embedding providers used by semantic search.
///
/// # Examples
///
/// ```
/// use argus_core::EmbeddingConfig;
///
/// let config = EmbeddingConfig::default();
/// assert_eq!(config.provider, "voyage");
/// assert_eq!(config.model, "voyage-code-3");
/// assert_eq!(config.dimensions, 1024);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Embedding provider (default: `"voyage"`).
    #[serde(default = "default_embedding_provider")]
    pub provider: String,
    /// API key for the embedding provider.
    pub api_key: Option<String>,
    /// Model name (default: `"voyage-code-3"`).
    #[serde(default = "default_embedding_model")]
    pub model: String,
    /// Embedding dimensions (default: 1024).
    #[serde(default = "default_embedding_dimensions")]
    pub dimensions: usize,
}

fn default_embedding_provider() -> String {
    "voyage".into()
}

fn default_embedding_model() -> String {
    "voyage-code-3".into()
}

fn default_embedding_dimensions() -> usize {
    1024
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: default_embedding_provider(),
            api_key: None,
            model: default_embedding_model(),
            dimensions: default_embedding_dimensions(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = ArgusConfig::default();
        assert_eq!(config.review.max_comments, 5);
        assert_eq!(config.review.min_confidence, 90.0);
        assert_eq!(config.review.max_diff_tokens, 64000);
        assert!(!config.review.include_suggestions);
        assert!(config.review.skip_patterns.is_empty());
        assert!(config.review.skip_extensions.is_empty());
        assert_eq!(config.llm.provider, "openai");
        assert_eq!(config.llm.model, "gpt-4o");
        assert_eq!(config.embedding.provider, "voyage");
        assert_eq!(config.embedding.model, "voyage-code-3");
        assert_eq!(config.embedding.dimensions, 1024);
        assert!(config.paths.is_empty());
        assert!(config.review.self_reflection);
        assert_eq!(config.review.self_reflection_score_threshold, 7);
    }

    #[test]
    fn parse_minimal_toml() {
        let toml = r#"
[review]
max_comments = 10
min_confidence = 85.0
"#;
        let config = ArgusConfig::from_toml(toml).unwrap();
        assert_eq!(config.review.max_comments, 10);
        assert_eq!(config.review.min_confidence, 85.0);
    }

    #[test]
    fn parse_full_toml() {
        let toml = r#"
[llm]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
base_url = "https://api.anthropic.com"
max_input_tokens = 50000

[review]
max_comments = 3
min_confidence = 95.0
severity_filter = ["bug"]

[paths."packages/auth"]
instructions = "Focus on authentication flows"
context_boundary = true
"#;
        let config = ArgusConfig::from_toml(toml).unwrap();
        assert_eq!(config.llm.provider, "anthropic");
        assert_eq!(config.llm.max_input_tokens, Some(50000));
        assert_eq!(config.review.max_comments, 3);
        assert_eq!(config.review.severity_filter, vec![Severity::Bug]);

        let auth_path = &config.paths["packages/auth"];
        assert!(auth_path.context_boundary);
        assert_eq!(
            auth_path.instructions.as_deref(),
            Some("Focus on authentication flows")
        );
    }

    #[test]
    fn empty_toml_gives_defaults() {
        let config = ArgusConfig::from_toml("").unwrap();
        assert_eq!(config.review.max_comments, 5);
        assert_eq!(config.llm.model, "gpt-4o");
    }

    #[test]
    fn invalid_toml_returns_error() {
        let result = ArgusConfig::from_toml("{{invalid}}");
        assert!(result.is_err());
    }

    #[test]
    fn parse_noise_reduction_config() {
        let toml = r#"
[review]
max_comments = 3
skip_patterns = ["*.test.ts", "fixtures/**"]
skip_extensions = ["snap", "lock"]
max_diff_tokens = 8000
include_suggestions = true
"#;
        let config = ArgusConfig::from_toml(toml).unwrap();
        assert_eq!(config.review.max_comments, 3);
        assert_eq!(
            config.review.skip_patterns,
            vec!["*.test.ts", "fixtures/**"]
        );
        assert_eq!(config.review.skip_extensions, vec!["snap", "lock"]);
        assert_eq!(config.review.max_diff_tokens, 8000);
        assert!(config.review.include_suggestions);
    }

    #[test]
    fn noise_reduction_defaults_when_omitted() {
        let toml = r#"
[review]
max_comments = 10
"#;
        let config = ArgusConfig::from_toml(toml).unwrap();
        assert!(config.review.skip_patterns.is_empty());
        assert!(config.review.skip_extensions.is_empty());
        assert_eq!(config.review.max_diff_tokens, 64000);
        assert!(!config.review.include_suggestions);
    }

    #[test]
    fn parse_rules_from_toml() {
        let toml = r#"
[[rules]]
name = "no-unwrap"
severity = "warning"
description = "Do not use .unwrap() in production code"

[[rules]]
name = "no-todo"
severity = "suggestion"
description = "Remove TODO comments before merging"
"#;
        let config = ArgusConfig::from_toml(toml).unwrap();
        assert_eq!(config.rules.len(), 2);
        assert_eq!(config.rules[0].name, "no-unwrap");
        assert_eq!(config.rules[0].severity, "warning");
        assert_eq!(
            config.rules[0].description,
            "Do not use .unwrap() in production code"
        );
        assert_eq!(config.rules[1].name, "no-todo");
        assert_eq!(config.rules[1].severity, "suggestion");
    }

    #[test]
    fn empty_rules_by_default() {
        assert!(ArgusConfig::default().rules.is_empty());
    }
}
