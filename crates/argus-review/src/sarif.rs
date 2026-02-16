use argus_core::{ReviewComment, Severity};

use crate::pipeline::ReviewResult;

/// Convert a review result to SARIF v2.1.0 JSON.
///
/// Produces a standalone SARIF log with a single run containing all review
/// comments as results. Intended for upload to GitHub Code Scanning via
/// `github/codeql-action/upload-sarif`.
///
/// # Examples
///
/// ```
/// use argus_review::pipeline::{ReviewResult, ReviewStats};
/// use argus_review::sarif::to_sarif;
///
/// let result = ReviewResult {
///     comments: vec![],
///     filtered_comments: vec![],
///     summary: None,
///     stats: ReviewStats {
///         files_reviewed: 0,
///         files_skipped: 0,
///         total_hunks: 0,
///         comments_generated: 0,
///         comments_filtered: 0,
///         comments_deduplicated: 0,
///         comments_reflected_out: 0,
///         skipped_files: vec![],
///         model_used: "gpt-4o".into(),
///         llm_calls: 0,
///         file_groups: vec![],
///         hotspot_files: 0,
///     },
/// };
/// let sarif = to_sarif(&result);
/// assert_eq!(sarif["version"], "2.1.0");
/// ```
pub fn to_sarif(result: &ReviewResult) -> serde_json::Value {
    let rules = build_rules(&result.comments);
    let results: Vec<serde_json::Value> = result
        .comments
        .iter()
        .map(|c| {
            let mut entry = serde_json::json!({
                "ruleId": format!("argus/{}", severity_to_rule_id(c.severity)),
                "level": severity_to_sarif_level(c.severity),
                "message": { "text": &c.message },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": {
                            "uri": c.file_path.display().to_string()
                        },
                        "region": {
                            "startLine": c.line
                        }
                    }
                }]
            });
            if let Some(patch) = &c.patch {
                entry["fixes"] = serde_json::json!([{
                    "description": { "text": "Suggested fix" },
                    "artifactChanges": [{
                        "artifactLocation": {
                            "uri": c.file_path.display().to_string()
                        },
                        "replacements": [{
                            "deletedRegion": {
                                "startLine": c.line
                            },
                            "insertedContent": {
                                "text": patch
                            }
                        }]
                    }]
                }]);
            }
            entry
        })
        .collect();

    serde_json::json!({
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "argus",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/Meru143/argus",
                    "rules": rules
                }
            },
            "results": results
        }]
    })
}

fn severity_to_sarif_level(severity: Severity) -> &'static str {
    match severity {
        Severity::Bug => "error",
        Severity::Warning => "warning",
        Severity::Suggestion => "note",
        Severity::Info => "note",
    }
}

fn severity_to_rule_id(severity: Severity) -> &'static str {
    match severity {
        Severity::Bug => "bug",
        Severity::Warning => "warning",
        Severity::Suggestion => "suggestion",
        Severity::Info => "info",
    }
}

/// Build the SARIF `rules` array from the set of comments present.
///
/// Deduplicates by severity so each rule ID appears at most once.
fn build_rules(comments: &[ReviewComment]) -> Vec<serde_json::Value> {
    let mut seen = [false; 4];
    let mut rules = Vec::new();

    for c in comments {
        let idx = match c.severity {
            Severity::Bug => 0,
            Severity::Warning => 1,
            Severity::Suggestion => 2,
            Severity::Info => 3,
        };
        if seen[idx] {
            continue;
        }
        seen[idx] = true;

        let (id, name, level) = match c.severity {
            Severity::Bug => ("argus/bug", "Bug", "error"),
            Severity::Warning => ("argus/warning", "Warning", "warning"),
            Severity::Suggestion => ("argus/suggestion", "Suggestion", "note"),
            Severity::Info => ("argus/info", "Info", "note"),
        };

        rules.push(serde_json::json!({
            "id": id,
            "shortDescription": { "text": name },
            "defaultConfiguration": { "level": level }
        }));
    }

    rules
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use argus_core::{ReviewComment, Severity};

    use super::*;
    use crate::pipeline::{ReviewResult, ReviewStats};

    fn make_result(comments: Vec<ReviewComment>) -> ReviewResult {
        ReviewResult {
            comments,
            filtered_comments: vec![],
            summary: None,
            stats: ReviewStats {
                files_reviewed: 1,
                files_skipped: 0,
                total_hunks: 1,
                comments_generated: 1,
                comments_filtered: 0,
                comments_deduplicated: 0,
                comments_reflected_out: 0,
                skipped_files: vec![],
                model_used: "test".into(),
                llm_calls: 1,
                file_groups: vec![],
                hotspot_files: 0,
            },
        }
    }

    #[test]
    fn sarif_has_required_fields() {
        let result = make_result(vec![]);
        let sarif = to_sarif(&result);

        assert_eq!(sarif["version"], "2.1.0");
        assert!(sarif["$schema"].as_str().unwrap().contains("sarif-schema"));
        assert!(sarif["runs"].is_array());
        assert_eq!(sarif["runs"].as_array().unwrap().len(), 1);

        let run = &sarif["runs"][0];
        assert_eq!(run["tool"]["driver"]["name"], "argus");
        assert!(run["results"].is_array());
    }

    #[test]
    fn sarif_empty_results_valid() {
        let result = make_result(vec![]);
        let sarif = to_sarif(&result);

        let results = sarif["runs"][0]["results"].as_array().unwrap();
        assert!(results.is_empty());

        let rules = sarif["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn sarif_severity_mapping() {
        assert_eq!(severity_to_sarif_level(Severity::Bug), "error");
        assert_eq!(severity_to_sarif_level(Severity::Warning), "warning");
        assert_eq!(severity_to_sarif_level(Severity::Suggestion), "note");
        assert_eq!(severity_to_sarif_level(Severity::Info), "note");
    }

    #[test]
    fn sarif_comments_mapped_correctly() {
        let comments = vec![
            ReviewComment {
                file_path: PathBuf::from("src/auth.rs"),
                line: 42,
                severity: Severity::Bug,
                message: "Null dereference".into(),
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
        let result = make_result(comments);
        let sarif = to_sarif(&result);

        let results = sarif["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);

        assert_eq!(results[0]["ruleId"], "argus/bug");
        assert_eq!(results[0]["level"], "error");
        assert_eq!(results[0]["message"]["text"], "Null dereference");
        let loc = &results[0]["locations"][0]["physicalLocation"];
        assert_eq!(loc["artifactLocation"]["uri"], "src/auth.rs");
        assert_eq!(loc["region"]["startLine"], 42);

        assert_eq!(results[1]["ruleId"], "argus/warning");
        assert_eq!(results[1]["level"], "warning");
        assert_eq!(
            results[1]["locations"][0]["physicalLocation"]["region"]["startLine"],
            10
        );
    }

    #[test]
    fn sarif_rules_deduplicated() {
        let comments = vec![
            ReviewComment {
                file_path: PathBuf::from("a.rs"),
                line: 1,
                severity: Severity::Bug,
                message: "bug 1".into(),
                confidence: 90.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
            ReviewComment {
                file_path: PathBuf::from("b.rs"),
                line: 2,
                severity: Severity::Bug,
                message: "bug 2".into(),
                confidence: 90.0,
                suggestion: None,
                patch: None,
                rule: None,
            },
        ];
        let result = make_result(comments);
        let sarif = to_sarif(&result);

        let rules = sarif["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0]["id"], "argus/bug");
    }
}
