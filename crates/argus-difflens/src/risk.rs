use std::fmt;
use std::path::Path;

use argus_core::{ChangeType, RiskScore};
use serde::{Deserialize, Serialize};

use crate::parser::FileDiff;

/// Complete risk analysis for a set of diffs.
///
/// # Examples
///
/// ```
/// use argus_difflens::parser::parse_unified_diff;
/// use argus_difflens::risk::compute_risk;
///
/// let diff = "diff --git a/f.rs b/f.rs\n\
///             --- a/f.rs\n\
///             +++ b/f.rs\n\
///             @@ -1,2 +1,3 @@\n\
///              line\n\
///             +new\n";
/// let files = parse_unified_diff(diff).unwrap();
/// let report = compute_risk(&files);
/// assert!(report.overall.total >= 0.0);
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskReport {
    /// Aggregate risk score across all files.
    pub overall: RiskScore,
    /// Per-file risk breakdown.
    pub per_file: Vec<FileRisk>,
    /// High-level summary statistics.
    pub summary: RiskSummary,
}

/// Risk details for a single file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRisk {
    /// File path.
    pub path: std::path::PathBuf,
    /// Computed risk score.
    pub score: RiskScore,
    /// Lines added in this file.
    pub lines_added: u32,
    /// Lines deleted in this file.
    pub lines_deleted: u32,
    /// Number of hunks in this file.
    pub hunk_count: usize,
    /// Overall change classification.
    pub change_type: ChangeType,
}

/// Summary statistics for a diff.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskSummary {
    /// Number of files changed.
    pub total_files: usize,
    /// Total lines added across all files.
    pub total_additions: u32,
    /// Total lines deleted across all files.
    pub total_deletions: u32,
    /// Overall risk classification.
    pub risk_level: RiskLevel,
}

/// Categorical risk classification based on score ranges.
///
/// # Examples
///
/// ```
/// use argus_difflens::risk::RiskLevel;
///
/// let level = RiskLevel::from_score(30.0);
/// assert!(matches!(level, RiskLevel::Medium));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// Score 0–25.
    Low,
    /// Score 26–50.
    Medium,
    /// Score 51–75.
    High,
    /// Score 76–100.
    Critical,
}

impl RiskLevel {
    /// Map a numeric score to a risk level.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_difflens::risk::RiskLevel;
    ///
    /// assert_eq!(RiskLevel::from_score(10.0), RiskLevel::Low);
    /// assert_eq!(RiskLevel::from_score(50.0), RiskLevel::Medium);
    /// assert_eq!(RiskLevel::from_score(60.0), RiskLevel::High);
    /// assert_eq!(RiskLevel::from_score(90.0), RiskLevel::Critical);
    /// ```
    pub fn from_score(score: f64) -> Self {
        if score <= 25.0 {
            RiskLevel::Low
        } else if score <= 50.0 {
            RiskLevel::Medium
        } else if score <= 75.0 {
            RiskLevel::High
        } else {
            RiskLevel::Critical
        }
    }
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "Low"),
            RiskLevel::Medium => write!(f, "Medium"),
            RiskLevel::High => write!(f, "High"),
            RiskLevel::Critical => write!(f, "Critical"),
        }
    }
}

