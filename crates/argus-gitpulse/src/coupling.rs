//! Temporal coupling detection.
//!
//! Identifies pairs of files that frequently change together in commits,
//! which may indicate hidden dependencies or architectural coupling.

use std::collections::HashMap;

use argus_core::ArgusError;
use serde::{Deserialize, Serialize};

use crate::mining::CommitInfo;

/// A pair of files that frequently change together.
///
/// # Examples
///
/// ```
/// use argus_gitpulse::coupling::CoupledPair;
///
/// let pair = CoupledPair {
///     file_a: "src/auth.rs".into(),
///     file_b: "src/session.rs".into(),
///     co_changes: 15,
///     coupling_degree: 0.75,
///     changes_a: 20,
///     changes_b: 18,
/// };
/// assert!(pair.coupling_degree > 0.5);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoupledPair {
    /// First file in the pair (lexicographically smaller).
    pub file_a: String,
    /// Second file in the pair.
    pub file_b: String,
    /// Number of commits touching both files.
    pub co_changes: u32,
    /// `co_changes / max(changes_a, changes_b)`.
    pub coupling_degree: f64,
    /// Total commits touching file_a.
    pub changes_a: u32,
    /// Total commits touching file_b.
    pub changes_b: u32,
}

/// Detect temporal coupling between files.
///
/// Returns coupled pairs sorted by `coupling_degree` descending.
/// Only returns pairs where `coupling_degree >= min_coupling`
/// and `co_changes >= min_co_changes`.
///
/// # Errors
///
/// Returns [`ArgusError`] on processing failure.
///
/// # Examples
///
/// ```
/// use argus_gitpulse::coupling::detect_coupling;
/// use argus_gitpulse::mining::{CommitInfo, FileChange, ChangeStatus};
///
/// let commits = vec![
///     CommitInfo {
///         hash: "abc".into(),
///         author: "alice".into(),
///         email: "alice@example.com".into(),
///         timestamp: 1000,
///         message: "change".into(),
///         files_changed: vec![
///             FileChange { path: "a.rs".into(), lines_added: 5, lines_deleted: 0, status: ChangeStatus::Modified },
///             FileChange { path: "b.rs".into(), lines_added: 3, lines_deleted: 0, status: ChangeStatus::Modified },
///         ],
///     },
/// ];
/// let pairs = detect_coupling(&commits, 0.0, 1).unwrap();
/// assert_eq!(pairs.len(), 1);
/// ```
pub fn detect_coupling(
    commits: &[CommitInfo],
    min_coupling: f64,
    min_co_changes: u32,
) -> Result<Vec<CoupledPair>, ArgusError> {
    // Count per-file changes
    let mut file_changes: HashMap<String, u32> = HashMap::new();
    // Count co-changes for pairs (normalized key: lexicographic order)
    let mut co_changes: HashMap<(String, String), u32> = HashMap::new();

    for commit in commits {
        let files: Vec<&str> = commit
            .files_changed
            .iter()
            .map(|f| f.path.as_str())
            .collect();
        let unique_files: Vec<&str> = {
            let mut seen = std::collections::HashSet::new();
            let mut unique = Vec::new();
            for f in &files {
                if seen.insert(*f) {
                    unique.push(*f);
                }
            }
            unique
        };

        // Count individual file changes
        for file in &unique_files {
            *file_changes.entry((*file).to_string()).or_default() += 1;
        }

        // Count co-changes for every pair
        for i in 0..unique_files.len() {
            for j in (i + 1)..unique_files.len() {
                let key = normalize_pair(unique_files[i], unique_files[j]);
                *co_changes.entry(key).or_default() += 1;
            }
        }
    }

    // Build coupled pairs
    let mut pairs = Vec::new();
    for ((file_a, file_b), co_count) in &co_changes {
        if *co_count < min_co_changes {
            continue;
        }

        let changes_a = file_changes.get(file_a).copied().unwrap_or(0);
        let changes_b = file_changes.get(file_b).copied().unwrap_or(0);
        let max_changes = changes_a.max(changes_b);

        if max_changes == 0 {
            continue;
        }

        let coupling_degree = *co_count as f64 / max_changes as f64;

        if coupling_degree < min_coupling {
            continue;
        }

        pairs.push(CoupledPair {
            file_a: file_a.clone(),
            file_b: file_b.clone(),
            co_changes: *co_count,
            coupling_degree,
            changes_a,
            changes_b,
        });
    }

    pairs.sort_by(|a, b| {
        b.coupling_degree
            .partial_cmp(&a.coupling_degree)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(pairs)
}

fn normalize_pair(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mining::{ChangeStatus, FileChange};

    fn make_commit(files: Vec<&str>) -> CommitInfo {
        CommitInfo {
            hash: "abc".into(),
            author: "alice".into(),
            email: "alice@example.com".into(),
            timestamp: 1000,
            message: "test".into(),
            files_changed: files
                .into_iter()
                .map(|path| FileChange {
                    path: path.into(),
                    lines_added: 5,
                    lines_deleted: 2,
                    status: ChangeStatus::Modified,
                })
                .collect(),
        }
    }

    #[test]
    fn files_always_changed_together_have_coupling_1() {
        let commits = vec![
            make_commit(vec!["a.rs", "b.rs"]),
            make_commit(vec!["a.rs", "b.rs"]),
            make_commit(vec!["a.rs", "b.rs"]),
        ];

        let pairs = detect_coupling(&commits, 0.0, 1).unwrap();
        assert_eq!(pairs.len(), 1);
        assert!((pairs[0].coupling_degree - 1.0).abs() < f64::EPSILON);
        assert_eq!(pairs[0].co_changes, 3);
    }

    #[test]
    fn files_never_changed_together_not_in_results() {
        let commits = vec![make_commit(vec!["a.rs"]), make_commit(vec!["b.rs"])];

        let pairs = detect_coupling(&commits, 0.0, 1).unwrap();
        assert!(pairs.is_empty());
    }

    #[test]
    fn min_coupling_filter_works() {
        let commits = vec![
            make_commit(vec!["a.rs", "b.rs"]),
            make_commit(vec!["a.rs"]),
            make_commit(vec!["a.rs"]),
        ];

        // coupling = 1/3 = 0.33
        let pairs_low = detect_coupling(&commits, 0.3, 1).unwrap();
        assert_eq!(pairs_low.len(), 1);

        let pairs_high = detect_coupling(&commits, 0.5, 1).unwrap();
        assert!(pairs_high.is_empty());
    }

    #[test]
    fn min_co_changes_filter_works() {
        let commits = vec![make_commit(vec!["a.rs", "b.rs"])];

        let pairs = detect_coupling(&commits, 0.0, 2).unwrap();
        assert!(pairs.is_empty(), "need at least 2 co-changes");

        let pairs = detect_coupling(&commits, 0.0, 1).unwrap();
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn pair_normalization_treats_ab_same_as_ba() {
        // Both orderings should produce the same result
        let commits = vec![
            make_commit(vec!["z.rs", "a.rs"]),
            make_commit(vec!["a.rs", "z.rs"]),
        ];

        let pairs = detect_coupling(&commits, 0.0, 1).unwrap();
        assert_eq!(pairs.len(), 1);
        // Should be normalized: a.rs < z.rs
        assert_eq!(pairs[0].file_a, "a.rs");
        assert_eq!(pairs[0].file_b, "z.rs");
        assert_eq!(pairs[0].co_changes, 2);
    }
}
