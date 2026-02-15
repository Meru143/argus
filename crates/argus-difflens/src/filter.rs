//! Pre-LLM file filtering to eliminate noise at the source.
//!
//! Filters out lock files, generated code, vendored dependencies,
//! minified files, and files matching custom patterns before they
//! reach the LLM, saving tokens and reducing false positives.

use std::path::{Path, PathBuf};

use argus_core::ReviewConfig;

use crate::parser::FileDiff;

/// Files and patterns to skip before sending to LLM.
///
/// # Examples
///
/// ```
/// use argus_difflens::filter::DiffFilter;
///
/// let filter = DiffFilter::default_filter();
/// assert!(filter.should_skip("package-lock.json"));
/// assert!(!filter.should_skip("src/main.rs"));
/// ```
pub struct DiffFilter {
    skip_patterns: Vec<glob::Pattern>,
    skip_extensions: Vec<String>,
    max_file_size_lines: usize,
}

impl DiffFilter {
    /// Create a filter with sensible defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_difflens::filter::DiffFilter;
    ///
    /// let filter = DiffFilter::default_filter();
    /// assert!(filter.should_skip("yarn.lock"));
    /// ```
    pub fn default_filter() -> Self {
        Self {
            skip_patterns: Vec::new(),
            skip_extensions: Vec::new(),
            max_file_size_lines: 1000,
        }
    }

    /// Create a filter from review configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_core::ReviewConfig;
    /// use argus_difflens::filter::DiffFilter;
    ///
    /// let config = ReviewConfig::default();
    /// let filter = DiffFilter::from_config(&config);
    /// assert!(filter.should_skip("Cargo.lock"));
    /// ```
    pub fn from_config(config: &ReviewConfig) -> Self {
        let mut skip_patterns = Vec::new();
        for pat in &config.skip_patterns {
            if let Ok(p) = glob::Pattern::new(pat) {
                skip_patterns.push(p);
            }
        }

        Self {
            skip_patterns,
            skip_extensions: config.skip_extensions.clone(),
            max_file_size_lines: 1000,
        }
    }

    /// Check if a single file path should be skipped.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_difflens::filter::DiffFilter;
    ///
    /// let filter = DiffFilter::default_filter();
    /// assert!(filter.should_skip("vendor/lib.js"));
    /// assert!(!filter.should_skip("src/lib.rs"));
    /// ```
    pub fn should_skip(&self, path: &str) -> bool {
        self.check_skip(Path::new(path), "", 0).is_some()
    }

    /// Filter a list of `FileDiff`s, returning only reviewable ones.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_difflens::filter::DiffFilter;
    /// use argus_difflens::parser::parse_unified_diff;
    ///
    /// let diff = "diff --git a/src/main.rs b/src/main.rs\n\
    ///             --- a/src/main.rs\n\
    ///             +++ b/src/main.rs\n\
    ///             @@ -1,2 +1,3 @@\n\
    ///              line\n\
    ///             +new\n";
    /// let diffs = parse_unified_diff(diff).unwrap();
    /// let filter = DiffFilter::default_filter();
    /// let result = filter.filter(diffs);
    /// assert_eq!(result.kept.len(), 1);
    /// assert!(result.skipped.is_empty());
    /// ```
    pub fn filter(&self, diffs: Vec<FileDiff>) -> FilterResult {
        let mut kept = Vec::new();
        let mut skipped = Vec::new();

        for diff in diffs {
            let path = &diff.new_path;
            let path_str = path.to_string_lossy();

            let content = Self::collect_hunk_content(&diff);
            let changed_lines = Self::count_changed_lines(&diff);

            if let Some(reason) = self.check_skip(path, &content, changed_lines) {
                skipped.push(SkippedFile {
                    path: path.clone(),
                    reason,
                });
            } else {
                // Check custom pattern matches
                let mut matched = false;
                for pat in &self.skip_patterns {
                    if pat.matches(&path_str) {
                        skipped.push(SkippedFile {
                            path: path.clone(),
                            reason: SkipReason::PatternMatch(pat.to_string()),
                        });
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    kept.push(diff);
                }
            }
        }

        FilterResult { kept, skipped }
    }

    fn check_skip(&self, path: &Path, content: &str, changed_lines: usize) -> Option<SkipReason> {
        let path_str = path.to_string_lossy();
        let file_name = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default();

        // Lock files
        if is_lock_file(&file_name) {
            return Some(SkipReason::LockFile);
        }

        // Vendored code
        if is_vendored(&path_str) {
            return Some(SkipReason::VendoredCode);
        }

        // Minified files
        if is_minified(&file_name, content) {
            return Some(SkipReason::MinifiedFile);
        }

        // Generated files (by name pattern)
        if is_generated_by_name(&file_name) {
            return Some(SkipReason::GeneratedFile);
        }

        // Generated files (by content header)
        if is_generated_by_content(content) {
            return Some(SkipReason::GeneratedFile);
        }

        // Custom extension skip
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            for skip_ext in &self.skip_extensions {
                if ext == skip_ext {
                    return Some(SkipReason::PatternMatch(format!("*.{skip_ext}")));
                }
            }
        }

        // Too large
        if changed_lines > self.max_file_size_lines {
            return Some(SkipReason::TooLarge);
        }

        None
    }

