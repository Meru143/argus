//! Knowledge silo and bus factor analysis.
//!
//! Analyzes code ownership distribution across a project to identify
//! knowledge silos (files dominated by a single author) and compute
//! the project bus factor.

use std::collections::HashMap;

use argus_core::ArgusError;
use serde::{Deserialize, Serialize};

use crate::mining::CommitInfo;

/// Ownership metrics for a single file.
///
/// # Examples
///
/// ```
/// use argus_gitpulse::ownership::FileOwnership;
///
/// let ownership = FileOwnership {
///     path: "src/main.rs".into(),
///     total_commits: 20,
///     authors: vec![],
///     bus_factor: 3,
///     dominant_author_ratio: 0.45,
///     is_knowledge_silo: false,
/// };
/// assert!(!ownership.is_knowledge_silo);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOwnership {
    /// File path relative to repo root.
    pub path: String,
    /// Total commits touching this file.
    pub total_commits: u32,
    /// Per-author contribution breakdown.
    pub authors: Vec<AuthorContribution>,
    /// Number of authors with >10% contribution.
    pub bus_factor: u32,
    /// `max(author_commits) / total_commits`.
    pub dominant_author_ratio: f64,
    /// Whether `dominant_author_ratio > 0.80`.
    pub is_knowledge_silo: bool,
}

/// Per-author contribution to a file.
///
/// # Examples
///
/// ```
/// use argus_gitpulse::ownership::AuthorContribution;
///
/// let contrib = AuthorContribution {
///     name: "alice".into(),
///     email: "alice@example.com".into(),
///     commits: 15,
///     ratio: 0.75,
/// };
/// assert!(contrib.ratio > 0.5);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorContribution {
    /// Author name.
    pub name: String,
    /// Author email.
    pub email: String,
    /// Number of commits by this author.
    pub commits: u32,
    /// `commits / total_commits` for this file.
    pub ratio: f64,
}

/// Summary of knowledge distribution across the project.
///
/// # Examples
///
/// ```
/// use argus_gitpulse::ownership::OwnershipSummary;
///
/// let summary = OwnershipSummary {
///     total_files: 50,
///     single_author_files: 10,
///     knowledge_silos: 15,
///     project_bus_factor: 2,
///     files: vec![],
/// };
/// assert_eq!(summary.total_files, 50);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnershipSummary {
    /// Total files analyzed.
    pub total_files: usize,
    /// Files with only one author.
    pub single_author_files: usize,
    /// Files where one author has >80% of commits.
    pub knowledge_silos: usize,
    /// Minimum authors to remove to orphan >50% of files.
    pub project_bus_factor: u32,
    /// Per-file ownership data.
    pub files: Vec<FileOwnership>,
}

