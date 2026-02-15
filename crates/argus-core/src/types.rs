use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A file in the repository with basic metadata.
///
/// Used by RepoMap to represent files in the codebase structure.
///
/// # Examples
///
/// ```
/// use argus_core::FileNode;
/// use std::path::PathBuf;
///
/// let node = FileNode {
///     path: PathBuf::from("src/main.rs"),
///     name: "main.rs".into(),
///     language: Some("rust".into()),
///     line_count: 42,
/// };
/// assert_eq!(node.name, "main.rs");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNode {
    /// Relative path from the repository root.
    pub path: PathBuf,
    /// File name (last component of path).
    pub name: String,
    /// Detected programming language, if any.
    pub language: Option<String>,
    /// Number of lines in the file.
    pub line_count: usize,
}

/// A single hunk from a unified diff.
///
/// # Examples
///
/// ```
/// use argus_core::{DiffHunk, ChangeType};
/// use std::path::PathBuf;
///
/// let hunk = DiffHunk {
///     file_path: PathBuf::from("src/lib.rs"),
///     old_start: 10,
///     old_lines: 5,
///     new_start: 10,
///     new_lines: 8,
///     content: "+ new line\n- old line".into(),
///     change_type: ChangeType::Modify,
/// };
/// assert_eq!(hunk.old_lines, 5);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    /// Path to the affected file.
    pub file_path: PathBuf,
    /// Starting line in the old version.
    pub old_start: u32,
    /// Number of lines in the old version.
    pub old_lines: u32,
    /// Starting line in the new version.
    pub new_start: u32,
    /// Number of lines in the new version.
    pub new_lines: u32,
    /// Raw diff content for this hunk.
    pub content: String,
    /// Classification of the change.
    pub change_type: ChangeType,
}

/// Classification of a diff hunk.
///
/// # Examples
///
/// ```
/// use argus_core::ChangeType;
///
/// let ct = ChangeType::Add;
/// assert_eq!(format!("{ct}"), "add");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    /// New file or code added.
    Add,
    /// Existing file or code removed.
    Delete,
    /// Existing code modified in place.
    Modify,
    /// Code moved between files or locations.
    Move,
}

impl fmt::Display for ChangeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChangeType::Add => write!(f, "add"),
            ChangeType::Delete => write!(f, "delete"),
            ChangeType::Modify => write!(f, "modify"),
            ChangeType::Move => write!(f, "move"),
        }
    }
}

/// Composite risk score for a set of changes.
///
/// The total score is computed from weighted components using the formula:
/// `total = 0.25*size + 0.25*complexity + 0.20*diffusion + 0.15*coverage + 0.15*file_type`,
/// clamped to `[0.0, 100.0]`.
///
/// # Examples
///
/// ```
/// use argus_core::RiskScore;
///
/// let score = RiskScore::new(80.0, 60.0, 40.0, 20.0, 10.0);
/// assert!(score.total >= 0.0 && score.total <= 100.0);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskScore {
    /// Weighted total risk (0–100).
    pub total: f64,
    /// Size component (0–100).
    pub size: f64,
    /// Complexity delta component (0–100).
    pub complexity: f64,
    /// Diffusion component (0–100).
    pub diffusion: f64,
    /// Coverage component (0–100).
    pub coverage: f64,
    /// File-type risk component (0–100).
    pub file_type: f64,
}

impl RiskScore {
    /// Create a new risk score from individual components.
    ///
    /// The total is computed automatically using the standard weighting formula
    /// and clamped to `[0.0, 100.0]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_core::RiskScore;
    ///
    /// let score = RiskScore::new(100.0, 100.0, 100.0, 100.0, 100.0);
    /// assert_eq!(score.total, 100.0);
    ///
    /// let zero = RiskScore::new(0.0, 0.0, 0.0, 0.0, 0.0);
    /// assert_eq!(zero.total, 0.0);
    /// ```
    pub fn new(size: f64, complexity: f64, diffusion: f64, coverage: f64, file_type: f64) -> Self {
        let total = (0.25 * size
            + 0.25 * complexity
            + 0.20 * diffusion
            + 0.15 * coverage
            + 0.15 * file_type)
            .clamp(0.0, 100.0);
        Self {
            total,
            size,
            complexity,
            diffusion,
            coverage,
            file_type,
        }
    }
}

