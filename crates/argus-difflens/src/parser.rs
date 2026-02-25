use std::fmt;
use std::path::PathBuf;

use argus_core::{ArgusError, ChangeType, DiffHunk};

/// A complete diff for a single file, containing one or more hunks.
///
/// # Examples
///
/// ```
/// use argus_difflens::parser::{parse_unified_diff, FileDiff};
///
/// let diff = "diff --git a/hello.rs b/hello.rs\n\
///             --- a/hello.rs\n\
///             +++ b/hello.rs\n\
///             @@ -1,3 +1,4 @@\n\
///              fn main() {\n\
///             +    println!(\"hello\");\n\
///              }\n";
/// let files = parse_unified_diff(diff).unwrap();
/// assert_eq!(files.len(), 1);
/// assert_eq!(files[0].hunks.len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// Path in the old version.
    pub old_path: PathBuf,
    /// Path in the new version.
    pub new_path: PathBuf,
    /// Parsed hunks for this file.
    pub hunks: Vec<DiffHunk>,
    /// Whether this is a newly created file.
    pub is_new_file: bool,
    /// Whether this file was deleted.
    pub is_deleted_file: bool,
    /// Whether this file was renamed.
    pub is_rename: bool,
}

impl fmt::Display for FileDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({} hunks)",
            self.new_path.display(),
            self.hunks.len()
        )
    }
}

/// Parse a unified diff string (as produced by `git diff`) into structured [`FileDiff`] entries.
///
/// Handles standard unified diff format including new files, deleted files,
/// renamed files, and binary files (which are skipped).
///
/// # Errors
///
/// Returns [`ArgusError::Parse`] if a hunk header is malformed.
///
/// # Examples
///
/// ```
/// use argus_difflens::parser::parse_unified_diff;
///
/// let files = parse_unified_diff("").unwrap();
/// assert!(files.is_empty());
/// ```
pub fn parse_unified_diff(input: &str) -> Result<Vec<FileDiff>, ArgusError> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut current: Option<FileDiff> = None;
    let mut current_hunk: Option<DiffHunk> = None;
    let mut is_binary = false;

    for line in input.lines() {
        if line.starts_with("diff --git ") {
            flush_hunk(&mut current, &mut current_hunk);
            if let Some(file) = current.take() {
                if !is_binary {
                    files.push(file);
                }
            }
            is_binary = false;
            current = Some(FileDiff {
                old_path: PathBuf::new(),
                new_path: PathBuf::new(),
                hunks: Vec::new(),
                is_new_file: false,
                is_deleted_file: false,
                is_rename: false,
            });
            continue;
        }

        // Implicitly start a file if we see a header but have no current file
        // This handles standard patches that lack the "diff --git" command line
        if line.starts_with("--- ") && current.is_none() {
            current = Some(FileDiff {
                old_path: PathBuf::new(),
                new_path: PathBuf::new(),
                hunks: Vec::new(),
                is_new_file: false,
                is_deleted_file: false,
                is_rename: false,
            });
        }

        let Some(file) = current.as_mut() else {
            continue;
        };

        if line.starts_with("Binary files ") && line.ends_with(" differ") {
            is_binary = true;
            continue;
        }

        if line.starts_with("new file mode") {
            file.is_new_file = true;
            continue;
        }

        if line.starts_with("deleted file mode") {
            file.is_deleted_file = true;
            continue;
        }

        if line.starts_with("rename from ") || line.starts_with("rename to ") {
            file.is_rename = true;
            continue;
        }

        if line.starts_with("index ") || line.starts_with("similarity index") {
            continue;
        }

        if let Some(path) = line.strip_prefix("--- ") {
            file.old_path = parse_path(path);
            continue;
        }

        if let Some(path) = line.strip_prefix("+++ ") {
            file.new_path = parse_path(path);
            if path == "/dev/null" {
                file.is_deleted_file = true;
            }
            continue;
        }

        if line.starts_with("@@ ") {
            flush_hunk(&mut current, &mut current_hunk);
            // Re-borrow after flush
            let file = current.as_ref().unwrap();
            let file_path = if file.is_deleted_file {
                file.old_path.clone()
            } else {
                file.new_path.clone()
            };
            let (old_start, old_lines, new_start, new_lines) = parse_hunk_header(line)?;
            let change_type = if file.is_new_file || old_lines == 0 {
                ChangeType::Add
            } else if file.is_deleted_file || new_lines == 0 {
                ChangeType::Delete
            } else {
                ChangeType::Modify
            };
            current_hunk = Some(DiffHunk {
                file_path,
                old_start,
                old_lines,
                new_start,
                new_lines,
                content: String::new(),
                change_type,
            });
            continue;
        }

        if line == "\\ No newline at end of file" {
            continue;
        }

        if let Some(hunk) = current_hunk.as_mut() {
            if line.starts_with('+') || line.starts_with('-') || line.starts_with(' ') {
                hunk.content.push_str(line);
                hunk.content.push('\n');
            }
        }
    }

    flush_hunk(&mut current, &mut current_hunk);
    if let Some(file) = current.take() {
        if !is_binary {
            files.push(file);
        }
    }

    Ok(files)
}

