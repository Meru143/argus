use std::fmt;

use argus_core::{ArgusError, ReviewComment, ReviewConfig, Severity};
use serde::Serialize;

use argus_difflens::parser::FileDiff;

use crate::llm::{ChatMessage, LlmClient, Role};
use crate::prompt;

/// Result of a completed code review.
///
/// # Examples
///
/// ```
/// use argus_review::pipeline::{ReviewResult, ReviewStats};
///
/// let result = ReviewResult {
///     comments: vec![],
///     stats: ReviewStats {
///         files_reviewed: 0,
///         total_hunks: 0,
///         comments_generated: 0,
///         comments_filtered: 0,
///         model_used: "gpt-4o".into(),
///     },
/// };
/// assert!(result.comments.is_empty());
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult {
    /// Filtered and sorted review comments.
    pub comments: Vec<ReviewComment>,
    /// Statistics about the review run.
    pub stats: ReviewStats,
}

/// Statistics about a review run.
///
/// # Examples
///
/// ```
/// use argus_review::pipeline::ReviewStats;
///
/// let stats = ReviewStats {
///     files_reviewed: 3,
///     total_hunks: 5,
///     comments_generated: 10,
///     comments_filtered: 7,
///     model_used: "gpt-4o".into(),
/// };
/// assert_eq!(stats.comments_generated - stats.comments_filtered, 3);
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewStats {
    /// Number of files that were reviewed.
    pub files_reviewed: usize,
    /// Total number of diff hunks sent.
    pub total_hunks: usize,
    /// Raw comments from the LLM before filtering.
    pub comments_generated: usize,
    /// Comments removed by confidence/severity filters.
    pub comments_filtered: usize,
    /// Model identifier used for the review.
    pub model_used: String,
}

/// Review orchestrator that drives the full review pipeline.
///
/// Concatenates diffs, sends them to the LLM, parses the response,
/// and applies confidence/severity filtering.
pub struct ReviewPipeline {
    llm: LlmClient,
    config: ReviewConfig,
}

impl ReviewPipeline {
    /// Create a new pipeline from an LLM client and review config.
    pub fn new(llm: LlmClient, config: ReviewConfig) -> Self {
        Self { llm, config }
    }

    /// Run a review on parsed diffs and return filtered comments.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Llm`] if the LLM call fails.
    pub async fn review(&self, diffs: &[FileDiff]) -> Result<ReviewResult, ArgusError> {
        let files_reviewed = diffs.len();
        let total_hunks: usize = diffs.iter().map(|d| d.hunks.len()).sum();

        let diff_text = diffs_to_text(diffs);
        let system = prompt::build_system_prompt();
        let user = prompt::build_review_prompt(&diff_text, None);

        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: system,
            },
            ChatMessage {
                role: Role::User,
                content: user,
            },
        ];

        let response = self.llm.chat(messages).await?;
        let raw_comments = prompt::parse_review_response(&response)?;
        let comments_generated = raw_comments.len();

        let (filtered, comments_filtered) = filter_and_sort(raw_comments, &self.config);

        Ok(ReviewResult {
            comments: filtered,
            stats: ReviewStats {
                files_reviewed,
                total_hunks,
                comments_generated,
                comments_filtered,
                model_used: self.llm.model().to_string(),
            },
        })
    }
}

fn diffs_to_text(diffs: &[FileDiff]) -> String {
    use std::fmt::Write;
    let mut text = String::new();
    for diff in diffs {
        let _ = writeln!(text, "--- a/{}", diff.old_path.display());
        let _ = writeln!(text, "+++ b/{}", diff.new_path.display());
        for hunk in &diff.hunks {
            let _ = writeln!(
                text,
                "@@ -{},{} +{},{} @@",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            );
            text.push_str(&hunk.content);
        }
    }
    text
}

fn filter_and_sort(
    comments: Vec<ReviewComment>,
    config: &ReviewConfig,
) -> (Vec<ReviewComment>, usize) {
    let before = comments.len();

    let mut kept: Vec<ReviewComment> = Vec::new();
    for comment in comments {
        if comment.confidence < config.min_confidence {
            continue;
        }
        if !config.severity_filter.contains(&comment.severity) {
            continue;
        }
        kept.push(comment);
    }

    kept.sort_by_key(|c| severity_rank(c.severity));

    kept.truncate(config.max_comments);
    let filtered = before - kept.len();
    (kept, filtered)
}

fn severity_rank(s: Severity) -> u8 {
    match s {
        Severity::Bug => 0,
        Severity::Warning => 1,
        Severity::Suggestion => 2,
        Severity::Info => 3,
    }
}

impl fmt::Display for ReviewResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Review Results")?;
        writeln!(f, "==============")?;
        writeln!(
            f,
            "Model: {} | Files: {} | Hunks: {} | Comments: {} (filtered: {})\n",
            self.stats.model_used,
            self.stats.files_reviewed,
            self.stats.total_hunks,
            self.comments.len(),
            self.stats.comments_filtered,
        )?;

        if self.comments.is_empty() {
            writeln!(f, "No issues found.")?;
        } else {
            for c in &self.comments {
                let label = match c.severity {
                    Severity::Bug => "BUG",
                    Severity::Warning => "WARNING",
                    Severity::Suggestion => "SUGGESTION",
                    Severity::Info => "INFO",
                };
                writeln!(
                    f,
                    "[{label}] {}:{} (confidence: {:.0}%)",
                    c.file_path.display(),
                    c.line,
                    c.confidence,
                )?;
                writeln!(f, "  {}", c.message)?;
                if let Some(s) = &c.suggestion {
                    writeln!(f, "  Suggestion: {s}")?;
                }
                writeln!(f)?;
            }
        }

        Ok(())
    }
}

