//! SQLite + FTS5 storage for code chunks and embeddings.
//!
//! Stores chunks in SQLite with FTS5 for keyword search and BLOBs for
//! vector embeddings. Cosine similarity is computed in Rust for vector search.

use std::path::{Path, PathBuf};

use argus_core::ArgusError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::chunker::CodeChunk;

/// A hit from a search operation.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use argus_codelens::store::{SearchHit, SearchSource};
/// use argus_codelens::chunker::CodeChunk;
///
/// let hit = SearchHit {
///     chunk: CodeChunk {
///         file_path: PathBuf::from("src/main.rs"),
///         start_line: 1,
///         end_line: 5,
///         entity_name: "main".into(),
///         entity_type: "function".into(),
///         language: "rust".into(),
///         content: "fn main() {}".into(),
///         context_header: "# File: src/main.rs".into(),
///         content_hash: "abc".into(),
///     },
///     score: 0.95,
///     source: SearchSource::Vector,
/// };
/// assert!(hit.score > 0.9);
/// ```
pub struct SearchHit {
    /// The matched chunk (without embedding).
    pub chunk: CodeChunk,
    /// Relevance score.
    pub score: f64,
    /// Whether this hit came from vector or keyword search.
    pub source: SearchSource,
}

/// Source of a search hit.
///
/// # Examples
///
/// ```
/// use argus_codelens::store::SearchSource;
///
/// let source = SearchSource::Vector;
/// assert!(matches!(source, SearchSource::Vector));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchSource {
    /// Result from vector similarity search.
    Vector,
    /// Result from FTS5 keyword search.
    Keyword,
}

/// Index statistics.
///
/// # Examples
///
/// ```
/// use argus_codelens::store::IndexStats;
///
/// let stats = IndexStats {
///     total_chunks: 100,
///     total_files: 10,
///     index_size_bytes: 50000,
/// };
/// assert_eq!(stats.total_chunks, 100);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStats {
    /// Total number of chunks in the index.
    pub total_chunks: usize,
    /// Total number of unique files indexed.
    pub total_files: usize,
    /// Size of the index database in bytes.
    pub index_size_bytes: u64,
}

/// SQLite-based code index with FTS5 keyword search and BLOB-stored embeddings.
///
/// # Examples
///
/// ```
/// use argus_codelens::store::CodeIndex;
///
/// let index = CodeIndex::in_memory().unwrap();
/// let stats = index.stats().unwrap();
/// assert_eq!(stats.total_chunks, 0);
/// ```
pub struct CodeIndex {
    conn: Connection,
}