/// Compute a risk report from parsed file diffs.
///
/// Uses Phase 1 simplified scoring: size-based and file-type heuristics only.
/// Complexity and coverage are set to 0 (requires tree-sitter / coverage data).
///
/// # Examples
///
/// ```
/// use argus_difflens::risk::compute_risk;
///
/// let report = compute_risk(&[]);
/// assert_eq!(report.summary.total_files, 0);
/// assert_eq!(report.overall.total, 0.0);
/// ```
pub fn compute_risk(diffs: &[FileDiff]) -> RiskReport {
    if diffs.is_empty() {
        return RiskReport {
            overall: RiskScore::new(0.0, 0.0, 0.0, 0.0, 0.0),
            per_file: Vec::new(),
            summary: RiskSummary {
                total_files: 0,
                total_additions: 0,
                total_deletions: 0,
                risk_level: RiskLevel::Low,
            },
        };
    }

    let mut per_file = Vec::with_capacity(diffs.len());
    let mut total_additions: u32 = 0;
    let mut total_deletions: u32 = 0;
    let mut max_file_type_score: f64 = 0.0;

    for diff in diffs {
        let (added, deleted) = count_lines(diff);
        total_additions += added;
        total_deletions += deleted;

        let lines_changed = (added + deleted) as f64;
        let size = (lines_changed * 2.0).min(100.0);
        let diffusion = (diff.hunks.len() as f64 * 20.0).min(100.0);
        let file_type_score = file_type_risk(&diff.new_path);
        if file_type_score > max_file_type_score {
            max_file_type_score = file_type_score;
        }

        let change_type = dominant_change_type(diff);

        per_file.push(FileRisk {
            path: diff.new_path.clone(),
            score: RiskScore::new(size, 0.0, diffusion, 0.0, file_type_score),
            lines_added: added,
            lines_deleted: deleted,
            hunk_count: diff.hunks.len(),
            change_type,
        });
    }

    let total_lines = (total_additions + total_deletions) as f64;
    let overall_size = (total_lines * 2.0).min(100.0);
    let overall_diffusion = (diffs.len() as f64 * 20.0).min(100.0);
    let overall = RiskScore::new(
        overall_size,
        0.0,
        overall_diffusion,
        0.0,
        max_file_type_score,
    );

    let summary = RiskSummary {
        total_files: diffs.len(),
        total_additions,
        total_deletions,
        risk_level: RiskLevel::from_score(overall.total),
    };

    RiskReport {
        overall,
        per_file,
        summary,
    }
}

fn count_lines(diff: &FileDiff) -> (u32, u32) {
    let mut added: u32 = 0;
    let mut deleted: u32 = 0;
    for hunk in &diff.hunks {
        for line in hunk.content.lines() {
            if line.starts_with('+') {
                added += 1;
            } else if line.starts_with('-') {
                deleted += 1;
            }
        }
    }
    (added, deleted)
}

fn dominant_change_type(diff: &FileDiff) -> ChangeType {
    if diff.is_new_file {
        return ChangeType::Add;
    }
    if diff.is_deleted_file {
        return ChangeType::Delete;
    }
    if diff.is_rename {
        return ChangeType::Move;
    }
    ChangeType::Modify
}

fn file_type_risk(path: &Path) -> f64 {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go" | "java" | "c" | "cpp" | "h" => 50.0,
        "toml" | "yaml" | "yml" | "json" => 20.0,
        "md" | "txt" | "rst" => 5.0,
        _ => 30.0,
    }
}

impl fmt::Display for RiskReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Risk Report")?;
        writeln!(f, "===========")?;
        writeln!(
            f,
            "Overall Risk: {:.1}/100 ({})\n",
            self.overall.total, self.summary.risk_level
        )?;

        if !self.per_file.is_empty() {
            writeln!(
                f,
                "{:<40} {:>8} {:>10} {:>8}",
                "File", "Change", "+/-", "Risk"
            )?;
            writeln!(f, "{}", "-".repeat(70))?;
            for fr in &self.per_file {
                writeln!(
                    f,
                    "{:<40} {:>8} {:>+4}/{:<-4}  {:>5.1}",
                    fr.path.display(),
                    fr.change_type,
                    fr.lines_added,
                    fr.lines_deleted,
                    fr.score.total,
                )?;
            }
        }

        writeln!(
            f,
            "\nSummary: {} files, +{} additions, -{} deletions",
            self.summary.total_files, self.summary.total_additions, self.summary.total_deletions
        )
    }
}

impl RiskReport {
    /// Render the report as a markdown string.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_difflens::risk::compute_risk;
    ///
    /// let report = compute_risk(&[]);
    /// let md = report.to_markdown();
    /// assert!(md.contains("# Risk Report"));
    /// ```
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Risk Report\n\n");
        out.push_str(&format!(
            "**Overall Risk:** {:.1}/100 ({})\n\n",
            self.overall.total, self.summary.risk_level
        ));