/// Analyze code ownership and knowledge distribution.
///
/// # Errors
///
/// Returns [`ArgusError`] on processing failure.
///
/// # Examples
///
/// ```
/// use argus_gitpulse::ownership::analyze_ownership;
/// use argus_gitpulse::mining::{CommitInfo, FileChange, ChangeStatus};
///
/// let commits = vec![
///     CommitInfo {
///         hash: "abc".into(),
///         author: "alice".into(),
///         email: "alice@example.com".into(),
///         timestamp: 1000,
///         message: "init".into(),
///         files_changed: vec![
///             FileChange { path: "main.rs".into(), lines_added: 50, lines_deleted: 0, status: ChangeStatus::Added },
///         ],
///     },
/// ];
/// let summary = analyze_ownership(&commits).unwrap();
/// assert_eq!(summary.total_files, 1);
/// ```
pub fn analyze_ownership(commits: &[CommitInfo]) -> Result<OwnershipSummary, ArgusError> {
    // Accumulate per-file, per-author commit counts
    // Key: file path, Value: map of (author_name, email) -> commit count
    let mut file_authors: HashMap<String, HashMap<(String, String), u32>> = HashMap::new();

    for commit in commits {
        let author_key = (commit.author.clone(), commit.email.clone());
        for file in &commit.files_changed {
            *file_authors
                .entry(file.path.clone())
                .or_default()
                .entry(author_key.clone())
                .or_default() += 1;
        }
    }

    let mut files = Vec::new();
    let mut single_author_files = 0usize;
    let mut knowledge_silos = 0usize;

    for (path, author_map) in &file_authors {
        let total_commits: u32 = author_map.values().sum();
        if total_commits == 0 {
            continue;
        }

        let mut author_contribs: Vec<AuthorContribution> = Vec::new();
        let mut max_commits = 0u32;

        for ((name, email), count) in author_map {
            let ratio = *count as f64 / total_commits as f64;
            if *count > max_commits {
                max_commits = *count;
            }
            author_contribs.push(AuthorContribution {
                name: name.clone(),
                email: email.clone(),
                commits: *count,
                ratio,
            });
        }

        // Sort authors by commits descending
        author_contribs.sort_by(|a, b| b.commits.cmp(&a.commits));

        let dominant_author_ratio = max_commits as f64 / total_commits as f64;
        let bus_factor = author_contribs.iter().filter(|a| a.ratio > 0.10).count() as u32;
        let is_silo = dominant_author_ratio > 0.80;

        if author_contribs.len() == 1 {
            single_author_files += 1;
        }
        if is_silo {
            knowledge_silos += 1;
        }

        files.push(FileOwnership {
            path: path.clone(),
            total_commits,
            authors: author_contribs,
            bus_factor,
            dominant_author_ratio,
            is_knowledge_silo: is_silo,
        });
    }

    // Sort by dominant_author_ratio descending (silos first)
    files.sort_by(|a, b| {
        b.dominant_author_ratio
            .partial_cmp(&a.dominant_author_ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let project_bus_factor = compute_project_bus_factor(&files);

    Ok(OwnershipSummary {
        total_files: files.len(),
        single_author_files,
        knowledge_silos,
        project_bus_factor,
        files,
    })
}

/// Compute the project bus factor.
///
/// Iteratively remove the top contributor until >50% of files lose
/// all "significant" authors (those with >10% ratio).
fn compute_project_bus_factor(files: &[FileOwnership]) -> u32 {
    if files.is_empty() {
        return 0;
    }

    // Collect all unique authors across all files
    let mut all_authors: HashMap<String, u32> = HashMap::new();
    for file in files {
        for author in &file.authors {
            *all_authors.entry(author.email.clone()).or_default() += 1;
        }
    }

    // Sort authors by number of files they contribute to (descending)
    let mut sorted_authors: Vec<(String, u32)> = all_authors.into_iter().collect();
    sorted_authors.sort_by(|a, b| b.1.cmp(&a.1));

    let total_files = files.len();
    let threshold = total_files / 2;
    let mut removed_authors: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut removals = 0u32;

    for (author_email, _) in &sorted_authors {
        removed_authors.insert(author_email.clone());
        removals += 1;

        // Count files that have lost all significant authors
        let mut orphaned = 0usize;
        for file in files {
            let has_significant_author = file
                .authors
                .iter()
                .any(|a| a.ratio > 0.10 && !removed_authors.contains(&a.email));
            if !has_significant_author {
                orphaned += 1;
            }
        }

        if orphaned > threshold {
            return removals;
        }
    }

    removals
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mining::{ChangeStatus, FileChange};

    fn make_commit(author: &str, email: &str, files: Vec<&str>) -> CommitInfo {
        CommitInfo {
            hash: "abc".into(),
            author: author.into(),
            email: email.into(),
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
    fn single_author_file_is_knowledge_silo() {
        let commits = vec![
            make_commit("alice", "alice@example.com", vec!["main.rs"]),
            make_commit("alice", "alice@example.com", vec!["main.rs"]),
            make_commit("alice", "alice@example.com", vec!["main.rs"]),
        ];

        let summary = analyze_ownership(&commits).unwrap();
        assert_eq!(summary.total_files, 1);
        assert_eq!(summary.single_author_files, 1);
        assert_eq!(summary.knowledge_silos, 1);

        let file = &summary.files[0];
        assert_eq!(file.bus_factor, 1);
        assert!(file.is_knowledge_silo);
        assert!((file.dominant_author_ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn five_equal_authors_not_a_silo() {
        let commits = vec![
            make_commit("alice", "alice@e.com", vec!["main.rs"]),
            make_commit("bob", "bob@e.com", vec!["main.rs"]),
            make_commit("carol", "carol@e.com", vec!["main.rs"]),
            make_commit("dave", "dave@e.com", vec!["main.rs"]),
            make_commit("eve", "eve@e.com", vec!["main.rs"]),
        ];

        let summary = analyze_ownership(&commits).unwrap();
        let file = &summary.files[0];
        assert_eq!(file.bus_factor, 5);
        assert!(!file.is_knowledge_silo);
        assert!((file.dominant_author_ratio - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn dominant_author_ratio_calculation() {
        let commits = vec![
            make_commit("alice", "alice@e.com", vec!["main.rs"]),
            make_commit("alice", "alice@e.com", vec!["main.rs"]),
            make_commit("alice", "alice@e.com", vec!["main.rs"]),
            make_commit("bob", "bob@e.com", vec!["main.rs"]),
        ];

        let summary = analyze_ownership(&commits).unwrap();
        let file = &summary.files[0];
        // alice: 3/4 = 0.75
        assert!((file.dominant_author_ratio - 0.75).abs() < f64::EPSILON);
        assert!(!file.is_knowledge_silo); // 0.75 < 0.80
    }

    #[test]
    fn project_bus_factor_calculation() {
        // alice owns file1 exclusively, bob owns file2 exclusively,
        // carol owns file3 exclusively
        let commits = vec![
            make_commit("alice", "alice@e.com", vec!["file1.rs"]),
            make_commit("bob", "bob@e.com", vec!["file2.rs"]),
            make_commit("carol", "carol@e.com", vec!["file3.rs"]),
        ];

        let summary = analyze_ownership(&commits).unwrap();
        // Removing any 2 authors orphans >50% of files (2 out of 3)
        assert_eq!(summary.project_bus_factor, 2);
    }
}
