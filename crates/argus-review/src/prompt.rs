use std::path::PathBuf;

use argus_core::{ArgusError, ReviewComment, ReviewConfig, Rule, Severity};
use serde::Deserialize;

/// Build the system prompt for the code review LLM.
///
/// Incorporates `max_comments` and severity configuration from [`ReviewConfig`]
/// into the prompt text for better LLM adherence. When `rules` is non-empty,
/// appends a project-specific rules section so the LLM checks for custom
/// patterns defined by the project maintainers.
///
/// # Examples
///
/// ```
/// use argus_core::ReviewConfig;
/// use argus_review::prompt::build_system_prompt;
///
/// let config = ReviewConfig::default();
/// let prompt = build_system_prompt(&config, &[]);
/// assert!(prompt.contains("Argus"));
/// assert!(prompt.contains("Maximum 5 comments"));
/// ```
pub fn build_system_prompt(config: &ReviewConfig, rules: &[Rule]) -> String {
    let severity_note = if config.include_suggestions {
        "- suggestion: Improvement that doesn't affect correctness"
    } else {
        "- suggestion: Improvement that doesn't affect correctness (ONLY include if explicitly enabled)"
    };

    let mut prompt = format!(
        "You are Argus, an expert code reviewer specializing in detecting genuine defects in code changes.\n\
         \n\
         RULES — FOLLOW STRICTLY:\n\
         1. Only comment on issues you are CERTAIN about. If confidence is below 90%, do not include it.\n\
         2. Reference EXACT line numbers from the diff. Every comment MUST have a valid line number.\n\
         3. Do NOT speculate about code behavior you cannot verify from the diff alone.\n\
         4. Do NOT comment on: style, formatting, naming conventions, missing comments, or documentation.\n\
         5. Do NOT suggest adding tests unless the change breaks existing test assumptions.\n\
         6. Focus EXCLUSIVELY on: bugs, security vulnerabilities, logic errors, race conditions, resource leaks, null/None dereferences, integer overflow, off-by-one errors.\n\
         7. For each issue, explain WHY it's a problem with a concrete scenario.\n\
         8. Maximum {max_comments} comments. Prioritize by severity (bug > warning).\n\
         \n\
         SEVERITY DEFINITIONS:\n\
         - bug: Code that WILL produce incorrect behavior in a concrete scenario you can describe\n\
         - warning: Code that COULD produce incorrect behavior under specific conditions\n\
         {severity_note}\n\
         - info: Observation (NEVER include unless explicitly enabled)\n\
         \n\
         Respond with a JSON object. No markdown fences, no explanation outside JSON:\n\
         {{\n\
           \"comments\": [\n\
             {{\n\
               \"file\": \"exact/path/from/diff.rs\",\n\
               \"line\": 42,\n\
               \"severity\": \"bug\",\n\
               \"message\": \"Concrete explanation with scenario\",\n\
               \"confidence\": 95,\n\
               \"suggestion\": \"Optional concrete fix\"\n\
             }}\n\
           ]\n\
         }}\n\
         \n\
         If you find no issues worth reporting, return: {{\"comments\": []}}",
        max_comments = config.max_comments,
    );

    if !rules.is_empty() {
        let mut sorted_rules: Vec<&Rule> = rules.iter().collect();
        sorted_rules.sort_by_key(|r| match r.severity.as_str() {
            "bug" => 0u8,
            "warning" => 1,
            "suggestion" => 2,
            _ => 3,
        });

        prompt.push_str("\n\n## Project-Specific Rules\n\n");
        prompt.push_str("The following rules are defined by the project maintainers. Check for violations of each rule and report them with the specified severity.\n\n");
        for rule in &sorted_rules {
            prompt.push_str(&format!(
                "- [{}] {}: {}\n",
                rule.severity, rule.name, rule.description
            ));
        }
    }

    prompt
}