/// Issue severity level for review comments.
///
/// # Examples
///
/// ```
/// use argus_core::Severity;
///
/// let s: Severity = serde_json::from_str("\"bug\"").unwrap();
/// assert_eq!(s, Severity::Bug);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A likely defect that should be fixed.
    Bug,
    /// A potential issue worth investigating.
    Warning,
    /// An optional improvement.
    Suggestion,
    /// Informational observation.
    Info,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Bug => write!(f, "bug"),
            Severity::Warning => write!(f, "warning"),
            Severity::Suggestion => write!(f, "suggestion"),
            Severity::Info => write!(f, "info"),
        }
    }
}

impl FromStr for Severity {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "bug" => Ok(Severity::Bug),
            "warning" => Ok(Severity::Warning),
            "suggestion" => Ok(Severity::Suggestion),
            "info" => Ok(Severity::Info),
            other => Err(format!("unknown severity: {other}")),
        }
    }
}

impl Severity {
    /// Returns `true` if `self` is at least as severe as `threshold`.
    ///
    /// Severity order: Bug > Warning > Suggestion > Info.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_core::Severity;
    ///
    /// assert!(Severity::Bug.meets_threshold(Severity::Warning));
    /// assert!(Severity::Warning.meets_threshold(Severity::Warning));
    /// assert!(!Severity::Suggestion.meets_threshold(Severity::Warning));
    /// ```
    pub fn meets_threshold(self, threshold: Severity) -> bool {
        self.rank() <= threshold.rank()
    }

    fn rank(self) -> u8 {
        match self {
            Severity::Bug => 0,
            Severity::Warning => 1,
            Severity::Suggestion => 2,
            Severity::Info => 3,
        }
    }
}

/// A single review comment produced by the AI reviewer.
///
/// # Examples
///
/// ```
/// use argus_core::{ReviewComment, Severity};
/// use std::path::PathBuf;
///
/// let comment = ReviewComment {
///     file_path: PathBuf::from("src/auth.rs"),
///     line: 42,
///     severity: Severity::Bug,
///     message: "Possible null dereference".into(),
///     confidence: 95.0,
///     suggestion: Some("Add a None check".into()),
/// };
/// assert_eq!(comment.severity, Severity::Bug);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewComment {
    /// Path to the file being commented on.
    pub file_path: PathBuf,
    /// Line number in the new version of the file.
    pub line: u32,
    /// Severity of the finding.
    pub severity: Severity,
    /// Explanation of the issue.
    pub message: String,
    /// LLM self-rated confidence (0–100).
    pub confidence: f64,
    /// Optional fix suggestion.
    pub suggestion: Option<String>,
}

/// A result from semantic code search.
///
/// # Examples
///
/// ```
/// use argus_core::SearchResult;
/// use std::path::PathBuf;
///
/// let result = SearchResult {
///     file_path: PathBuf::from("src/db.rs"),
///     line_start: 10,
///     line_end: 25,
///     snippet: "fn connect() { ... }".into(),
///     score: 0.92,
///     language: Some("rust".into()),
/// };
/// assert!(result.score > 0.9);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Path to the file containing the match.
    pub file_path: PathBuf,
    /// First line of the matched snippet.
    pub line_start: u32,
    /// Last line of the matched snippet.
    pub line_end: u32,
    /// The matched code snippet.
    pub snippet: String,
    /// Relevance score (0.0–1.0).
    pub score: f64,
    /// Detected language of the snippet.
    pub language: Option<String>,
}