fn flush_hunk(current: &mut Option<FileDiff>, hunk: &mut Option<DiffHunk>) {
    if let Some(h) = hunk.take() {
        if let Some(file) = current.as_mut() {
            file.hunks.push(h);
        }
    }
}

fn parse_path(raw: &str) -> PathBuf {
    let normalized = raw.trim_matches('"');

    if normalized == "/dev/null" {
        return PathBuf::from("/dev/null");
    }

    let stripped = normalized
        .strip_prefix("a/")
        .or_else(|| normalized.strip_prefix("b/"))
        .unwrap_or(normalized);

    PathBuf::from(stripped)
}

fn parse_hunk_header(line: &str) -> Result<(u32, u32, u32, u32), ArgusError> {
    let inner = line
        .strip_prefix("@@ ")
        .and_then(|s| {
            let end = s.find(" @@")?;
            Some(&s[..end])
        })
        .ok_or_else(|| ArgusError::Parse(format!("invalid hunk header: {line}")))?;

    let parts: Vec<&str> = inner.split(' ').collect();
    if parts.len() != 2 {
        return Err(ArgusError::Parse(format!("invalid hunk header: {line}")));
    }

    let old = parts[0]
        .strip_prefix('-')
        .ok_or_else(|| ArgusError::Parse(format!("invalid old range in hunk: {line}")))?;
    let new = parts[1]
        .strip_prefix('+')
        .ok_or_else(|| ArgusError::Parse(format!("invalid new range in hunk: {line}")))?;

    let (old_start, old_lines) = parse_range(old, line)?;
    let (new_start, new_lines) = parse_range(new, line)?;

    Ok((old_start, old_lines, new_start, new_lines))
}

fn parse_range(range: &str, context: &str) -> Result<(u32, u32), ArgusError> {
    if let Some((start, count)) = range.split_once(',') {
        let s = start
            .parse()
            .map_err(|_| ArgusError::Parse(format!("invalid range number in: {context}")))?;
        let c = count
            .parse()
            .map_err(|_| ArgusError::Parse(format!("invalid range count in: {context}")))?;
        Ok((s, c))
    } else {
        let s = range
            .parse()
            .map_err(|_| ArgusError::Parse(format!("invalid range number in: {context}")))?;
        Ok((s, 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_diff_returns_empty_vec() {
        let files = parse_unified_diff("").unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn single_file_single_hunk() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
index abc1234..def5678 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!(\"hello\");
     let x = 1;
 }
";
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].new_path, PathBuf::from("src/main.rs"));
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[0].hunks[0].old_start, 1);
        assert_eq!(files[0].hunks[0].old_lines, 3);
        assert_eq!(files[0].hunks[0].new_start, 1);
        assert_eq!(files[0].hunks[0].new_lines, 4);
        assert_eq!(files[0].hunks[0].change_type, ChangeType::Modify);
        assert!(files[0].hunks[0].content.contains("+    println!"));
    }

    #[test]
    fn single_file_multiple_hunks() {
        let diff = "\
diff --git a/lib.rs b/lib.rs
--- a/lib.rs
+++ b/lib.rs
@@ -1,3 +1,4 @@
 fn foo() {
+    bar();
 }
@@ -10,3 +11,4 @@
 fn baz() {
+    qux();
 }
";
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].hunks.len(), 2);
        assert_eq!(files[0].hunks[0].old_start, 1);
        assert_eq!(files[0].hunks[1].old_start, 10);
    }

    #[test]
    fn multiple_files() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1 +1,2 @@
 line1
