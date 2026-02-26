use std::path::PathBuf;

use argus_core::{ArgusError, ReviewComment, ReviewConfig, Rule, Severity};
use serde::{Deserialize, Serialize};

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
/// let prompt = build_system_prompt(&config, &[], &[]);
/// assert!(prompt.contains("Argus"));
/// assert!(prompt.contains("Maximum 5 comments"));
/// ```
pub fn build_system_prompt(
    config: &ReviewConfig,
    rules: &[Rule],
    negative_examples: &[String],
) -> String {
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
               \"suggestion\": \"Optional concrete fix\",\n\
               \"patch\": \"fn example() {{\\n    // corrected code here\\n}}\"\n\
             }}\n\
           ]\n\
         }}\n\
         \n\
         For each comment, if you can suggest a concrete fix, include a \"patch\" field with the corrected code snippet. \
         Only include the fixed lines, not the entire file. If you cannot suggest a fix, omit the field.\n\
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

    if !negative_examples.is_empty() {
        prompt.push_str("\n\n## User Preferences / Negative Examples\n\n");
        prompt.push_str("The user has explicitly rejected similar comments in the past. Do NOT report issues like these:\n\n");
        for ex in negative_examples {
            // Truncate long examples to avoid token bloat
            let display_ex = if ex.chars().count() > 200 {
                let truncated: String = ex.chars().take(200).collect();
                format!("{truncated}...")
            } else {
                ex.clone()
            };
            prompt.push_str(&format!("- \"{}\"\n", display_ex));
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
    patch: Option<String>,
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
            patch: c.patch.clone(),
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

/// Build a prompt for the self-reflection pass that evaluates review comments.
///
/// The self-reflection pass asks the LLM to critically evaluate its own
/// review comments, scoring each 1-10 for relevance and accuracy. Comments
/// scoring below the threshold are filtered out.
///
/// # Examples
///
/// ```
/// use argus_core::{ReviewComment, Severity};
/// use argus_review::prompt::build_self_reflection_prompt;
/// use std::path::PathBuf;
///
/// let comments = vec![ReviewComment {
///     file_path: PathBuf::from("src/lib.rs"),
///     line: 10,
///     severity: Severity::Bug,
///     message: "Null dereference".into(),
///     confidence: 95.0,
///     suggestion: None,
///     patch: None,
///     rule: None,
/// }];
/// let prompt = build_self_reflection_prompt(&comments, "+added line");
/// assert!(prompt.contains("Null dereference"));
/// assert!(prompt.contains("score"));
/// ```
pub fn build_self_reflection_prompt(comments: &[ReviewComment], diff: &str) -> String {
    use std::fmt::Write;

    let mut prompt = String::from(
        "You are a senior code reviewer evaluating AI-generated review comments for quality.\n\n\
         Below is a code diff and a set of review comments generated by an AI reviewer.\n\
         Your job is to critically evaluate EACH comment and score it 1-10 based on:\n\
         - Is the issue REAL and verifiable from the diff alone? (not speculative)\n\
         - Is it a genuine defect, not a style/formatting preference?\n\
         - Is the line number correct and does it reference actual changed code?\n\
         - Would a senior developer agree this is worth flagging?\n\n\
         Score guide:\n\
         - 9-10: Critical real bug, clearly verifiable\n\
         - 7-8: Legitimate concern worth raising\n\
         - 5-6: Somewhat speculative or minor\n\
         - 1-4: False positive, style nit, or not verifiable from the diff\n\n\
         Respond with a JSON object. No markdown fences, no explanation outside JSON:\n\
         {\n\
           \"evaluations\": [\n\
             {\n\
               \"index\": 0,\n\
               \"score\": 8,\n\
               \"reason\": \"Brief explanation of why this score\",\n\
               \"revised_severity\": \"bug\"\n\
             }\n\
           ]\n\
         }\n\n\
         The \"index\" field corresponds to the 0-based index of the comment below.\n\
         The \"revised_severity\" is optional — include it only if the severity should change.\n\
         If the original severity is correct, omit \"revised_severity\".\n\n\
         ## Review Comments to Evaluate\n\n",
    );

    for (i, c) in comments.iter().enumerate() {
        let _ = writeln!(
            prompt,
            "[{i}] [{severity}] {path}:{line} (confidence: {conf:.0}%)\n  {message}\n",
            severity = c.severity,
            path = c.file_path.display(),
            line = c.line,
            conf = c.confidence,
            message = c.message,
        );
    }

    let _ = write!(prompt, "\n## Original Diff\n\n```diff\n{diff}\n```");
    prompt
}

#[derive(Deserialize)]
struct SelfReflectionResponse {
    evaluations: Vec<SelfReflectionEval>,
}

#[derive(Deserialize)]
struct SelfReflectionEval {
    index: usize,
    score: u8,
    #[allow(dead_code)]
    reason: Option<String>,
    revised_severity: Option<String>,
}

/// Parse the self-reflection LLM response and return scored evaluations.
///
/// Returns a vec of `(index, score, optional_revised_severity)` tuples.
/// Invalid entries are silently skipped.
///
/// # Examples
///
/// ```
/// use argus_review::prompt::parse_self_reflection_response;
///
/// let json = r#"{"evaluations":[{"index":0,"score":8,"reason":"real bug"}]}"#;
/// let evals = parse_self_reflection_response(json).unwrap();
/// assert_eq!(evals.len(), 1);
/// assert_eq!(evals[0].0, 0);
/// assert_eq!(evals[0].1, 8);
/// ```
pub fn parse_self_reflection_response(
    response: &str,
) -> Result<Vec<(usize, u8, Option<Severity>)>, ArgusError> {
    let cleaned = strip_code_fences(response);

    let parsed: SelfReflectionResponse = match serde_json::from_str(cleaned) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("warning: failed to parse self-reflection response: {e}");
            return Ok(Vec::new());
        }
    };

    let mut results = Vec::new();
    for eval in parsed.evaluations {
        let score = eval.score.min(10);
        let revised_sev = eval
            .revised_severity
            .and_then(|s| match s.to_lowercase().as_str() {
                "bug" => Some(Severity::Bug),
                "warning" => Some(Severity::Warning),
                "suggestion" => Some(Severity::Suggestion),
                "info" => Some(Severity::Info),
                _ => None,
            });
        results.push((eval.index, score, revised_sev));
    }

    Ok(results)
}

/// Build the system prompt for PR description generation.
///
/// Instructs the LLM to generate a title, description, and labels from a diff.
///
/// # Examples
///
/// ```
/// use argus_review::prompt::build_describe_system_prompt;
///
/// let prompt = build_describe_system_prompt();
/// assert!(prompt.contains("pull request descriptions"));
/// ```
pub fn build_describe_system_prompt() -> String {
    "You are Argus, an expert at writing clear, informative pull request descriptions from code diffs.\n\
     \n\
     RULES — FOLLOW STRICTLY:\n\
     1. Read the diff carefully and understand what changed.\n\
     2. Generate a concise PR title (max 72 chars) using conventional commit format when appropriate (feat:, fix:, refactor:, docs:, chore:, etc.).\n\
     3. Write a description with:\n\
        - A one-sentence summary of what the PR does\n\
        - A bullet list of key changes\n\
        - Any notable considerations (breaking changes, migration needed, etc.)\n\
     4. Suggest 1-4 labels from common categories: bug, feature, refactor, docs, tests, ci, performance, security, breaking-change, dependencies.\n\
     5. Only suggest labels that genuinely apply.\n\
     \n\
     Respond with a JSON object. No markdown fences, no explanation outside JSON:\n\
     {\n\
       \"title\": \"feat: add user authentication\",\n\
       \"description\": \"Adds JWT-based authentication...\\n\\n## Changes\\n- Added auth middleware\\n- ...\",\n\
       \"labels\": [\"feature\", \"security\"]\n\
     }\n\
     \n\
     Keep the description professional and informative. Use markdown in the description field."
        .into()
}

/// Build the user prompt for PR description generation.
///
/// # Examples
///
/// ```
/// use argus_review::prompt::build_describe_prompt;
///
/// let prompt = build_describe_prompt("+new line", None, None);
/// assert!(prompt.contains("+new line"));
/// ```
pub fn build_describe_prompt(
    diff: &str,
    repo_map: Option<&str>,
    history_context: Option<&str>,
) -> String {
    let mut prompt = String::new();

    if let Some(map) = repo_map {
        prompt.push_str("Here is the codebase structure for context:\n\n```\n");
        prompt.push_str(map);
        prompt.push_str("```\n\n");
    }

    if let Some(history) = history_context {
        prompt.push_str("## Git History Context\n");
        prompt.push_str(history);
        prompt.push_str("\n\n");
    }

    prompt.push_str(&format!(
        "Generate a PR title, description, and labels for the following changes:\n\n```diff\n{diff}\n```\n"
    ));
    prompt
}

/// A generated PR description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrDescription {
    /// Suggested PR title.
    pub title: String,
    /// Suggested PR description body (markdown).
    pub description: String,
    /// Suggested labels.
    pub labels: Vec<String>,
}