    fn collect_hunk_content(diff: &FileDiff) -> String {
        let mut content = String::new();
        for hunk in &diff.hunks {
            content.push_str(&hunk.content);
        }
        content
    }

    fn count_changed_lines(diff: &FileDiff) -> usize {
        let mut count = 0;
        for hunk in &diff.hunks {
            for line in hunk.content.lines() {
                if line.starts_with('+') || line.starts_with('-') {
                    count += 1;
                }
            }
        }
        count
    }
}

/// Result of filtering diffs.
///
/// # Examples
///
/// ```
/// use argus_difflens::filter::FilterResult;
///
/// let result = FilterResult {
///     kept: vec![],
///     skipped: vec![],
/// };
/// assert!(result.kept.is_empty());
/// ```
pub struct FilterResult {
    /// Diffs that passed the filter.
    pub kept: Vec<FileDiff>,
    /// Files that were skipped with reasons.
    pub skipped: Vec<SkippedFile>,
}

/// A file that was skipped during filtering.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use argus_difflens::filter::{SkippedFile, SkipReason};
///
/// let skipped = SkippedFile {
///     path: PathBuf::from("package-lock.json"),
///     reason: SkipReason::LockFile,
/// };
/// assert!(matches!(skipped.reason, SkipReason::LockFile));
/// ```
#[derive(Debug, Clone)]
pub struct SkippedFile {
    /// Path of the skipped file.
    pub path: PathBuf,
    /// Why the file was skipped.
    pub reason: SkipReason,
}

/// Reason a file was skipped.
///
/// # Examples
///
/// ```
/// use argus_difflens::filter::SkipReason;
///
/// let reason = SkipReason::LockFile;
/// assert_eq!(format!("{reason}"), "lock file");
/// ```
#[derive(Debug, Clone)]
pub enum SkipReason {
    /// Package manager lock file.
    LockFile,
    /// Auto-generated code.
    GeneratedFile,
    /// Third-party vendored code.
    VendoredCode,
    /// Minified or bundled file.
    MinifiedFile,
    /// Binary file.
    BinaryFile,
    /// File exceeds max changed lines threshold.
    TooLarge,
    /// Matched a custom skip pattern.
    PatternMatch(String),
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkipReason::LockFile => write!(f, "lock file"),
            SkipReason::GeneratedFile => write!(f, "generated file"),
            SkipReason::VendoredCode => write!(f, "vendored code"),
            SkipReason::MinifiedFile => write!(f, "minified file"),
            SkipReason::BinaryFile => write!(f, "binary file"),
            SkipReason::TooLarge => write!(f, "too large"),
            SkipReason::PatternMatch(pat) => write!(f, "pattern: {pat}"),
        }
    }
}

const LOCK_FILES: &[&str] = &[
    "package-lock.json",
    "yarn.lock",
    "Cargo.lock",
    "pnpm-lock.yaml",
    "poetry.lock",
    "Gemfile.lock",
    "composer.lock",
    "go.sum",
];

fn is_lock_file(file_name: &str) -> bool {
    LOCK_FILES.contains(&file_name)
}

fn is_vendored(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    for part in &parts {
        if *part == "vendor" || *part == "third_party" || *part == "node_modules" {
            return true;
        }
    }
    false
}

fn is_minified(file_name: &str, content: &str) -> bool {
    if file_name.ends_with(".min.js") || file_name.ends_with(".min.css") {
        return true;
    }
    // Heuristic: any line longer than 500 chars suggests minification
    for line in content.lines() {
        if line.len() > 500 {
            return true;
        }
    }
    false
}

fn is_generated_by_name(file_name: &str) -> bool {
    if file_name.contains(".generated.") {
        return true;
    }
    if file_name.ends_with(".g.dart") {
        return true;
    }
    if file_name.ends_with(".pb.go") || file_name.ends_with(".pb.rs") {
        return true;
    }
    false
}