/// Output format for CLI subcommands.
///
/// Implements [`FromStr`] so it can be used directly with `clap` argument parsing.
///
/// # Examples
///
/// ```
/// use argus_core::OutputFormat;
///
/// let fmt: OutputFormat = "json".parse().unwrap();
/// assert_eq!(fmt, OutputFormat::Json);
///
/// let fmt: OutputFormat = "md".parse().unwrap();
/// assert_eq!(fmt, OutputFormat::Markdown);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Human-readable tables and summaries.
    #[default]
    Text,
    /// Machine-readable JSON with camelCase keys.
    Json,
    /// Markdown-formatted output.
    Markdown,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputFormat::Text => write!(f, "text"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Markdown => write!(f, "markdown"),
        }
    }
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            "markdown" | "md" => Ok(OutputFormat::Markdown),
            other => Err(format!("unknown output format: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_from_str() {
        assert_eq!("text".parse::<OutputFormat>().unwrap(), OutputFormat::Text);
        assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert_eq!(
            "markdown".parse::<OutputFormat>().unwrap(),
            OutputFormat::Markdown
        );
        assert_eq!(
            "md".parse::<OutputFormat>().unwrap(),
            OutputFormat::Markdown
        );
        assert_eq!("JSON".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert!("xml".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn output_format_display() {
        assert_eq!(OutputFormat::Text.to_string(), "text");
        assert_eq!(OutputFormat::Json.to_string(), "json");
        assert_eq!(OutputFormat::Markdown.to_string(), "markdown");
    }

    #[test]
    fn output_format_default_is_text() {
        assert_eq!(OutputFormat::default(), OutputFormat::Text);
    }

    #[test]
    fn risk_score_weighted_formula() {
        let score = RiskScore::new(100.0, 100.0, 100.0, 100.0, 100.0);
        assert_eq!(score.total, 100.0);

        let score = RiskScore::new(0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(score.total, 0.0);

        let score = RiskScore::new(40.0, 60.0, 50.0, 30.0, 20.0);
        let expected = 0.25 * 40.0 + 0.25 * 60.0 + 0.20 * 50.0 + 0.15 * 30.0 + 0.15 * 20.0;
        assert!((score.total - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn risk_score_clamps_to_bounds() {
        let score = RiskScore::new(200.0, 200.0, 200.0, 200.0, 200.0);
        assert_eq!(score.total, 100.0);

        let score = RiskScore::new(-50.0, -50.0, -50.0, -50.0, -50.0);
        assert_eq!(score.total, 0.0);
    }

    #[test]
    fn severity_roundtrips_through_json() {
        let json = serde_json::to_string(&Severity::Bug).unwrap();
        assert_eq!(json, "\"bug\"");

        let parsed: Severity = serde_json::from_str("\"warning\"").unwrap();
        assert_eq!(parsed, Severity::Warning);
    }

    #[test]
    fn severity_from_str() {
        assert_eq!("bug".parse::<Severity>().unwrap(), Severity::Bug);
        assert_eq!("Warning".parse::<Severity>().unwrap(), Severity::Warning);
        assert_eq!(
            "SUGGESTION".parse::<Severity>().unwrap(),
            Severity::Suggestion
        );
        assert_eq!("info".parse::<Severity>().unwrap(), Severity::Info);
        assert!("unknown".parse::<Severity>().is_err());
    }

    #[test]
    fn severity_meets_threshold() {
        assert!(Severity::Bug.meets_threshold(Severity::Bug));
        assert!(Severity::Bug.meets_threshold(Severity::Warning));
        assert!(Severity::Bug.meets_threshold(Severity::Suggestion));
        assert!(Severity::Warning.meets_threshold(Severity::Warning));
        assert!(Severity::Warning.meets_threshold(Severity::Suggestion));
        assert!(!Severity::Warning.meets_threshold(Severity::Bug));
        assert!(!Severity::Suggestion.meets_threshold(Severity::Bug));
        assert!(!Severity::Suggestion.meets_threshold(Severity::Warning));
    }

    #[test]
    fn change_type_display() {
        assert_eq!(ChangeType::Add.to_string(), "add");
        assert_eq!(ChangeType::Delete.to_string(), "delete");
        assert_eq!(ChangeType::Modify.to_string(), "modify");
        assert_eq!(ChangeType::Move.to_string(), "move");
    }

    #[test]
    fn file_node_serializes_camel_case() {
        let node = FileNode {
            path: PathBuf::from("src/main.rs"),
            name: "main.rs".into(),
            language: Some("rust".into()),
            line_count: 100,
        };
        let json = serde_json::to_value(&node).unwrap();
        assert!(json.get("lineCount").is_some());
        assert!(json.get("line_count").is_none());
    }

    #[test]
    fn review_comment_serializes_camel_case() {
        let comment = ReviewComment {
            file_path: PathBuf::from("test.rs"),
            line: 1,
            severity: Severity::Info,
            message: "test".into(),
            confidence: 99.0,
            suggestion: None,
        };
        let json = serde_json::to_value(&comment).unwrap();
        assert!(json.get("filePath").is_some());
        assert!(json.get("file_path").is_none());
    }

    #[test]
    fn search_result_serializes_camel_case() {
        let result = SearchResult {
            file_path: PathBuf::from("lib.rs"),
            line_start: 1,
            line_end: 10,
            snippet: "code".into(),
            score: 0.5,
            language: None,
        };
        let json = serde_json::to_value(&result).unwrap();
        assert!(json.get("lineStart").is_some());
        assert!(json.get("fileePath").is_none());
    }
}
