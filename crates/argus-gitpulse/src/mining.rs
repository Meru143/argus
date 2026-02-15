//! Git history extraction via git2.
//!
//! Mines commit history from a repository, extracting per-commit
//! file changes with line counts, author info, and timestamps.

use std::path::Path;

use argus_core::ArgusError;
use git2::{Delta, DiffOptions, Repository, Sort};

/// Raw commit data extracted from git history.
///
/// # Examples
///
/// ```
/// use argus_gitpulse::mining::CommitInfo;
///
/// let info = CommitInfo {
///     hash: "abc123".into(),
///     author: "alice".into(),
///     email: "alice@example.com".into(),
///     timestamp: 1700000000,
///     message: "fix: auth bug".into(),
///     files_changed: vec![],
/// };
/// assert_eq!(info.author, "alice");
/// ```
#[derive(Debug, Clone)]
pub struct CommitInfo {
    /// Short commit hash.
    pub hash: String,
    /// Author name.
    pub author: String,
    /// Author email.
    pub email: String,
    /// Unix timestamp of the commit.
    pub timestamp: i64,
    /// First line of commit message.
    pub message: String,
    /// Files modified in this commit.
    pub files_changed: Vec<FileChange>,
}

/// A single file change within a commit.
///
/// # Examples
///
/// ```
/// use argus_gitpulse::mining::{FileChange, ChangeStatus};
///
/// let change = FileChange {
///     path: "src/main.rs".into(),
///     lines_added: 10,
///     lines_deleted: 3,
///     status: ChangeStatus::Modified,
/// };
/// assert_eq!(change.lines_added, 10);
/// ```
#[derive(Debug, Clone)]
pub struct FileChange {
    /// File path relative to repo root.
    pub path: String,
    /// Lines added in this commit.
    pub lines_added: u64,
    /// Lines deleted in this commit.
    pub lines_deleted: u64,
    /// Type of change.
    pub status: ChangeStatus,
}

/// Status of a file change within a commit.
///
/// # Examples
///
/// ```
/// use argus_gitpulse::mining::ChangeStatus;
///
/// let status = ChangeStatus::Added;
/// assert_eq!(format!("{status:?}"), "Added");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeStatus {
    /// New file.
    Added,
    /// Existing file modified.
    Modified,
    /// File removed.
    Deleted,
    /// File renamed from another path.
    Renamed {
        /// Original path before rename.
        from: String,
    },
}

/// Options for history mining.
///
/// # Examples
///
/// ```
/// use argus_gitpulse::mining::MiningOptions;
///
/// let opts = MiningOptions::default();
/// assert_eq!(opts.since_days, 180);
/// assert_eq!(opts.max_files_per_commit, 25);
/// ```
pub struct MiningOptions {
    /// Only include commits from the last N days (default: 180).
    pub since_days: u64,
    /// Skip commits touching more files than this (default: 25).
    pub max_files_per_commit: usize,
    /// Branch to walk (default: HEAD).
    pub branch: Option<String>,
}

impl Default for MiningOptions {
    fn default() -> Self {
        Self {
            since_days: 180,
            max_files_per_commit: 25,
            branch: None,
        }
    }
}