fn is_generated_by_content(content: &str) -> bool {
    let mut line_count = 0;
    for line in content.lines() {
        // Only look at added lines for header detection
        let check_line = if let Some(stripped) = line.strip_prefix('+') {
            stripped
        } else if line.starts_with('-') || line.starts_with(' ') {
            // Also check context/removed lines (the file might have the header already)
            &line[1..]
        } else {
            line
        };

        if check_line.contains("// Code generated") || check_line.contains("# AUTO-GENERATED") {
            return true;
        }
        line_count += 1;
        if line_count >= 5 {
            break;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_unified_diff;

    fn make_diff(path: &str, content: &str) -> Vec<FileDiff> {
        let diff = format!(
            "diff --git a/{path} b/{path}\n\
             --- a/{path}\n\
             +++ b/{path}\n\
             @@ -1,1 +1,2 @@\n\
             {content}\n"
        );
        parse_unified_diff(&diff).unwrap()
    }

    #[test]
    fn lock_files_skipped() {
        let filter = DiffFilter::default_filter();
        for name in LOCK_FILES {
            let diffs = make_diff(name, "+new line");
            let result = filter.filter(diffs);
            assert!(result.kept.is_empty(), "expected {name} to be skipped");
            assert_eq!(result.skipped.len(), 1);
            assert!(matches!(result.skipped[0].reason, SkipReason::LockFile));
        }
    }

    #[test]
    fn generated_files_skipped_by_name() {
        let filter = DiffFilter::default_filter();

        for name in &[
            "api.generated.ts",
            "model.g.dart",
            "proto.pb.go",
            "msg.pb.rs",
        ] {
            let diffs = make_diff(name, "+new line");
            let result = filter.filter(diffs);
            assert!(result.kept.is_empty(), "expected {name} to be skipped");
            assert!(matches!(
                result.skipped[0].reason,
                SkipReason::GeneratedFile
            ));
        }
    }

    #[test]
    fn generated_files_skipped_by_header() {
        let filter = DiffFilter::default_filter();
        let diffs = make_diff("gen.go", "+// Code generated by protoc. DO NOT EDIT.");
        let result = filter.filter(diffs);
        assert!(result.kept.is_empty());
        assert!(matches!(
            result.skipped[0].reason,
            SkipReason::GeneratedFile
        ));
    }

    #[test]
    fn minified_files_skipped() {
        let filter = DiffFilter::default_filter();

        // By name
        let diffs = make_diff("app.min.js", "+var x=1;");
        let result = filter.filter(diffs);
        assert!(result.kept.is_empty());
        assert!(matches!(result.skipped[0].reason, SkipReason::MinifiedFile));

        // By long line heuristic
        let long_line = format!("+{}", "x".repeat(501));
        let diffs = make_diff("bundle.js", &long_line);
        let result = filter.filter(diffs);
        assert!(result.kept.is_empty());
        assert!(matches!(result.skipped[0].reason, SkipReason::MinifiedFile));
    }

    #[test]
    fn vendored_code_skipped() {
        let filter = DiffFilter::default_filter();

        for path in &[
            "vendor/lib.go",
            "third_party/dep.rs",
            "node_modules/pkg/index.js",
        ] {
            let diffs = make_diff(path, "+line");
            let result = filter.filter(diffs);
            assert!(result.kept.is_empty(), "expected {path} to be skipped");
            assert!(matches!(result.skipped[0].reason, SkipReason::VendoredCode));
        }
    }

    #[test]
    fn normal_source_files_kept() {
        let filter = DiffFilter::default_filter();
        let diffs = make_diff("src/main.rs", "+let x = 1;");
        let result = filter.filter(diffs);
        assert_eq!(result.kept.len(), 1);
        assert!(result.skipped.is_empty());
    }

    #[test]
    fn custom_patterns_from_config() {
        let config = ReviewConfig {
            skip_patterns: vec!["*.test.ts".into(), "fixtures/**".into()],
            ..ReviewConfig::default()
        };
        let filter = DiffFilter::from_config(&config);

        let diffs = make_diff("auth.test.ts", "+test line");
        let result = filter.filter(diffs);
        assert!(result.kept.is_empty());
        assert!(matches!(
            result.skipped[0].reason,
            SkipReason::PatternMatch(_)
        ));

        // Normal file still kept
        let diffs = make_diff("src/auth.ts", "+real code");
        let result = filter.filter(diffs);
        assert_eq!(result.kept.len(), 1);
    }

    #[test]
    fn custom_extensions_from_config() {
        let config = ReviewConfig {
            skip_extensions: vec!["snap".into()],
            ..ReviewConfig::default()
        };
        let filter = DiffFilter::from_config(&config);

        let diffs = make_diff("component.test.snap", "+snapshot content");
        let result = filter.filter(diffs);
        assert!(result.kept.is_empty());
    }

    #[test]
    fn empty_diff_returns_empty_result() {
        let filter = DiffFilter::default_filter();
        let result = filter.filter(Vec::new());
        assert!(result.kept.is_empty());
        assert!(result.skipped.is_empty());
    }

    #[test]
    fn too_large_files_skipped() {
        let filter = DiffFilter::default_filter();
        // Generate >1000 changed lines
        let mut lines = String::new();
        for i in 0..1002 {
            lines.push_str(&format!("+line {i}\n"));
        }
        let diff = format!(
            "diff --git a/big.rs b/big.rs\n\
             --- a/big.rs\n\
             +++ b/big.rs\n\
             @@ -1,1 +1,1003 @@\n\
             {lines}"
        );
        let diffs = parse_unified_diff(&diff).unwrap();
        let result = filter.filter(diffs);
        assert!(result.kept.is_empty());
        assert!(matches!(result.skipped[0].reason, SkipReason::TooLarge));
    }
}
