use std::collections::HashMap;
use std::path::Path;

use argus_core::{ArgusError, ReviewComment};

/// A successfully applied patch.
pub struct AppliedPatch {
    /// Path to the patched file.
    pub file_path: String,
    /// Line number targeted by the patch.
    pub line: usize,
    /// The review comment message.
    pub message: String,
}

/// A patch that was skipped.
pub struct SkippedPatch {
    /// Path to the file the patch targeted.
    pub file_path: String,
    /// Line number targeted by the patch.
    pub line: usize,
    /// Why the patch was skipped.
    pub reason: String,
}

/// Outcome of applying patches from review comments.
pub struct PatchResult {
    /// Patches that were applied successfully.
    pub applied: Vec<AppliedPatch>,
    /// Patches that were skipped.
    pub skipped: Vec<SkippedPatch>,
}

/// Apply patches from review comments to the working tree.
/// Only applies comments that have a non-empty `patch` field.
/// Uses simple line-based replacement: reads the file, finds the target line range, replaces with patch content.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use argus_core::ReviewComment;
/// use argus_review::patch::apply_patches;
///
/// let comments: Vec<ReviewComment> = vec![];
/// let result = apply_patches(&comments, Path::new(".")).unwrap();
/// assert!(result.applied.is_empty());
/// ```
pub fn apply_patches(
    comments: &[ReviewComment],
    repo_root: &Path,
) -> Result<PatchResult, ArgusError> {
    let mut applied: Vec<AppliedPatch> = Vec::new();
    let mut skipped: Vec<SkippedPatch> = Vec::new();

    // Collect comments that have patches, grouped by file
    let mut patches_by_file: HashMap<String, Vec<&ReviewComment>> = HashMap::new();
    for comment in comments {
        let Some(patch) = &comment.patch else {
            continue;
        };
        if patch.is_empty() {
            continue;
        }
        let key = comment.file_path.to_string_lossy().to_string();
        patches_by_file.entry(key).or_default().push(comment);
    }

    // Process each file
    for (file_path_str, mut file_comments) in patches_by_file {
        let full_path = repo_root.join(&file_path_str);

        let file_content = match std::fs::read_to_string(&full_path) {
            Ok(content) => content,
            Err(e) => {
                for comment in &file_comments {
                    skipped.push(SkippedPatch {
                        file_path: file_path_str.clone(),
                        line: comment.line as usize,
                        reason: format!("cannot read file: {e}"),
                    });
                }
                continue;
            }
        };

        let mut lines: Vec<String> = file_content.lines().map(String::from).collect();

        // Sort by line number descending (bottom-up) to avoid offset issues
        file_comments.sort_by(|a, b| b.line.cmp(&a.line));

        for comment in &file_comments {
            let patch_content = comment.patch.as_deref().unwrap();
            let target_line = comment.line as usize;

            if target_line == 0 || target_line > lines.len() {
                skipped.push(SkippedPatch {
                    file_path: file_path_str.clone(),
                    line: target_line,
                    reason: format!(
                        "line {} out of range (file has {} lines)",
                        target_line,
                        lines.len()
                    ),
                });
                continue;
            }

            // Replace only the single target line with the patch content.
            // The patch is a replacement snippet for the flagged line; using
            // the patch line count to determine how many original lines to
            // remove is incorrect because the patch may expand a single line
            // into multiple lines (or vice-versa). Replacing just the target
            // line is the safest approach to avoid corrupting surrounding code.
            let patch_lines: Vec<&str> = patch_content.lines().collect();

            // Replace starting at the target line (1-indexed)
            let start_idx = target_line - 1;

            lines.splice(
                start_idx..start_idx + 1,
                patch_lines.iter().map(|l| l.to_string()),
            );

            applied.push(AppliedPatch {
                file_path: file_path_str.clone(),
                line: target_line,
                message: comment.message.clone(),
            });
        }

        // Write the modified file back
        let new_content = if file_content.ends_with('\n') {
            format!("{}\n", lines.join("\n"))
        } else {
            lines.join("\n")
        };
        std::fs::write(&full_path, new_content).map_err(|e| {
            ArgusError::Config(format!("failed to write {}: {e}", full_path.display()))
        })?;
    }

    Ok(PatchResult { applied, skipped })
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_core::Severity;
    use std::path::PathBuf;

    fn make_comment(
        file_path: &str,
        line: u32,
        patch: Option<&str>,
        message: &str,
    ) -> ReviewComment {
        ReviewComment {
            file_path: PathBuf::from(file_path),
            line,
            severity: Severity::Warning,
            message: message.into(),
            confidence: 90.0,
            suggestion: None,
            patch: patch.map(String::from),
            rule: None,
        }
    }

    #[test]
    fn test_apply_single_patch() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("example.rs");
        std::fs::write(&file_path, "fn main() {\n    println!(\"old\");\n}\n").unwrap();

        let comments = vec![make_comment(
            "example.rs",
            2,
            Some("    println!(\"new\");"),
            "use new message",
        )];

        let result = apply_patches(&comments, dir.path()).unwrap();
        assert_eq!(result.applied.len(), 1);
        assert_eq!(result.skipped.len(), 0);

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("\"new\""));
        assert!(!content.contains("\"old\""));
    }

    #[test]
    fn test_skip_no_patch() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("example.rs");
        std::fs::write(&file_path, "fn main() {}\n").unwrap();

        let comments = vec![make_comment("example.rs", 1, None, "no patch here")];

        let result = apply_patches(&comments, dir.path()).unwrap();
        assert_eq!(result.applied.len(), 0);
        assert_eq!(result.skipped.len(), 0);
    }

    #[test]
    fn test_skip_missing_file() {
        let dir = tempfile::tempdir().unwrap();

        let comments = vec![make_comment(
            "nonexistent.rs",
            1,
            Some("replacement"),
            "file missing",
        )];

        let result = apply_patches(&comments, dir.path()).unwrap();
        assert_eq!(result.applied.len(), 0);
        assert_eq!(result.skipped.len(), 1);
        assert!(result.skipped[0].reason.contains("cannot read file"));
    }

    #[test]
    fn test_multiple_patches_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("multi.rs");
        std::fs::write(&file_path, "line1\nline2\nline3\nline4\nline5\n").unwrap();

        let comments = vec![
            make_comment("multi.rs", 2, Some("patched2"), "fix line 2"),
            make_comment("multi.rs", 4, Some("patched4"), "fix line 4"),
        ];

        let result = apply_patches(&comments, dir.path()).unwrap();
        assert_eq!(result.applied.len(), 2);
        assert_eq!(result.skipped.len(), 0);

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("patched2"));
        assert!(content.contains("patched4"));
        assert!(content.contains("line1"));
        assert!(content.contains("line3"));
        assert!(content.contains("line5"));
        assert!(!content.contains("\nline2\n"));
        assert!(!content.contains("\nline4\n"));
    }

    #[test]
    fn test_multiline_patch_expands_single_line() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("expand.rs");
        std::fs::write(
            &file_path,
            "fn main() {\n    old_call();\n    keep_me();\n}\n",
        )
        .unwrap();

        // A multi-line patch replacing a single target line should NOT
        // overwrite the lines that follow.
        let comments = vec![make_comment(
            "expand.rs",
            2,
            Some("    let x = setup();\n    new_call(x);"),
            "expand one line to two",
        )];

        let result = apply_patches(&comments, dir.path()).unwrap();
        assert_eq!(result.applied.len(), 1);
        assert_eq!(result.skipped.len(), 0);

        let content = std::fs::read_to_string(&file_path).unwrap();
        // The patch should have replaced only line 2, inserting 2 new lines
        assert!(content.contains("setup()"));
        assert!(content.contains("new_call(x)"));
        // Line 3 ("keep_me()") must NOT be overwritten
        assert!(
            content.contains("keep_me()"),
            "multi-line patch must not overwrite subsequent lines"
        );
    }
}
