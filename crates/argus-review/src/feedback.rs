use std::path::Path;

use argus_codelens::store::{CodeIndex, Feedback};
use argus_core::ReviewComment;
use miette::Result; // Use miette::Result for easier error handling across crates
use sha2::{Digest, Sha256};

pub struct FeedbackStore {
    index: CodeIndex,
}

impl FeedbackStore {
    /// Opens a FeedbackStore backed by the repository's shared code index.
    ///
    /// The index file is located at `<repo_root>/.argus/index.db`; this will create
    /// the index and any required tables if they do not exist.
    ///
    /// # Parameters
    ///
    /// - `repo_root`: path to the repository root where the `.argus` directory resides.
    ///
    /// # Returns
    ///
    /// A `FeedbackStore` connected to the repository's shared `CodeIndex` on success.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    /// let store = argus_review::feedback::FeedbackStore::open(Path::new("."));
    /// assert!(store.is_ok());
    /// ```
    pub fn open(repo_root: &Path) -> Result<Self> {
        let index_path = repo_root.join(".argus/index.db");
        // Open the shared index (creates tables if needed)
        let index = CodeIndex::open(&index_path)?;
        Ok(Self { index })
    }

    /// Adds a feedback entry for a review comment into the shared index.
    ///
    /// The supplied `verdict` is mapped to an integer rating: `"positive"` or `"useful"` → `1`,
    /// `"negative"` or `"not useful"` → `-1`, any other value → `0`. The feedback record
    /// includes a deterministic comment identifier, file path, line number, comment text,
    /// rating, and the current UTC timestamp.
    ///
    /// # Parameters
    ///
    /// - `comment`: the review comment to record feedback for.
    /// - `verdict`: a short textual verdict that determines the numeric rating.
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or an `Err` if inserting the feedback into the index fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    /// // Assuming FeedbackStore, ReviewComment are in scope and constructible.
    /// let store = FeedbackStore::open(Path::new(".")).unwrap();
    /// let comment = ReviewComment {
    ///     file_path: Path::new("src/lib.rs").to_path_buf(),
    ///     line: 42,
    ///     message: "Consider renaming this variable".to_string(),
    ///     ..Default::default()
    /// };
    /// store.add_feedback(&comment, "useful").unwrap();
    /// ```
    pub fn add_feedback(&self, comment: &ReviewComment, verdict: &str) -> Result<()> {
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

    /// Retrieves up to five negative feedback entries.
    ///
    /// Returns a `Vec<String>` containing up to five feedback texts that were recorded with negative ratings; the vector may be empty if there are no negative entries.
    ///
    /// # Examples
    ///
    /// ```
    /// // assuming `store` is a `FeedbackStore`
    /// let examples = store.get_negative_examples().unwrap();
    /// assert!(examples.len() <= 5);
    /// ```
    pub fn get_negative_examples(&self) -> Result<Vec<String>> {
        Ok(self.index.get_negative_feedback(5)?)
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
