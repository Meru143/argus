use std::path::PathBuf;

use argus_core::{ArgusError, ReviewComment, Severity};
use serde::Deserialize;

const SYSTEM_PROMPT: &str = "\
You are Argus, an expert code reviewer. Your job is to find genuine bugs, \
security issues, and significant problems in code changes.

Rules:
- Only comment on issues you are CERTAIN about
- Reference specific line numbers from the diff
- Do not speculate about code behavior you cannot verify
- If unsure, do not comment
- Do not comment on style, formatting, or naming unless it creates a bug
- Focus on: bugs, security vulnerabilities, logic errors, race conditions, resource leaks

Respond with a JSON object:
{
  \"comments\": [
    {
      \"file\": \"path/to/file.rs\",
      \"line\": 42,
      \"severity\": \"bug\" | \"warning\" | \"suggestion\" | \"info\",
      \"message\": \"Clear explanation of the issue\",
      \"confidence\": 0-100,
      \"suggestion\": \"Optional fix suggestion\"
    }
  ]
}

If you find no issues, return: { \"comments\": [] }";

/// Build the system prompt for the code review LLM.
///
/// # Examples
///
/// ```
/// use argus_review::prompt::build_system_prompt;
///
/// let prompt = build_system_prompt();
/// assert!(prompt.contains("Argus"));
/// assert!(prompt.contains("bugs"));
/// ```
pub fn build_system_prompt() -> String {
    SYSTEM_PROMPT.to_string()
}

/// Build the user prompt containing the diff to review.
///
/// # Examples
///
/// ```
/// use argus_review::prompt::build_review_prompt;
///
/// let prompt = build_review_prompt("+new line", None);
/// assert!(prompt.contains("+new line"));
/// ```
pub fn build_review_prompt(diff: &str, file_context: Option<&str>) -> String {
    let mut prompt = format!("Review the following code changes:\n\n```diff\n{diff}\n```\n");
    if let Some(ctx) = file_context {
        prompt.push_str(&format!("\nAdditional context:\n{ctx}\n"));
    }
    prompt
}

#[derive(Deserialize)]
struct LlmResponse {
    comments: Vec<LlmComment>,
}

#[derive(Deserialize)]
struct LlmComment {
    file: String,
    line: Option<serde_json::Value>,
    severity: String,
    message: String,
    confidence: Option<serde_json::Value>,
    suggestion: Option<String>,
}

/// Parse the LLM JSON response into validated [`ReviewComment`] entries.
///
/// Handles markdown code fences around JSON. Returns an empty vec on
/// parse failure rather than propagating the error.
///
/// # Examples
///
/// ```
/// use argus_review::prompt::parse_review_response;
///
/// let json = r#"{"comments":[]}"#;
/// let comments = parse_review_response(json).unwrap();
/// assert!(comments.is_empty());
/// ```
pub fn parse_review_response(response: &str) -> Result<Vec<ReviewComment>, ArgusError> {
    let cleaned = strip_code_fences(response);

    let parsed: LlmResponse = match serde_json::from_str(cleaned) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("warning: failed to parse LLM response: {e}");
            return Ok(Vec::new());
        }
    };

    let mut comments = Vec::new();
    for c in parsed.comments {
        let line = match &c.line {
            Some(serde_json::Value::Number(n)) => {
                let Some(l) = n.as_u64() else { continue };
                if l == 0 {
                    continue;
                }
                l as u32
            }
            _ => continue,
        };

        let severity = match c.severity.to_lowercase().as_str() {
            "bug" => Severity::Bug,
            "warning" => Severity::Warning,
            "suggestion" => Severity::Suggestion,
            "info" => Severity::Info,
            _ => continue,
        };

        let confidence = match &c.confidence {
            Some(serde_json::Value::Number(n)) => {
                let v = n.as_f64().unwrap_or(0.0);
                v.clamp(0.0, 100.0)
            }
            _ => 50.0,
        };

        comments.push(ReviewComment {
            file_path: PathBuf::from(&c.file),
            line,
            severity,
            message: c.message.clone(),
            confidence,
            suggestion: c.suggestion.clone(),
        });
    }

    Ok(comments)
}

fn strip_code_fences(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_contains_key_instructions() {
        let prompt = build_system_prompt();
        assert!(prompt.contains("CERTAIN"));
        assert!(prompt.contains("line numbers"));
        assert!(prompt.contains("comments"));
    }

    #[test]
    fn review_prompt_includes_diff() {
        let prompt = build_review_prompt("+added line", None);
        assert!(prompt.contains("+added line"));
        assert!(prompt.contains("```diff"));
    }

    #[test]
    fn review_prompt_includes_context() {
        let prompt = build_review_prompt("+x", Some("This is an auth module"));
        assert!(prompt.contains("auth module"));
    }

    #[test]
    fn parse_valid_response() {
        let json = r#"{
            "comments": [
                {
                    "file": "src/auth.rs",
                    "line": 42,
                    "severity": "bug",
                    "message": "Null dereference",
                    "confidence": 95,
                    "suggestion": "Add a check"
                },
                {
                    "file": "src/db.rs",
                    "line": 10,
                    "severity": "warning",
                    "message": "SQL injection risk",
                    "confidence": 88
                }
            ]
        }"#;
        let comments = parse_review_response(json).unwrap();
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].severity, Severity::Bug);
        assert_eq!(comments[0].line, 42);
        assert_eq!(comments[0].confidence, 95.0);
        assert_eq!(comments[1].severity, Severity::Warning);
    }

    #[test]
    fn parse_empty_comments() {
        let json = r#"{"comments":[]}"#;
        let comments = parse_review_response(json).unwrap();
        assert!(comments.is_empty());
    }

    #[test]
    fn parse_with_code_fences() {
        let fenced = "```json\n{\"comments\":[]}\n```";
        let comments = parse_review_response(fenced).unwrap();
        assert!(comments.is_empty());
    }

    #[test]
    fn parse_malformed_returns_empty() {
        let garbage = "this is not json at all";
        let comments = parse_review_response(garbage).unwrap();
        assert!(comments.is_empty());
    }

    #[test]
    fn parse_skips_invalid_entries() {
        let json = r#"{
            "comments": [
                {"file": "a.rs", "line": 0, "severity": "bug", "message": "bad line", "confidence": 90},
                {"file": "b.rs", "line": 5, "severity": "invalid", "message": "bad severity", "confidence": 90},
                {"file": "c.rs", "line": 10, "severity": "bug", "message": "valid", "confidence": 95}
            ]
        }"#;
        let comments = parse_review_response(json).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].file_path, PathBuf::from("c.rs"));
    }

    #[test]
    fn parse_clamps_confidence() {
        let json = r#"{"comments":[
            {"file":"a.rs","line":1,"severity":"info","message":"x","confidence":150}
        ]}"#;
        let comments = parse_review_response(json).unwrap();
        assert_eq!(comments[0].confidence, 100.0);
    }
}