        if !self.per_file.is_empty() {
            out.push_str("| File | Change | +/- | Risk |\n");
            out.push_str("|------|--------|-----|------|\n");
            for fr in &self.per_file {
                out.push_str(&format!(
                    "| {} | {} | +{}/-{} | {:.1} |\n",
                    fr.path.display(),
                    fr.change_type,
                    fr.lines_added,
                    fr.lines_deleted,
                    fr.score.total,
                ));
            }
            out.push('\n');
        }

        out.push_str(&format!(
            "**Summary:** {} files, +{} additions, -{} deletions\n",
            self.summary.total_files, self.summary.total_additions, self.summary.total_deletions
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_unified_diff;

    #[test]
    fn empty_diff_risk() {
        let report = compute_risk(&[]);
        assert_eq!(report.summary.total_files, 0);
        assert_eq!(report.overall.total, 0.0);
        assert_eq!(report.summary.risk_level, RiskLevel::Low);
    }

    #[test]
    fn single_file_risk() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,6 @@
 fn main() {
+    let a = 1;
+    let b = 2;
+    let c = 3;
 }
";
        let files = parse_unified_diff(diff).unwrap();
        let report = compute_risk(&files);
        assert_eq!(report.summary.total_files, 1);
        assert_eq!(report.summary.total_additions, 3);
        assert_eq!(report.summary.total_deletions, 0);
        assert!(report.overall.total > 0.0);
        assert_eq!(report.per_file[0].change_type, ChangeType::Modify);
    }

    #[test]
    fn multi_file_increases_diffusion() {
        let diff = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1 +1,2 @@
 a
+b
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -1 +1,2 @@
 a
+b
diff --git a/c.rs b/c.rs
--- a/c.rs
+++ b/c.rs
@@ -1 +1,2 @@
 a
+b
";
        let files = parse_unified_diff(diff).unwrap();
        let report = compute_risk(&files);
        assert_eq!(report.summary.total_files, 3);
        // 3 files * 20 = 60 diffusion
        assert!((report.overall.diffusion - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn file_type_scoring() {
        assert_eq!(file_type_risk(Path::new("main.rs")), 50.0);
        assert_eq!(file_type_risk(Path::new("config.toml")), 20.0);
        assert_eq!(file_type_risk(Path::new("README.md")), 5.0);
        assert_eq!(file_type_risk(Path::new("data.csv")), 30.0);
        assert_eq!(file_type_risk(Path::new("app.py")), 50.0);
        assert_eq!(file_type_risk(Path::new("index.ts")), 50.0);
    }

    #[test]
    fn risk_level_boundaries() {
        assert_eq!(RiskLevel::from_score(0.0), RiskLevel::Low);
        assert_eq!(RiskLevel::from_score(25.0), RiskLevel::Low);
        assert_eq!(RiskLevel::from_score(25.1), RiskLevel::Medium);
        assert_eq!(RiskLevel::from_score(50.0), RiskLevel::Medium);
        assert_eq!(RiskLevel::from_score(50.1), RiskLevel::High);
        assert_eq!(RiskLevel::from_score(75.0), RiskLevel::High);
        assert_eq!(RiskLevel::from_score(75.1), RiskLevel::Critical);
        assert_eq!(RiskLevel::from_score(100.0), RiskLevel::Critical);
    }

    #[test]
    fn display_and_markdown_output() {
        let diff = "\
diff --git a/f.rs b/f.rs
--- a/f.rs
+++ b/f.rs
@@ -1 +1,2 @@
 x
+y
";
        let files = parse_unified_diff(diff).unwrap();
        let report = compute_risk(&files);
        let text = format!("{report}");
        assert!(text.contains("Risk Report"));
        assert!(text.contains("f.rs"));

        let md = report.to_markdown();
        assert!(md.contains("# Risk Report"));
        assert!(md.contains("f.rs"));
    }
}
