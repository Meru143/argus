//! Churn × complexity hotspot detection.
//!
//! Identifies files with high churn and high complexity (LoC) that are
//! likely sources of bugs, following the Tornhill methodology.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use argus_core::ArgusError;
use serde::{Deserialize, Serialize};

use crate::mining::CommitInfo;

/// A hotspot — a file with high churn and high complexity.
///
/// # Examples
///
/// ```
/// use argus_gitpulse::hotspots::Hotspot;
///
/// let h = Hotspot {
///     path: "src/main.rs".into(),
///     revisions: 10,
///     total_churn: 500,
///     relative_churn: 2.5,
///     current_loc: 200,
///     score: 0.85,
///     last_modified: 1700000000,
///     authors: 3,
/// };
/// assert!(h.score > 0.0 && h.score <= 1.0);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hotspot {
    /// File path relative to repo root.
    pub path: String,
    /// Number of commits touching this file.
    pub revisions: u32,
    /// Total lines added + deleted across all commits.
    pub total_churn: u64,
    /// `total_churn / current_loc`.
    pub relative_churn: f64,
    /// Current lines of code in the file.
    pub current_loc: u64,
    /// Normalized hotspot score (0.0–1.0).
    pub score: f64,
    /// Unix timestamp of most recent change.
    pub last_modified: i64,
    /// Number of distinct authors.
    pub authors: u32,
}

/// Detect hotspots from commit history.
///
/// Returns hotspots sorted by score descending. Only includes files
/// that still exist on disk at `repo_path`.
///
/// # Errors
///
/// Returns [`ArgusError::Git`] on filesystem errors.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use argus_gitpulse::hotspots::detect_hotspots;
/// use argus_gitpulse::mining::{mine_history, MiningOptions};
///
/// let commits = mine_history(Path::new("."), &MiningOptions::default()).unwrap();
/// let hotspots = detect_hotspots(Path::new("."), &commits).unwrap();
/// for h in hotspots.iter().take(5) {
///     println!("{}: score={:.2}, revisions={}", h.path, h.score, h.revisions);
/// }
/// ```
pub fn detect_hotspots(
    repo_path: &Path,
    commits: &[CommitInfo],
) -> Result<Vec<Hotspot>, ArgusError> {
    if commits.is_empty() {
        return Ok(Vec::new());
    }

    // Accumulate per-file stats
    let mut revisions: HashMap<String, u32> = HashMap::new();
    let mut churn: HashMap<String, u64> = HashMap::new();
    let mut authors: HashMap<String, HashSet<String>> = HashMap::new();
    let mut last_modified: HashMap<String, i64> = HashMap::new();

    for commit in commits {
        for file in &commit.files_changed {
            *revisions.entry(file.path.clone()).or_default() += 1;
            *churn.entry(file.path.clone()).or_default() += file.lines_added + file.lines_deleted;
            authors
                .entry(file.path.clone())
                .or_default()
                .insert(commit.author.clone());
            let entry = last_modified.entry(file.path.clone()).or_insert(0);
            if commit.timestamp > *entry {
                *entry = commit.timestamp;
            }
        }
    }

    // Build hotspots, only for files that exist on disk
    let mut hotspots = Vec::new();
    for (path, rev_count) in &revisions {
        let full_path = repo_path.join(path);
        let Some(loc) = count_lines(&full_path) else {
            continue;
        };

        let total_churn = churn.get(path).copied().unwrap_or(0);
        let relative_churn = if loc > 0 {
            total_churn as f64 / loc as f64
        } else {
            0.0
        };
        let author_count = authors.get(path).map_or(0, |s| s.len() as u32);
        let last_mod = last_modified.get(path).copied().unwrap_or(0);

        hotspots.push(Hotspot {
            path: path.clone(),
            revisions: *rev_count,
            total_churn,
            relative_churn,
            current_loc: loc,
            score: 0.0, // computed below
            last_modified: last_mod,
            authors: author_count,
        });
    }

    if hotspots.is_empty() {
        return Ok(hotspots);
    }

    // Normalize and compute scores
    let max_revisions = hotspots.iter().map(|h| h.revisions).max().unwrap_or(1) as f64;
    let max_relative_churn = hotspots
        .iter()
        .map(|h| h.relative_churn)
        .fold(0.0f64, f64::max)
        .max(1.0);
    let max_loc = hotspots.iter().map(|h| h.current_loc).max().unwrap_or(1) as f64;

    for hotspot in &mut hotspots {
        let norm_revisions = hotspot.revisions as f64 / max_revisions;
        let norm_churn = hotspot.relative_churn / max_relative_churn;
        let norm_loc = hotspot.current_loc as f64 / max_loc;

        hotspot.score = norm_revisions * 0.5 + norm_churn * 0.3 + norm_loc * 0.2;
    }

    hotspots.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(hotspots)
}