impl CodeIndex {
    /// Open or create an index database at the given path.
    ///
    /// Creates tables if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] if the database cannot be opened.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::Path;
    /// use argus_codelens::store::CodeIndex;
    ///
    /// let index = CodeIndex::open(Path::new(".argus/index.db")).unwrap();
    /// ```
    pub fn open(path: &Path) -> Result<Self, ArgusError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ArgusError::Database(format!("failed to create index directory: {e}"))
            })?;
        }
        let conn = Connection::open(path)
            .map_err(|e| ArgusError::Database(format!("failed to open database: {e}")))?;

        let index = Self { conn };
        index.init_schema()?;
        Ok(index)
    }

    /// Create an in-memory index (for testing).
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] if schema creation fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_codelens::store::CodeIndex;
    ///
    /// let index = CodeIndex::in_memory().unwrap();
    /// ```
    pub fn in_memory() -> Result<Self, ArgusError> {
        let conn = Connection::open_in_memory().map_err(|e| {
            ArgusError::Database(format!("failed to create in-memory database: {e}"))
        })?;

        let index = Self { conn };
        index.init_schema()?;
        Ok(index)
    }

    fn init_schema(&self) -> Result<(), ArgusError> {
        self.conn
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS metadata (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS files (
                    path TEXT PRIMARY KEY,
                    content_hash TEXT NOT NULL,
                    indexed_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS chunks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    file_path TEXT NOT NULL,
                    content_hash TEXT NOT NULL UNIQUE,
                    start_line INTEGER NOT NULL,
                    end_line INTEGER NOT NULL,
                    entity_name TEXT NOT NULL,
                    entity_type TEXT NOT NULL,
                    language TEXT NOT NULL,
                    content TEXT NOT NULL,
                    context_header TEXT NOT NULL,
                    embedding BLOB,
                    FOREIGN KEY (file_path) REFERENCES files(path)
                );

                CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                    entity_name, content, context_header,
                    content='chunks', content_rowid='id'
                );

                -- Triggers to keep FTS in sync
                CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
                    INSERT INTO chunks_fts(rowid, entity_name, content, context_header)
                    VALUES (new.id, new.entity_name, new.content, new.context_header);
                END;

                CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
                    INSERT INTO chunks_fts(chunks_fts, rowid, entity_name, content, context_header)
                    VALUES ('delete', old.id, old.entity_name, old.content, old.context_header);
                END;

                CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
                    INSERT INTO chunks_fts(chunks_fts, rowid, entity_name, content, context_header)
                    VALUES ('delete', old.id, old.entity_name, old.content, old.context_header);
                    INSERT INTO chunks_fts(rowid, entity_name, content, context_header)
                    VALUES (new.id, new.entity_name, new.content, new.context_header);
                END;
                ",
            )
            .map_err(|e| ArgusError::Database(format!("failed to create schema: {e}")))?;

        Ok(())
    }

    /// Store embedding dimensions in the metadata table.
    ///
    /// If dimensions are already stored and match, this is a no-op.
    /// If they don't match, returns an error suggesting re-indexing.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] if dimensions conflict with
    /// an existing index.
    pub fn set_dimensions(&self, dimensions: usize) -> Result<(), ArgusError> {
        let existing = self.get_metadata("embedding_dimensions")?;

        if let Some(stored) = existing {
            let stored_dims: usize = stored.parse().map_err(|_| {
                ArgusError::Database(format!(
                    "Corrupted dimension metadata in index: '{stored}'"
                ))
            })?;
            if stored_dims != dimensions {
                return Err(ArgusError::Database(format!(
                    "Index was created with {stored_dims} dimensions but config specifies {dimensions}. \
                     Re-index with --index to rebuild."
                )));
            }
            return Ok(());
        }

        self.set_metadata("embedding_dimensions", &dimensions.to_string())
    }

    /// Get embedding dimensions stored in metadata, if any.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] on query failure.
    pub fn get_dimensions(&self) -> Result<Option<usize>, ArgusError> {
        let value = self.get_metadata("embedding_dimensions")?;
        match value {
            Some(v) => {
                let dims: usize = v.parse().map_err(|_| {
                    ArgusError::Database(format!(
                        "Corrupted dimension metadata in index: '{v}'"
                    ))
                })?;
                Ok(Some(dims))
            }
            None => Ok(None),
        }
    }

    fn get_metadata(&self, key: &str) -> Result<Option<String>, ArgusError> {
        let result = self.conn.query_row(
            "SELECT value FROM metadata WHERE key = ?1",
            params![key],
            |row| row.get(0),
        );

        match result {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ArgusError::Database(format!(
                "failed to get metadata '{key}': {e}"
            ))),
        }
    }

    fn set_metadata(&self, key: &str, value: &str) -> Result<(), ArgusError> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO metadata (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .map_err(|e| ArgusError::Database(format!("failed to set metadata '{key}': {e}")))?;
        Ok(())
    }

    /// Store a chunk with its embedding.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] on insert failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use argus_codelens::store::CodeIndex;
    /// use argus_codelens::chunker::CodeChunk;
    ///
    /// let index = CodeIndex::in_memory().unwrap();
    /// index.record_file(std::path::Path::new("src/main.rs"), "file_hash").unwrap();
    /// let chunk = CodeChunk {
    ///     file_path: PathBuf::from("src/main.rs"),
    ///     start_line: 1, end_line: 3,
    ///     entity_name: "main".into(), entity_type: "function".into(),
    ///     language: "rust".into(), content: "fn main() {}".into(),
    ///     context_header: "# File: src/main.rs".into(),
    ///     content_hash: "abc123".into(),
    /// };
    /// index.insert_chunk(&chunk, &[0.1, 0.2, 0.3]).unwrap();
    /// ```
    pub fn insert_chunk(&self, chunk: &CodeChunk, embedding: &[f32]) -> Result<(), ArgusError> {
        let embedding_bytes = floats_to_bytes(embedding);

        self.conn
            .execute(
                "INSERT OR REPLACE INTO chunks
                 (file_path, content_hash, start_line, end_line, entity_name, entity_type,
                  language, content, context_header, embedding)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    chunk.file_path.to_string_lossy().to_string(),
                    chunk.content_hash,
                    chunk.start_line,
                    chunk.end_line,
                    chunk.entity_name,
                    chunk.entity_type,
                    chunk.language,
                    chunk.content,
                    chunk.context_header,
                    embedding_bytes,
                ],
            )
            .map_err(|e| ArgusError::Database(format!("failed to insert chunk: {e}")))?;

        Ok(())
    }

    /// Batch insert chunks with embeddings.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] on insert failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use argus_codelens::store::CodeIndex;
    /// use argus_codelens::chunker::CodeChunk;
    ///
    /// let index = CodeIndex::in_memory().unwrap();
    /// index.record_file(std::path::Path::new("src/main.rs"), "file_hash").unwrap();
    /// let chunk = CodeChunk {
    ///     file_path: PathBuf::from("src/main.rs"),
    ///     start_line: 1, end_line: 3,
    ///     entity_name: "main".into(), entity_type: "function".into(),
    ///     language: "rust".into(), content: "fn main() {}".into(),
    ///     context_header: "# File: src/main.rs".into(),
    ///     content_hash: "abc123".into(),
    /// };
    /// index.insert_chunks(&[(chunk, vec![0.1, 0.2, 0.3])]).unwrap();
    /// ```
    pub fn insert_chunks(&self, chunks: &[(CodeChunk, Vec<f32>)]) -> Result<(), ArgusError> {
        for (chunk, embedding) in chunks {
            self.insert_chunk(chunk, embedding)?;
        }
        Ok(())
    }

    /// Vector similarity search (cosine similarity computed in Rust).
    ///
    /// Loads all embeddings from the database and computes cosine similarity
    /// against the query embedding. Returns the top `limit` results sorted by score.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] on query failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_codelens::store::CodeIndex;
    ///
    /// let index = CodeIndex::in_memory().unwrap();
    /// let results = index.vector_search(&[0.1, 0.2], 5).unwrap();
    /// assert!(results.is_empty());
    /// ```
    pub fn vector_search(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<SearchHit>, ArgusError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, file_path, content_hash, start_line, end_line, entity_name,
                        entity_type, language, content, context_header, embedding
                 FROM chunks WHERE embedding IS NOT NULL",
            )
            .map_err(|e| ArgusError::Database(format!("failed to prepare query: {e}")))?;

        let mut scored: Vec<(f64, CodeChunk)> = Vec::new();

        let rows = stmt
            .query_map([], |row| {
                let embedding_bytes: Vec<u8> = row.get(10)?;
                let embedding = bytes_to_floats(&embedding_bytes);
                let score = cosine_similarity(query_embedding, &embedding);

                let chunk = CodeChunk {
                    file_path: PathBuf::from(row.get::<_, String>(1)?),
                    content_hash: row.get(2)?,
                    start_line: row.get(3)?,
                    end_line: row.get(4)?,
                    entity_name: row.get(5)?,
                    entity_type: row.get(6)?,
                    language: row.get(7)?,
                    content: row.get(8)?,
                    context_header: row.get(9)?,
                };

                Ok((score, chunk))
            })
            .map_err(|e| ArgusError::Database(format!("failed to query chunks: {e}")))?;

        for row in rows {
            let (score, chunk) =
                row.map_err(|e| ArgusError::Database(format!("failed to read row: {e}")))?;
            scored.push((score, chunk));
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);

        let hits = scored
            .into_iter()
            .map(|(score, chunk)| SearchHit {
                chunk,
                score,
                source: SearchSource::Vector,
            })
            .collect();

        Ok(hits)
    }

    /// Full-text keyword search via FTS5.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] on query failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_codelens::store::CodeIndex;
    ///
    /// let index = CodeIndex::in_memory().unwrap();
    /// let results = index.keyword_search("main", 5).unwrap();
    /// assert!(results.is_empty());
    /// ```
    pub fn keyword_search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, ArgusError> {
        // Escape FTS5 special characters for safety
        let safe_query = sanitize_fts_query(query);
        if safe_query.is_empty() {
            return Ok(Vec::new());
        }

        let mut stmt = self
            .conn
            .prepare(
                "SELECT c.id, c.file_path, c.content_hash, c.start_line, c.end_line,
                        c.entity_name, c.entity_type, c.language, c.content, c.context_header,
                        rank
                 FROM chunks_fts f
                 JOIN chunks c ON c.id = f.rowid
                 WHERE chunks_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )
            .map_err(|e| ArgusError::Database(format!("failed to prepare FTS query: {e}")))?;

        let rows = stmt
            .query_map(params![safe_query, limit as i64], |row| {
                let rank: f64 = row.get(10)?;
                let chunk = CodeChunk {
                    file_path: PathBuf::from(row.get::<_, String>(1)?),
                    content_hash: row.get(2)?,
                    start_line: row.get(3)?,
                    end_line: row.get(4)?,
                    entity_name: row.get(5)?,
                    entity_type: row.get(6)?,
                    language: row.get(7)?,
                    content: row.get(8)?,
                    context_header: row.get(9)?,
                };
                // FTS5 rank is negative (more negative = more relevant), convert to positive score
                Ok(((-rank).max(0.0), chunk))
            })
            .map_err(|e| ArgusError::Database(format!("FTS query failed: {e}")))?;

        let mut hits = Vec::new();
        for row in rows {
            let (score, chunk) =
                row.map_err(|e| ArgusError::Database(format!("failed to read FTS row: {e}")))?;
            hits.push(SearchHit {
                chunk,
                score,
                source: SearchSource::Keyword,
            });
        }

        Ok(hits)
    }

    /// Check if a chunk with this `content_hash` already exists.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] on query failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_codelens::store::CodeIndex;
    ///
    /// let index = CodeIndex::in_memory().unwrap();
    /// assert!(!index.has_chunk("nonexistent").unwrap());
    /// ```
    pub fn has_chunk(&self, content_hash: &str) -> Result<bool, ArgusError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE content_hash = ?1",
                params![content_hash],
                |row| row.get(0),
            )
            .map_err(|e| ArgusError::Database(format!("failed to check chunk: {e}")))?;

        Ok(count > 0)
    }

    /// Remove all chunks for a given file path (for re-indexing).
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] on delete failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::Path;
    /// use argus_codelens::store::CodeIndex;
    ///
    /// let index = CodeIndex::in_memory().unwrap();
    /// index.remove_file(Path::new("src/main.rs")).unwrap();
    /// ```
    pub fn remove_file(&self, file_path: &Path) -> Result<(), ArgusError> {
        let path_str = file_path.to_string_lossy().to_string();

        self.conn
            .execute("DELETE FROM chunks WHERE file_path = ?1", params![path_str])
            .map_err(|e| ArgusError::Database(format!("failed to delete chunks: {e}")))?;

        self.conn
            .execute("DELETE FROM files WHERE path = ?1", params![path_str])
            .map_err(|e| ArgusError::Database(format!("failed to delete file record: {e}")))?;

        Ok(())
    }

    /// Record a file as indexed.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] on insert failure.
    pub fn record_file(&self, file_path: &Path, content_hash: &str) -> Result<(), ArgusError> {
        let path_str = file_path.to_string_lossy().to_string();
        let now = chrono_now();

        self.conn
            .execute(
                "INSERT OR REPLACE INTO files (path, content_hash, indexed_at)
                 VALUES (?1, ?2, ?3)",
                params![path_str, content_hash, now],
            )
            .map_err(|e| ArgusError::Database(format!("failed to record file: {e}")))?;

        Ok(())
    }

    /// Get the stored content hash for a file, if it has been indexed.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] on query failure.
    pub fn file_hash(&self, file_path: &Path) -> Result<Option<String>, ArgusError> {
        let path_str = file_path.to_string_lossy().to_string();

        let result = self.conn.query_row(
            "SELECT content_hash FROM files WHERE path = ?1",
            params![path_str],
            |row| row.get(0),
        );

        match result {
            Ok(hash) => Ok(Some(hash)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(ArgusError::Database(format!(
                "failed to get file hash: {e}"
            ))),
        }
    }

    /// Get all indexed file paths.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] on query failure.
    pub fn indexed_files(&self) -> Result<Vec<String>, ArgusError> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM files")
            .map_err(|e| ArgusError::Database(format!("failed to prepare query: {e}")))?;

        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| ArgusError::Database(format!("failed to query files: {e}")))?;

        let mut paths = Vec::new();
        for row in rows {
            let path: String =
                row.map_err(|e| ArgusError::Database(format!("failed to read row: {e}")))?;
            paths.push(path);
        }

        Ok(paths)
    }

    /// Get index statistics.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Database`] on query failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_codelens::store::CodeIndex;
    ///
    /// let index = CodeIndex::in_memory().unwrap();
    /// let stats = index.stats().unwrap();
    /// assert_eq!(stats.total_chunks, 0);
    /// assert_eq!(stats.total_files, 0);
    /// ```
    pub fn stats(&self) -> Result<IndexStats, ArgusError> {
        let total_chunks: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .map_err(|e| ArgusError::Database(format!("failed to count chunks: {e}")))?;

        let total_files: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .map_err(|e| ArgusError::Database(format!("failed to count files: {e}")))?;

        // For in-memory databases, page_count returns a small number
        let page_count: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .unwrap_or(0);
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))
            .unwrap_or(4096);

        Ok(IndexStats {
            total_chunks: total_chunks as usize,
            total_files: total_files as usize,
            index_size_bytes: (page_count * page_size) as u64,
        })
    }
}

