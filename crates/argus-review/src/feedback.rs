use std::path::Path;

use argus_core::ReviewComment;
use chrono::Utc;
use rusqlite::{params, Connection, Result};
use sha2::{Digest, Sha256};

pub struct FeedbackStore {
    conn: Connection,
}

impl FeedbackStore {
    pub fn open(repo_root: &Path) -> Result<Self> {
        let argus_dir = repo_root.join(".argus");
        if !argus_dir.exists() {
            std::fs::create_dir_all(&argus_dir).expect("failed to create .argus directory");
        }
        let db_path = argus_dir.join("feedback.db");
        let conn = Connection::open(db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS feedback (
                id INTEGER PRIMARY KEY,
                comment_hash TEXT NOT NULL,
                file_path TEXT NOT NULL,
                message TEXT NOT NULL,
                verdict TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                UNIQUE(comment_hash, verdict)
            )",
            [],
        )?;
        Ok(Self { conn })
    }

    pub fn add_feedback(&self, comment: &ReviewComment, verdict: &str) -> Result<()> {
        let hash = compute_comment_hash(comment);
        let timestamp = Utc::now().timestamp();
        self.conn.execute(
            "INSERT OR REPLACE INTO feedback (comment_hash, file_path, message, verdict, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                hash,
                comment.file_path.to_string_lossy(),
                comment.message,
                verdict,
                timestamp
            ],
        )?;
        Ok(())
    }

    pub fn get_negative_examples(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT message FROM feedback WHERE verdict = 'negative' ORDER BY timestamp DESC LIMIT 5",
        )?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut messages = Vec::new();
        for r in rows {
            messages.push(r?);
        }
        Ok(messages)
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