impl ReviewResult {
    /// Render the review result as markdown.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_review::pipeline::{ReviewResult, ReviewStats};
    ///
    /// let result = ReviewResult {
    ///     comments: vec![],
    ///     stats: ReviewStats {
    ///         files_reviewed: 0,
    ///         total_hunks: 0,
    ///         comments_generated: 0,
    ///         comments_filtered: 0,
    ///         model_used: "gpt-4o".into(),
    ///     },
    /// };
    /// let md = result.to_markdown();
    /// assert!(md.contains("# Review Results"));
    /// ```
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Review Results\n\n");
        out.push_str(&format!(
            "**Model:** {} | **Files:** {} | **Hunks:** {} | **Comments:** {} (filtered: {})\n\n",
            self.stats.model_used,
            self.stats.files_reviewed,
            self.stats.total_hunks,
            self.comments.len(),
            self.stats.comments_filtered,
        ));

        if self.comments.is_empty() {
            out.push_str("No issues found.\n");
        } else {
            for c in &self.comments {
                let emoji = match c.severity {
                    Severity::Bug => "\u{1f41b}",
                    Severity::Warning => "\u{26a0}\u{fe0f}",
                    Severity::Suggestion => "\u{1f4a1}",
                    Severity::Info => "\u{2139}\u{fe0f}",
                };
                let label = match c.severity {
                    Severity::Bug => "Bug",
                    Severity::Warning => "Warning",
                    Severity::Suggestion => "Suggestion",
                    Severity::Info => "Info",
                };
                out.push_str(&format!(
                    "## {emoji} {label} — `{}:{}` ({:.0}%)\n\n",
                    c.file_path.display(),
                    c.line,
                    c.confidence,
                ));
                out.push_str(&format!("{}\n\n", c.message));
                if let Some(s) = &c.suggestion {
                    out.push_str(&format!("> **Suggestion:** {s}\n\n"));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_comments() -> Vec<ReviewComment> {
        vec![
            ReviewComment {
                file_path: PathBuf::from("a.rs"),
                line: 1,
                severity: Severity::Info,
                message: "info comment".into(),
                confidence: 95.0,
                suggestion: None,
            },
            ReviewComment {
                file_path: PathBuf::from("b.rs"),
                line: 10,
                severity: Severity::Bug,
                message: "real bug".into(),
                confidence: 98.0,
                suggestion: Some("fix it".into()),
            },
            ReviewComment {
                file_path: PathBuf::from("c.rs"),
                line: 20,
                severity: Severity::Warning,
                message: "potential issue".into(),
                confidence: 85.0,
                suggestion: None,
            },
            ReviewComment {
                file_path: PathBuf::from("d.rs"),
                line: 30,
                severity: Severity::Bug,
                message: "low confidence bug".into(),
                confidence: 50.0,
                suggestion: None,
            },
        ]
    }

    #[test]
    fn filter_removes_low_confidence() {
        let config = ReviewConfig {
            min_confidence: 90.0,
            severity_filter: vec![Severity::Bug, Severity::Warning, Severity::Info],
            max_comments: 10,
        };
        let (kept, filtered) = filter_and_sort(make_comments(), &config);
        // c.rs (85%) and d.rs (50%) should be removed
        assert_eq!(kept.len(), 2);
        assert_eq!(filtered, 2);
    }

    #[test]
    fn filter_removes_non_matching_severity() {
        let config = ReviewConfig {
            min_confidence: 0.0,
            severity_filter: vec![Severity::Bug, Severity::Warning],
            max_comments: 10,
        };
        let (kept, _) = filter_and_sort(make_comments(), &config);
        // Info comment should be removed
        for c in &kept {
            assert!(c.severity == Severity::Bug || c.severity == Severity::Warning);
        }
    }

    #[test]
    fn sort_by_severity_bug_first() {
        let config = ReviewConfig {
            min_confidence: 0.0,
            severity_filter: vec![
                Severity::Bug,
                Severity::Warning,
                Severity::Suggestion,
                Severity::Info,
            ],
            max_comments: 10,
        };
        let (kept, _) = filter_and_sort(make_comments(), &config);
        assert!(kept.len() >= 2);
        // Bugs should come before warnings/info
        assert_eq!(kept[0].severity, Severity::Bug);
    }

    #[test]
    fn truncate_to_max_comments() {
        let config = ReviewConfig {
            min_confidence: 0.0,
            severity_filter: vec![
                Severity::Bug,
                Severity::Warning,
                Severity::Suggestion,
                Severity::Info,
            ],
            max_comments: 2,
        };
        let (kept, _) = filter_and_sort(make_comments(), &config);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn display_and_markdown_output() {
        let result = ReviewResult {
            comments: vec![ReviewComment {
                file_path: PathBuf::from("test.rs"),
                line: 5,
                severity: Severity::Bug,
                message: "test bug".into(),
                confidence: 99.0,
                suggestion: Some("fix it".into()),
            }],
            stats: ReviewStats {
                files_reviewed: 1,
                total_hunks: 1,
                comments_generated: 1,
                comments_filtered: 0,
                model_used: "test".into(),
            },
        };
        let text = format!("{result}");
        assert!(text.contains("[BUG]"));
        assert!(text.contains("test.rs:5"));

        let md = result.to_markdown();
        assert!(md.contains("# Review Results"));
        assert!(md.contains("Bug"));
    }
}
