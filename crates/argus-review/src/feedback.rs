use std::path::Path;

use argus_codelens::store::{CodeIndex, Feedback};
use argus_core::{ArgusError, ReviewComment};
use sha2::{Digest, Sha256};

pub struct FeedbackStore {
    index: CodeIndex,
}

impl FeedbackStore {
    pub fn open(repo_root: &Path) -> Result<Self, ArgusError> {
        let index_path = repo_root.join(".argus/index.db");
        // Open the shared index (creates tables if needed)
        let index = CodeIndex::open(&index_path)?;
        Ok(Self { index })
    }

    pub fn add_feedback(&self, comment: &ReviewComment, verdict: &str) -> Result<(), ArgusError> {
        let hash = compute_comment_hash(comment);
        // Map verdict string to integer rating
        let rating = match verdict {
            "positive" | "useful" => 1,
            "negative" | "not useful" => -1,
            _ => 0,
        };

        let feedback = Feedback {
            comment_id: hash,
            file_path: comment.file_path.to_string_lossy().to_string(),
            line_number: Some(comment.line as usize),
            comment_text: comment.message.clone(),
            rating,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        self.index.insert_feedback(&feedback)?;
        Ok(())
    }

    pub fn get_negative_examples(&self) -> Result<Vec<String>, ArgusError> {
        self.index.get_negative_feedback(5)
    }
}

pub fn compute_comment_hash(comment: &ReviewComment) -> String {
    let mut hasher = Sha256::new();
    hasher.update(comment.file_path.to_string_lossy().as_bytes());
    // Line number might change, so maybe exclude it if we want "similar issue elsewhere"?
    // But for now, let's include it to identify the *specific* comment instance.
    // If we want to suppress "similar" issues, we might need a looser hash (e.g. just message).
    // Let's stick to strict identity for now.
    hasher.update(comment.line.to_le_bytes());
    hasher.update(comment.message.as_bytes());
    format!("{:x}", hasher.finalize())
}