/// Build the user prompt containing the diff to review.
///
/// When `cross_file_review` is `true`, appends an instruction block asking
/// the LLM to look for cross-file issues such as API mismatches and
/// missing updates.
///
/// # Examples
///
/// ```
/// use argus_review::prompt::build_review_prompt;
///
/// let prompt = build_review_prompt("+new line", None, None, None, None, false);
/// assert!(prompt.contains("+new line"));
/// ```
pub fn build_review_prompt(
    diff: &str,
    repo_map: Option<&str>,
    related_code: Option<&str>,
    history_context: Option<&str>,
    file_context: Option<&str>,
    cross_file_review: bool,
) -> String {
    let mut prompt = String::new();

    if let Some(map) = repo_map {
        prompt.push_str("Here is the codebase structure for context:\n\n```\n");
        prompt.push_str(map);
        prompt.push_str("```\n\n");
    }

    if let Some(code) = related_code {
        prompt.push_str("Here is related code from the codebase that may be relevant:\n\n");
        prompt.push_str(code);
        prompt.push_str("\n\n");
    }

    if let Some(history) = history_context {
        prompt.push_str("## Git History Context\n");
        prompt.push_str(history);
        prompt.push_str("\n\n");
    }

    prompt.push_str(&format!(
        "Review the following code changes:\n\n```diff\n{diff}\n```\n"
    ));
    if let Some(ctx) = file_context {
        prompt.push_str(&format!("\nAdditional context:\n{ctx}\n"));
    }
    if cross_file_review {
        prompt.push_str(
            "\nIMPORTANT: These files are part of the same change and may be related.\n\
             Look for cross-file issues:\n\
             - Function/type signature changes not reflected in callers\n\
             - Inconsistent error handling across files\n\
             - Missing updates in related files\n\
             - API contract violations between modules\n",
        );
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
            rule: None,
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
        let config = ReviewConfig::default();
        let prompt = build_system_prompt(&config, &[]);
        assert!(prompt.contains("CERTAIN"));
        assert!(prompt.contains("line number"));
        assert!(prompt.contains("comments"));
        assert!(prompt.contains("Maximum 5 comments"));
    }

    #[test]
    fn system_prompt_reflects_max_comments() {
        let config = ReviewConfig {
            max_comments: 10,
            ..ReviewConfig::default()
        };
        let prompt = build_system_prompt(&config, &[]);
        assert!(prompt.contains("Maximum 10 comments"));
    }

    #[test]
    fn system_prompt_reflects_include_suggestions() {
        let config = ReviewConfig {
            include_suggestions: true,
            ..ReviewConfig::default()
        };
        let prompt = build_system_prompt(&config, &[]);
        // Should NOT contain the restriction about "ONLY include if explicitly enabled"
        assert!(!prompt.contains("ONLY include if explicitly enabled"));
    }

    #[test]
    fn review_prompt_includes_diff() {
        let prompt = build_review_prompt("+added line", None, None, None, None, false);
        assert!(prompt.contains("+added line"));
        assert!(prompt.contains("```diff"));
    }

    #[test]
    fn review_prompt_includes_context() {
        let prompt =
            build_review_prompt("+x", None, None, None, Some("This is an auth module"), false);
        assert!(prompt.contains("auth module"));
    }

    #[test]
    fn review_prompt_includes_related_code() {
        let prompt =
            build_review_prompt("+x", None, Some("fn authenticate() { }"), None, None, false);
        assert!(prompt.contains("authenticate"));
        assert!(prompt.contains("related code"));
    }

    #[test]
    fn review_prompt_includes_history_context() {
        let prompt = build_review_prompt(
            "+x",
            None,
            None,
            Some("- src/auth.rs: 47 revisions, HOTSPOT\n"),
            None,
            false,
        );
        assert!(prompt.contains("Git History Context"));
        assert!(prompt.contains("47 revisions"));
    }

    #[test]
    fn review_prompt_includes_cross_file_instruction() {
        let prompt = build_review_prompt("+x", None, None, None, None, true);
        assert!(prompt.contains("cross-file issues"));
        assert!(prompt.contains("API contract violations"));
    }

    #[test]
    fn review_prompt_omits_cross_file_when_disabled() {
        let prompt = build_review_prompt("+x", None, None, None, None, false);
        assert!(!prompt.contains("cross-file issues"));
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

    #[test]
    fn system_prompt_includes_rules() {
        let config = ReviewConfig::default();
        let rules = vec![
            Rule {
                name: "no-unwrap".into(),
                severity: "warning".into(),
                description: "Do not use .unwrap() in production code".into(),
            },
            Rule {
                name: "no-panic".into(),
                severity: "bug".into(),
                description: "Never use panic! in library code".into(),
            },
        ];
        let prompt = build_system_prompt(&config, &rules);
        assert!(prompt.contains("Project-Specific Rules"));
        assert!(prompt.contains("no-unwrap"));
        assert!(prompt.contains("no-panic"));
        assert!(prompt.contains("Do not use .unwrap() in production code"));
        assert!(prompt.contains("Never use panic! in library code"));
    }

    #[test]
    fn system_prompt_no_rules_section_when_empty() {
        let config = ReviewConfig::default();
        let prompt = build_system_prompt(&config, &[]);
        assert!(!prompt.contains("Project-Specific Rules"));
    }

    #[test]
    fn system_prompt_rules_sorted_by_severity() {
        let config = ReviewConfig::default();
        let rules = vec![
            Rule {
                name: "style-check".into(),
                severity: "suggestion".into(),
                description: "Check style".into(),
            },
            Rule {
                name: "warn-check".into(),
                severity: "warning".into(),
                description: "Check warnings".into(),
            },
            Rule {
                name: "critical-bug".into(),
                severity: "bug".into(),
                description: "Check bugs".into(),
            },
        ];
        let prompt = build_system_prompt(&config, &rules);
        let bug_pos = prompt.find("critical-bug").unwrap();
        let warn_pos = prompt.find("warn-check").unwrap();
        let suggestion_pos = prompt.find("style-check").unwrap();
        assert!(bug_pos < warn_pos, "bug should appear before warning");
        assert!(
            warn_pos < suggestion_pos,
            "warning should appear before suggestion"
        );
    }
}