+line2
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -1 +1,2 @@
 line1
+line2
";
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].new_path, PathBuf::from("a.rs"));
        assert_eq!(files[1].new_path, PathBuf::from("b.rs"));
    }

    #[test]
    fn new_file() {
        let diff = "\
diff --git a/new.rs b/new.rs
new file mode 100644
--- /dev/null
+++ b/new.rs
@@ -0,0 +1,3 @@
+fn hello() {
+    println!(\"new\");
+}
";
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].is_new_file);
        assert_eq!(files[0].old_path, PathBuf::from("/dev/null"));
        assert_eq!(files[0].new_path, PathBuf::from("new.rs"));
        assert_eq!(files[0].hunks[0].change_type, ChangeType::Add);
    }

    #[test]
    fn deleted_file() {
        let diff = "\
diff --git a/old.rs b/old.rs
deleted file mode 100644
--- a/old.rs
+++ /dev/null
@@ -1,3 +0,0 @@
-fn goodbye() {
-    println!(\"old\");
-}
";
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].is_deleted_file);
        assert_eq!(files[0].new_path, PathBuf::from("/dev/null"));
        assert_eq!(files[0].hunks[0].change_type, ChangeType::Delete);
    }

    #[test]
    fn renamed_file() {
        let diff = "\
diff --git a/old_name.rs b/new_name.rs
similarity index 100%
rename from old_name.rs
rename to new_name.rs
";
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].is_rename);
    }

    #[test]
    fn hunk_only_additions() {
        let diff = "\
diff --git a/add.rs b/add.rs
--- a/add.rs
+++ b/add.rs
@@ -5,0 +6,3 @@
+line1
+line2
+line3
";
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files[0].hunks[0].change_type, ChangeType::Add);
        assert_eq!(files[0].hunks[0].old_lines, 0);
        assert_eq!(files[0].hunks[0].new_lines, 3);
    }

    #[test]
    fn hunk_only_deletions() {
        let diff = "\
diff --git a/del.rs b/del.rs
--- a/del.rs
+++ b/del.rs
@@ -1,3 +0,0 @@
-line1
-line2
-line3
";
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files[0].hunks[0].change_type, ChangeType::Delete);
        assert_eq!(files[0].hunks[0].new_lines, 0);
    }

    #[test]
    fn binary_files_skipped() {
        let diff = "\
diff --git a/image.png b/image.png
Binary files a/image.png and b/image.png differ
diff --git a/code.rs b/code.rs
--- a/code.rs
+++ b/code.rs
@@ -1 +1,2 @@
 line1
+line2
";
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].new_path, PathBuf::from("code.rs"));
    }

    #[test]
    fn no_newline_at_eof_handled() {
        let diff = "\
diff --git a/f.rs b/f.rs
--- a/f.rs
+++ b/f.rs
@@ -1 +1 @@
-old
\\ No newline at end of file
+new
\\ No newline at end of file
";
        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 1);
        let content = &files[0].hunks[0].content;
        assert!(!content.contains("No newline"));
        assert!(content.contains("-old"));
        assert!(content.contains("+new"));
    }


    #[test]
    fn parse_path_handles_quoted_paths() {
        assert_eq!(parse_path("\"a/src/my file.rs\""), PathBuf::from("src/my file.rs"));
        assert_eq!(parse_path("\"b/src/my file.rs\""), PathBuf::from("src/my file.rs"));
    }

    #[test]
    fn quoted_paths_are_parsed_in_unified_diff() {
        let diff = r#"--- "a/src/my file.rs"
+++ "b/src/my file.rs"
@@ -1 +1,2 @@
 old
+new
"#;

        let files = parse_unified_diff(diff).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].old_path, PathBuf::from("src/my file.rs"));
        assert_eq!(files[0].new_path, PathBuf::from("src/my file.rs"));
        assert_eq!(files[0].hunks[0].file_path, PathBuf::from("src/my file.rs"));
    }

    #[test]
    fn real_world_fixture() {
        let diff = include_str!("../tests/fixtures/simple.diff");
        let files = parse_unified_diff(diff).unwrap();
        assert!(!files.is_empty());
        for file in &files {
            assert!(!file.hunks.is_empty() || file.is_rename);
        }
    }
}
