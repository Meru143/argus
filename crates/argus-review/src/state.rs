use std::path::Path;

use argus_core::ReviewComment;
use chrono::{DateTime, Utc};
use miette::{IntoDiagnostic, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewState {
    /// The commit SHA of the base commit for the last review.
    ///
    /// When running an incremental review, this SHA is used as the starting point
    /// for the diff (e.g., `git diff last_reviewed_sha`).
    pub last_reviewed_sha: String,

    /// When the last review was performed.
    pub timestamp: DateTime<Utc>,

    /// The comments generated in the last review.
    ///
    /// Stored here so the `feedback` command can load them for user rating.
    #[serde(default)]
    pub comments: Vec<ReviewComment>,
}

impl ReviewState {
    /// Load the review state from the repository's `.argus` directory.
    ///
    /// Returns `Ok(None)` if the state file does not exist.
    pub fn load(repo_root: &Path) -> Result<Option<Self>> {
        let state_path = repo_root.join(".argus/review-state.json");
        if !state_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&state_path).into_diagnostic()?;
        let state = serde_json::from_str(&content).into_diagnostic()?;
        Ok(Some(state))
    }

    /// Save the review state to the repository's `.argus` directory.
    pub fn save(&self, repo_root: &Path) -> Result<()> {
        let argus_dir = repo_root.join(".argus");
        if !argus_dir.exists() {
            std::fs::create_dir_all(&argus_dir).into_diagnostic()?;
        }

        let state_path = argus_dir.join("review-state.json");
        let content = serde_json::to_string_pretty(self).into_diagnostic()?;
        std::fs::write(state_path, content).into_diagnostic()?;
        Ok(())
    }
}