/// Mine commit history from a git repository.
///
/// Returns commits in reverse chronological order (newest first).
/// Skips merge commits with more files than `max_files_per_commit`.
///
/// # Errors
///
/// Returns [`ArgusError::Git`] if the repository cannot be opened or walked.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use argus_gitpulse::mining::{mine_history, MiningOptions};
///
/// let commits = mine_history(Path::new("."), &MiningOptions::default()).unwrap();
/// for c in &commits {
///     println!("{}: {} ({})", &c.hash[..7], c.message, c.author);
/// }
/// ```
pub fn mine_history(
    repo_path: &Path,
    options: &MiningOptions,
) -> Result<Vec<CommitInfo>, ArgusError> {
    let repo = Repository::open(repo_path)
        .map_err(|e| ArgusError::Git(format!("failed to open repository: {e}")))?;

    let mut revwalk = repo
        .revwalk()
        .map_err(|e| ArgusError::Git(format!("failed to create revwalk: {e}")))?;

    revwalk.set_sorting(Sort::TIME).ok();

    // Start from HEAD or specified branch
    if let Some(ref branch) = options.branch {
        let reference = repo
            .resolve_reference_from_short_name(branch)
            .map_err(|e| ArgusError::Git(format!("failed to resolve branch '{branch}': {e}")))?;
        let oid = reference
            .target()
            .ok_or_else(|| ArgusError::Git("branch has no target".into()))?;
        revwalk
            .push(oid)
            .map_err(|e| ArgusError::Git(format!("failed to push oid: {e}")))?;
    } else {
        revwalk
            .push_head()
            .map_err(|e| ArgusError::Git(format!("failed to push HEAD: {e}")))?;
    }

    let cutoff = compute_cutoff(options.since_days);
    let mut commits = Vec::new();

    for oid_result in revwalk {
        let oid = oid_result.map_err(|e| ArgusError::Git(format!("revwalk error: {e}")))?;

        let commit = repo
            .find_commit(oid)
            .map_err(|e| ArgusError::Git(format!("failed to find commit: {e}")))?;

        let timestamp = commit.time().seconds();
        if timestamp < cutoff {
            break;
        }

        // Skip merge commits with too many parents (unless they have few file changes)
        let parent_count = commit.parent_count();
        if parent_count > 1 {
            // Check file count before skipping
            let file_count = count_diff_files(&repo, &commit)?;
            if file_count > options.max_files_per_commit {
                continue;
            }
        }

        let files_changed = extract_file_changes(&repo, &commit)?;

        // Skip commits with too many files (large refactors)
        if files_changed.len() > options.max_files_per_commit {
            continue;
        }

        let author = commit.author();
        let hash = oid.to_string();

        commits.push(CommitInfo {
            hash: hash[..hash.len().min(8)].to_string(),
            author: author.name().unwrap_or("unknown").to_string(),
            email: author.email().unwrap_or("unknown").to_string(),
            timestamp,
            message: commit
                .message()
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
            files_changed,
        });
    }

    Ok(commits)
}

fn compute_cutoff(since_days: u64) -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    now - (since_days as i64 * 86400)
}

fn count_diff_files(repo: &Repository, commit: &git2::Commit) -> Result<usize, ArgusError> {
    let commit_tree = commit
        .tree()
        .map_err(|e| ArgusError::Git(format!("failed to get commit tree: {e}")))?;

    let parent_tree = if commit.parent_count() > 0 {
        let parent = commit
            .parent(0)
            .map_err(|e| ArgusError::Git(format!("failed to get parent: {e}")))?;
        Some(
            parent
                .tree()
                .map_err(|e| ArgusError::Git(format!("failed to get parent tree: {e}")))?,
        )
    } else {
        None
    };

    let mut diff_opts = DiffOptions::new();
    let diff = repo
        .diff_tree_to_tree(
            parent_tree.as_ref(),
            Some(&commit_tree),
            Some(&mut diff_opts),
        )
        .map_err(|e| ArgusError::Git(format!("failed to compute diff: {e}")))?;

    Ok(diff.deltas().len())
}