/// Parse the LLM response for PR description generation.
///
/// # Examples
///
/// ```
/// use argus_review::prompt::parse_describe_response;
///
/// let json = r#"{"title":"fix: typo","description":"Fixes a typo.","labels":["docs"]}"#;
/// let desc = parse_describe_response(json).unwrap();
/// assert_eq!(desc.title, "fix: typo");
/// ```
pub fn parse_describe_response(response: &str) -> Result<PrDescription, ArgusError> {
    let cleaned = strip_code_fences(response);

    let parsed: PrDescription = serde_json::from_str(cleaned)
        .map_err(|e| ArgusError::Llm(format!("failed to parse PR description response: {e}")))?;

    Ok(parsed)
}

/// Build a prompt asking the LLM to summarize the review findings.
///
/// Takes the final review comments and the original diff text, producing
/// a prompt that asks for a 2-4 sentence summary covering risk level,
/// key themes, and merge safety.
///
/// # Examples
///
/// ```
/// use argus_core::{ReviewComment, Severity};
/// use argus_review::prompt::build_summary_prompt;
/// use std::path::PathBuf;
///
/// let comments = vec![ReviewComment {
///     file_path: PathBuf::from("src/lib.rs"),
///     line: 10,
///     severity: Severity::Bug,
///     message: "Null dereference".into(),
///     confidence: 95.0,
///     suggestion: None,
///     patch: None,
///     rule: None,
/// }];
/// let prompt = build_summary_prompt(&comments, "+added line");
/// assert!(prompt.contains("Null dereference"));
/// ```
pub fn build_summary_prompt(comments: &[ReviewComment], diff: &str) -> String {
    use std::fmt::Write;

    let mut prompt = String::from(
        "Given these review findings and the diff, write a 2-4 sentence summary. \
         Cover: overall risk level (low/medium/high/critical), key themes, \
         and whether the change is safe to merge. Return plain text, no JSON.\n\n\
         ## Review Findings\n\n",
    );
    for c in comments {
        let _ = writeln!(
            prompt,
            "- [{severity}] {path}:{line}: {message}",
            severity = c.severity,
            path = c.file_path.display(),
            line = c.line,
            message = c.message,
        );
    }
    let _ = write!(prompt, "\n## Diff\n\n```diff\n{diff}\n```");
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_contains_key_instructions() {
        let config = ReviewConfig::default();
        let prompt = build_system_prompt(&config, &[], &[]);
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
        let prompt = build_system_prompt(&config, &[], &[]);
        assert!(prompt.contains("Maximum 10 comments"));
    }

    #[test]
    fn system_prompt_reflects_include_suggestions() {
        let config = ReviewConfig {
            include_suggestions: true,
            ..ReviewConfig::default()
        };
        let prompt = build_system_prompt(&config, &[], &[]);
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
        let prompt = build_review_prompt(
            "+x",
            None,
            None,
            None,
            Some("This is an auth module"),
            false,
        );
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
        let prompt = build_system_prompt(&config, &rules, &[]);
        assert!(prompt.contains("Project-Specific Rules"));
        assert!(prompt.contains("no-unwrap"));
        assert!(prompt.contains("no-panic"));
        assert!(prompt.contains("Do not use .unwrap() in production code"));
        assert!(prompt.contains("Never use panic! in library code"));
    }

    #[test]
    fn system_prompt_no_rules_section_when_empty() {
        let config = ReviewConfig::default();
        let prompt = build_system_prompt(&config, &[], &[]);
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
        let prompt = build_system_prompt(&config, &rules, &[]);
        let bug_pos = prompt.find("critical-bug").unwrap();
        let warn_pos = prompt.find("warn-check").unwrap();
        let suggestion_pos = prompt.find("style-check").unwrap();
        assert!(bug_pos < warn_pos, "bug should appear before warning");
        assert!(
            warn_pos < suggestion_pos,
            "warning should appear before suggestion"
        );
    }

    #[test]
    fn summary_prompt_contains_comment_messages() {
        let comments = vec![
            ReviewComment {
                file_path: PathBuf::from("src/auth.rs"),
                line: 42,
                severity: Severity::Bug,
                message: "Null pointer dereference".into(),
                confidence: 95.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
            ReviewComment {
                file_path: PathBuf::from("src/db.rs"),
                line: 10,
                severity: Severity::Warning,
                message: "SQL injection risk".into(),
                confidence: 88.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
        ];
        let prompt = build_summary_prompt(&comments, "+added line");
        assert!(prompt.contains("Null pointer dereference"));
        assert!(prompt.contains("SQL injection risk"));
        assert!(prompt.contains("src/auth.rs"));
        assert!(prompt.contains("+added line"));
        assert!(prompt.contains("risk level"));
    }

    #[test]
    fn parse_response_with_patch() {
        let json = r#"{"comments": [{
            "file": "src/auth.rs",
            "line": 42,
            "severity": "bug",
            "message": "Null dereference",
            "confidence": 95,
            "suggestion": "Add a check",
            "patch": "if let Some(val) = maybe_val {\n    use(val);\n}"
        }]}"#;
        let comments = parse_review_response(json).unwrap();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].patch.is_some());
        assert!(comments[0].patch.as_ref().unwrap().contains("Some(val)"));
    }

    #[test]
    fn parse_response_without_patch() {
        let json = r#"{"comments": [{
            "file": "src/auth.rs",
            "line": 42,
            "severity": "bug",
            "message": "Null dereference",
            "confidence": 95
        }]}"#;
        let comments = parse_review_response(json).unwrap();
        assert_eq!(comments.len(), 1);
        assert!(comments[0].patch.is_none());
    }

    #[test]
    fn self_reflection_prompt_contains_comments_and_diff() {
        let comments = vec![
            ReviewComment {
                file_path: PathBuf::from("src/auth.rs"),
                line: 42,
                severity: Severity::Bug,
                message: "Null pointer dereference".into(),
                confidence: 95.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
            ReviewComment {
                file_path: PathBuf::from("src/db.rs"),
                line: 10,
                severity: Severity::Warning,
                message: "SQL injection risk".into(),
                confidence: 88.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
        ];
        let prompt = build_self_reflection_prompt(&comments, "+added line");
        assert!(prompt.contains("Null pointer dereference"));
        assert!(prompt.contains("SQL injection risk"));
        assert!(prompt.contains("[0]"));
        assert!(prompt.contains("[1]"));
        assert!(prompt.contains("+added line"));
        assert!(prompt.contains("score"));
        assert!(prompt.contains("1-10"));
    }

    #[test]
    fn parse_self_reflection_valid() {
        let json = r#"{"evaluations":[
            {"index":0,"score":9,"reason":"real bug"},
            {"index":1,"score":3,"reason":"style nit"}
        ]}"#;
        let evals = parse_self_reflection_response(json).unwrap();
        assert_eq!(evals.len(), 2);
        assert_eq!(evals[0], (0, 9, None));
        assert_eq!(evals[1], (1, 3, None));
    }

    #[test]
    fn parse_self_reflection_with_revised_severity() {
        let json = r#"{"evaluations":[
            {"index":0,"score":6,"reason":"downgrade","revised_severity":"suggestion"}
        ]}"#;
        let evals = parse_self_reflection_response(json).unwrap();
        assert_eq!(evals.len(), 1);
        assert_eq!(evals[0].0, 0);
        assert_eq!(evals[0].1, 6);
        assert_eq!(evals[0].2, Some(Severity::Suggestion));
    }

    #[test]
    fn parse_self_reflection_clamps_score() {
        let json = r#"{"evaluations":[{"index":0,"score":15,"reason":"overflow"}]}"#;
        let evals = parse_self_reflection_response(json).unwrap();
        assert_eq!(evals[0].1, 10);
    }

    #[test]
    fn parse_self_reflection_malformed_returns_empty() {
        let evals = parse_self_reflection_response("not json").unwrap();
        assert!(evals.is_empty());
    }

    #[test]
    fn parse_self_reflection_with_code_fences() {
        let fenced = "```json\n{\"evaluations\":[]}\n```";
        let evals = parse_self_reflection_response(fenced).unwrap();
        assert!(evals.is_empty());
    }

    #[test]
    fn describe_system_prompt_contains_key_instructions() {
        let prompt = build_describe_system_prompt();
        assert!(prompt.contains("pull request descriptions"));
        assert!(prompt.contains("title"));
        assert!(prompt.contains("labels"));
        assert!(prompt.contains("conventional commit"));
    }

    #[test]
    fn describe_prompt_includes_diff() {
        let prompt = build_describe_prompt("+added line", None, None);
        assert!(prompt.contains("+added line"));
        assert!(prompt.contains("```diff"));
    }

    #[test]
    fn describe_prompt_includes_repo_map() {
        let prompt = build_describe_prompt("+x", Some("src/main.rs\n  fn main()"), None);
        assert!(prompt.contains("src/main.rs"));
        assert!(prompt.contains("codebase structure"));
    }

    #[test]
    fn describe_prompt_includes_history() {
        let prompt = build_describe_prompt("+x", None, Some("- src/auth.rs: HOTSPOT"));
        assert!(prompt.contains("HOTSPOT"));
        assert!(prompt.contains("Git History Context"));
    }

    #[test]
    fn parse_describe_response_valid() {
        let json = r#"{"title":"feat: add auth","description":"Adds authentication.\n\n- JWT tokens\n- Middleware","labels":["feature","security"]}"#;
        let desc = parse_describe_response(json).unwrap();
        assert_eq!(desc.title, "feat: add auth");
        assert!(desc.description.contains("JWT tokens"));
        assert_eq!(desc.labels, vec!["feature", "security"]);
    }

    #[test]
    fn parse_describe_response_with_fences() {
        let json = "```json\n{\"title\":\"fix: typo\",\"description\":\"Fix.\",\"labels\":[]}\n```";
        let desc = parse_describe_response(json).unwrap();
        assert_eq!(desc.title, "fix: typo");
    }

    #[test]
    fn parse_describe_response_malformed() {
        let result = parse_describe_response("not json");
        assert!(result.is_err());
    }
}