fn count_lines(path: &Path) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    Some(content.lines().count() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mining::{ChangeStatus, FileChange};
    use std::path::PathBuf;

    fn make_commit(author: &str, timestamp: i64, files: Vec<(&str, u64, u64)>) -> CommitInfo {
        CommitInfo {
            hash: format!("hash_{timestamp}"),
            author: author.into(),
            email: format!("{author}@example.com"),
            timestamp,
            message: "test commit".into(),
            files_changed: files
                .into_iter()
                .map(|(path, added, deleted)| FileChange {
                    path: path.into(),
                    lines_added: added,
                    lines_deleted: deleted,
                    status: ChangeStatus::Modified,
                })
                .collect(),
        }
    }

    #[test]
    fn high_churn_file_gets_high_score() {
        // Use real files in the repo
        let repo_path = find_repo_root().unwrap();
        let commits = vec![
            make_commit("alice", 1000, vec![("src/main.rs", 100, 50)]),
            make_commit("alice", 2000, vec![("src/main.rs", 80, 40)]),
            make_commit("alice", 3000, vec![("src/main.rs", 60, 30)]),
            make_commit("bob", 4000, vec![("Cargo.toml", 1, 0)]),
        ];

        let hotspots = detect_hotspots(&repo_path, &commits).unwrap();
        assert!(!hotspots.is_empty());

        // main.rs should score higher than Cargo.toml
        let main_spot = hotspots.iter().find(|h| h.path == "src/main.rs");
        let cargo_spot = hotspots.iter().find(|h| h.path == "Cargo.toml");

        if let (Some(main_h), Some(cargo_h)) = (main_spot, cargo_spot) {
            assert!(
                main_h.score > cargo_h.score,
                "main.rs ({:.4}) should score higher than Cargo.toml ({:.4})",
                main_h.score,
                cargo_h.score,
            );
        }
    }

    #[test]
    fn deleted_files_are_excluded() {
        let repo_path = find_repo_root().unwrap();
        let commits = vec![make_commit(
            "alice",
            1000,
            vec![("nonexistent_file_xyz.rs", 100, 50)],
        )];

        let hotspots = detect_hotspots(&repo_path, &commits).unwrap();
        let found = hotspots.iter().any(|h| h.path == "nonexistent_file_xyz.rs");
        assert!(!found, "nonexistent files should be excluded");
    }

    #[test]
    fn scores_are_in_valid_range() {
        let repo_path = find_repo_root().unwrap();
        let commits = vec![
            make_commit("alice", 1000, vec![("src/main.rs", 50, 20)]),
            make_commit("bob", 2000, vec![("Cargo.toml", 5, 2)]),
        ];

        let hotspots = detect_hotspots(&repo_path, &commits).unwrap();
        for h in &hotspots {
            assert!(
                h.score >= 0.0 && h.score <= 1.0,
                "score {} is out of range for {}",
                h.score,
                h.path,
            );
        }
    }

    #[test]
    fn empty_commits_dont_crash() {
        let repo_path = find_repo_root().unwrap();
        let hotspots = detect_hotspots(&repo_path, &[]).unwrap();
        assert!(hotspots.is_empty());
    }

    fn find_repo_root() -> Option<PathBuf> {
        let mut path = std::env::current_dir().ok()?;
        loop {
            if path.join(".git").exists() {
                return Some(path);
            }
            if !path.pop() {
                return None;
            }
        }
    }
}