fn floats_to_bytes(floats: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(floats.len() * 4);
    for f in floats {
        bytes.extend_from_slice(&f.to_le_bytes());
    }
    bytes
}

fn bytes_to_floats(bytes: &[u8]) -> Vec<f32> {
    let mut floats = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let arr: [u8; 4] = [chunk[0], chunk[1], chunk[2], chunk[3]];
        floats.push(f32::from_le_bytes(arr));
    }
    floats
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;

    for i in 0..a.len() {
        let ai = a[i] as f64;
        let bi = b[i] as f64;
        dot += ai * bi;
        norm_a += ai * ai;
        norm_b += bi * bi;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        return 0.0;
    }

    dot / denom
}

fn sanitize_fts_query(query: &str) -> String {
    // Split into words, wrap each in quotes for exact matching
    let words: Vec<String> = query
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(|w| {
            // Remove FTS5 special chars
            let clean: String = w
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            format!("\"{clean}\"")
        })
        .filter(|w| w != "\"\"")
        .collect();
    words.join(" OR ")
}

fn chrono_now() -> String {
    // Simple ISO 8601 timestamp without chrono dependency
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_chunk(name: &str, content: &str) -> CodeChunk {
        CodeChunk {
            file_path: PathBuf::from("src/main.rs"),
            start_line: 1,
            end_line: 5,
            entity_name: name.into(),
            entity_type: "function".into(),
            language: "rust".into(),
            content: content.into(),
            context_header: format!("# File: src/main.rs\n# Name: {name}"),
            content_hash: format!("hash_{name}"),
        }
    }

    #[test]
    fn create_index_and_insert() {
        let index = CodeIndex::in_memory().unwrap();
        index
            .record_file(Path::new("src/main.rs"), "file_hash")
            .unwrap();
        let chunk = sample_chunk("main", "fn main() {}");
        index.insert_chunk(&chunk, &[0.1, 0.2, 0.3]).unwrap();

        let stats = index.stats().unwrap();
        assert_eq!(stats.total_chunks, 1);
    }

    #[test]
    fn vector_search_finds_similar() {
        let index = CodeIndex::in_memory().unwrap();
        index
            .record_file(Path::new("src/main.rs"), "file_hash")
            .unwrap();

        let chunk1 = sample_chunk("auth", "fn authenticate(user: &str) -> bool { true }");
        let chunk2 = sample_chunk("parse", "fn parse_json(data: &str) -> Value { todo!() }");

        // auth chunk has embedding [1, 0, 0], parse has [0, 1, 0]
        index.insert_chunk(&chunk1, &[1.0, 0.0, 0.0]).unwrap();
        index.insert_chunk(&chunk2, &[0.0, 1.0, 0.0]).unwrap();

        // Query for something close to auth
        let results = index.vector_search(&[0.9, 0.1, 0.0], 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].chunk.entity_name, "auth");
        assert!(matches!(results[0].source, SearchSource::Vector));
    }

    #[test]
    fn keyword_search_finds_by_name() {
        let index = CodeIndex::in_memory().unwrap();
        index
            .record_file(Path::new("src/main.rs"), "file_hash")
            .unwrap();

        let chunk = sample_chunk("process_payment", "fn process_payment(amount: f64) { }");
        index.insert_chunk(&chunk, &[0.1, 0.2]).unwrap();

        let results = index.keyword_search("process_payment", 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.entity_name, "process_payment");
        assert!(matches!(results[0].source, SearchSource::Keyword));
    }

    #[test]
    fn has_chunk_dedup_works() {
        let index = CodeIndex::in_memory().unwrap();
        index
            .record_file(Path::new("src/main.rs"), "file_hash")
            .unwrap();
        assert!(!index.has_chunk("hash_test").unwrap());

        let chunk = sample_chunk("test", "fn test() {}");
        index.insert_chunk(&chunk, &[0.1]).unwrap();
        assert!(index.has_chunk("hash_test").unwrap());
    }

    #[test]
    fn remove_file_cleans_up() {
        let index = CodeIndex::in_memory().unwrap();
        index
            .record_file(Path::new("src/main.rs"), "file_hash_123")
            .unwrap();

        let chunk = sample_chunk("main", "fn main() {}");
        index.insert_chunk(&chunk, &[0.1]).unwrap();

        assert_eq!(index.stats().unwrap().total_chunks, 1);

        index.remove_file(Path::new("src/main.rs")).unwrap();
        assert_eq!(index.stats().unwrap().total_chunks, 0);
    }

    #[test]
    fn stats_are_correct() {
        let index = CodeIndex::in_memory().unwrap();

        let stats = index.stats().unwrap();
        assert_eq!(stats.total_chunks, 0);
        assert_eq!(stats.total_files, 0);

        let chunk1 = sample_chunk("func1", "fn func1() {}");
        let mut chunk2 = sample_chunk("func2", "fn func2() {}");
        chunk2.file_path = PathBuf::from("src/other.rs");

        index
            .record_file(Path::new("src/main.rs"), "hash1")
            .unwrap();
        index
            .record_file(Path::new("src/other.rs"), "hash2")
            .unwrap();
        index.insert_chunk(&chunk1, &[0.1]).unwrap();
        index.insert_chunk(&chunk2, &[0.2]).unwrap();

        let stats = index.stats().unwrap();
        assert_eq!(stats.total_chunks, 2);
        assert_eq!(stats.total_files, 2);
    }

    #[test]
    fn cosine_similarity_correct() {
        // Identical vectors
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        // Orthogonal vectors
        assert!((cosine_similarity(&[1.0, 0.0], &[0.0, 1.0])).abs() < 1e-6);
        // Opposite vectors
        assert!((cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn floats_bytes_roundtrip() {
        let original = vec![1.0f32, -2.5, 0.0, 3.14];
        let bytes = floats_to_bytes(&original);
        let recovered = bytes_to_floats(&bytes);
        assert_eq!(original, recovered);
    }

    #[test]
    fn set_dimensions_stores_and_validates() {
        let index = CodeIndex::in_memory().unwrap();

        // First set succeeds
        index.set_dimensions(1024).unwrap();
        assert_eq!(index.get_dimensions().unwrap(), Some(1024));

        // Same dimensions is a no-op
        index.set_dimensions(1024).unwrap();

        // Different dimensions returns error
        let result = index.set_dimensions(768);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("1024"));
        assert!(err.contains("768"));
        assert!(err.contains("Re-index"));
    }

    #[test]
    fn get_dimensions_returns_none_for_new_index() {
        let index = CodeIndex::in_memory().unwrap();
        assert_eq!(index.get_dimensions().unwrap(), None);
    }
}