fn extract_file_changes(
    repo: &Repository,
    commit: &git2::Commit,
) -> Result<Vec<FileChange>, ArgusError> {
    let commit_tree = commit
        .tree()
        .map_err(|e| ArgusError::Git(format!("failed to get commit tree: {e}")))?;

    let parent_tree = if commit.parent_count() > 0 {
        let parent = commit
            .parent(0)
            .map_err(|e| ArgusError::Git(format!("failed to get parent: {e}")))?;
        Some(
            parent
                .tree()
                .map_err(|e| ArgusError::Git(format!("failed to get parent tree: {e}")))?,
        )
    } else {
        None
    };

    let mut diff_opts = DiffOptions::new();
    let diff = repo
        .diff_tree_to_tree(
            parent_tree.as_ref(),
            Some(&commit_tree),
            Some(&mut diff_opts),
        )
        .map_err(|e| ArgusError::Git(format!("failed to compute diff: {e}")))?;

    // Enable rename detection
    let mut find_opts = git2::DiffFindOptions::new();
    find_opts.renames(true);
    let mut diff = diff;
    diff.find_similar(Some(&mut find_opts))
        .map_err(|e| ArgusError::Git(format!("failed to find renames: {e}")))?;

    let mut changes = Vec::new();
    let num_deltas = diff.deltas().len();

    for delta_idx in 0..num_deltas {
        let delta = diff.get_delta(delta_idx).unwrap();

        let new_file = delta.new_file();
        let path = new_file
            .path()
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .to_string();

        if path.is_empty() {
            continue;
        }

        let status = match delta.status() {
            Delta::Added => ChangeStatus::Added,
            Delta::Deleted => {
                let old_path = delta
                    .old_file()
                    .path()
                    .unwrap_or(Path::new(""))
                    .to_string_lossy()
                    .to_string();
                // Use old path for deleted files
                changes.push(FileChange {
                    path: old_path,
                    lines_added: 0,
                    lines_deleted: 0,
                    status: ChangeStatus::Deleted,
                });
                continue;
            }
            Delta::Modified => ChangeStatus::Modified,
            Delta::Renamed => {
                let old_path = delta
                    .old_file()
                    .path()
                    .unwrap_or(Path::new(""))
                    .to_string_lossy()
                    .to_string();
                ChangeStatus::Renamed { from: old_path }
            }
            _ => ChangeStatus::Modified,
        };

        changes.push(FileChange {
            path,
            lines_added: 0,
            lines_deleted: 0,
            status,
        });
    }

    // Count lines added/deleted per file using foreach
    let mut line_counts: std::collections::HashMap<String, (u64, u64)> =
        std::collections::HashMap::new();

    diff.foreach(
        &mut |_delta, _progress| true,
        None,
        None,
        Some(&mut |delta, _hunk, line| {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .unwrap_or(Path::new(""))
                .to_string_lossy()
                .to_string();

            let entry = line_counts.entry(path).or_insert((0, 0));
            match line.origin() {
                '+' => entry.0 += 1,
                '-' => entry.1 += 1,
                _ => {}
            }
            true
        }),
    )
    .map_err(|e| ArgusError::Git(format!("failed to iterate diff lines: {e}")))?;

    // Apply line counts to changes
    for change in &mut changes {
        if let Some((added, deleted)) = line_counts.get(&change.path) {
            change.lines_added = *added;
            change.lines_deleted = *deleted;
        }
    }

    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mining_options_defaults_are_correct() {
        let opts = MiningOptions::default();
        assert_eq!(opts.since_days, 180);
        assert_eq!(opts.max_files_per_commit, 25);
        assert!(opts.branch.is_none());
    }

    #[test]
    fn mine_argus_repo_returns_commits() {
        // Find the repo root (this test runs from crate dir or workspace root)
        let repo_path = find_repo_root().expect("should find repo root");
        let opts = MiningOptions {
            since_days: 365,
            ..MiningOptions::default()
        };
        let commits = mine_history(&repo_path, &opts).unwrap();
        assert!(!commits.is_empty(), "argus repo should have commits");
        // Verify basic structure
        let first = &commits[0];
        assert!(!first.hash.is_empty());
        assert!(!first.author.is_empty());
        assert!(first.timestamp > 0);
    }

    #[test]
    fn large_commits_are_skipped() {
        let repo_path = find_repo_root().expect("should find repo root");
        let opts = MiningOptions {
            since_days: 365,
            max_files_per_commit: 2, // Very small threshold
            ..MiningOptions::default()
        };
        let commits = mine_history(&repo_path, &opts).unwrap();
        // All returned commits should have <= 2 files
        for commit in &commits {
            assert!(
                commit.files_changed.len() <= 2,
                "commit {} has {} files, expected <= 2",
                commit.hash,
                commit.files_changed.len()
            );
        }
    }

    #[test]
    fn change_status_identifies_correctly() {
        let added = ChangeStatus::Added;
        let modified = ChangeStatus::Modified;
        let deleted = ChangeStatus::Deleted;
        let renamed = ChangeStatus::Renamed {
            from: "old.rs".into(),
        };

        assert_eq!(added, ChangeStatus::Added);
        assert_eq!(modified, ChangeStatus::Modified);
        assert_eq!(deleted, ChangeStatus::Deleted);
        assert_ne!(renamed, ChangeStatus::Modified);
    }

    fn find_repo_root() -> Option<std::path::PathBuf> {
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
