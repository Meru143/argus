//! Hybrid search with Reciprocal Rank Fusion (RRF).
//!
//! Combines vector similarity and keyword search results using RRF scoring
//! for better retrieval quality than either method alone.

use std::collections::HashMap;
use std::path::Path;

use argus_core::{ArgusError, SearchResult};
use sha2::{Digest, Sha256};

use crate::chunker::{chunk_file, CodeChunk};
use crate::embedding::EmbeddingClient;
use crate::store::{CodeIndex, IndexStats, SearchHit};

/// Hybrid search engine combining vector and keyword search with RRF fusion.
///
/// # Examples
///
/// ```no_run
/// use argus_codelens::search::HybridSearch;
/// use argus_codelens::store::CodeIndex;
/// use argus_codelens::embedding::EmbeddingClient;
///
/// let index = CodeIndex::in_memory().unwrap();
/// let client = EmbeddingClient::new("key");
/// let search = HybridSearch::new(index, client);
/// ```
pub struct HybridSearch {
    index: CodeIndex,
    embedding_client: EmbeddingClient,
}

impl HybridSearch {
    /// Create a new hybrid search engine.
    pub fn new(index: CodeIndex, embedding_client: EmbeddingClient) -> Self {
        Self {
            index,
            embedding_client,
        }
    }

    /// Access the underlying index.
    pub fn index(&self) -> &CodeIndex {
        &self.index
    }

    /// Search using hybrid retrieval (vector + keyword + RRF fusion).
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError`] if embedding or database queries fail.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use argus_codelens::search::HybridSearch;
    /// use argus_codelens::store::CodeIndex;
    /// use argus_codelens::embedding::EmbeddingClient;
    ///
    /// # async fn example() {
    /// let index = CodeIndex::in_memory().unwrap();
    /// let client = EmbeddingClient::new("key");
    /// let search = HybridSearch::new(index, client);
    /// let results = search.search("authentication", 10).await.unwrap();
    /// # }
    /// ```
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, ArgusError> {
        let fetch_count = limit * 2;

        // Run vector search
        let query_embedding = self.embedding_client.embed_query(query).await?;
        let vector_results = self.index.vector_search(&query_embedding, fetch_count)?;

        // Run keyword search
        let keyword_results = self.index.keyword_search(query, fetch_count)?;

        // Fuse results with RRF
        let fused = reciprocal_rank_fusion(&vector_results, &keyword_results, 60);

        // Take top `limit` and convert to SearchResult
        let results: Vec<SearchResult> = fused
            .into_iter()
            .take(limit)
            .map(|item| SearchResult {
                file_path: item.chunk.file_path,
                line_start: item.chunk.start_line,
                line_end: item.chunk.end_line,
                snippet: item.chunk.content,
                score: item.score,
                language: Some(item.chunk.language),
            })
            .collect();

        Ok(results)
    }

    /// Index a repository (chunk + embed + store).
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError`] if chunking, embedding, or storage fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::Path;
    /// use argus_codelens::search::HybridSearch;
    /// use argus_codelens::store::CodeIndex;
    /// use argus_codelens::embedding::EmbeddingClient;
    ///
    /// # async fn example() {
    /// let index = CodeIndex::open(Path::new(".argus/index.db")).unwrap();
    /// let client = EmbeddingClient::new("key");
    /// let search = HybridSearch::new(index, client);
    /// let stats = search.index_repo(Path::new(".")).await.unwrap();
    /// println!("Indexed {} chunks from {} files", stats.total_chunks, stats.total_files);
    /// # }
    /// ```
    pub async fn index_repo(&self, root: &Path) -> Result<IndexStats, ArgusError> {
        let files = argus_repomap::walker::walk_repo(root)?;
        let mut all_chunks = Vec::new();

        for file in &files {
            let chunks = chunk_file(&file.path, &file.content, file.language)?;
            let file_hash = compute_file_hash(&file.content);
            self.index.record_file(&file.path, &file_hash)?;
            all_chunks.extend(chunks);
        }

        if all_chunks.is_empty() {
            return self.index.stats();
        }

        // Build texts for embedding (context_header + content)
        let texts: Vec<String> = all_chunks
            .iter()
            .map(|c| format!("{}\n\n{}", c.context_header, c.content))
            .collect();

        // Embed in batches
        let embeddings = self.embedding_client.embed_batch(&texts).await?;

        // Store chunks with embeddings
        let pairs: Vec<(CodeChunk, Vec<f32>)> = all_chunks.into_iter().zip(embeddings).collect();

        self.index.insert_chunks(&pairs)?;

        self.index.stats()
    }

    /// Incremental re-index (only changed files).
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError`] if chunking, embedding, or storage fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::path::Path;
    /// use argus_codelens::search::HybridSearch;
    /// use argus_codelens::store::CodeIndex;
    /// use argus_codelens::embedding::EmbeddingClient;
    ///
    /// # async fn example() {
    /// let index = CodeIndex::open(Path::new(".argus/index.db")).unwrap();
    /// let client = EmbeddingClient::new("key");
    /// let search = HybridSearch::new(index, client);
    /// let stats = search.reindex_repo(Path::new(".")).await.unwrap();
    /// # }
    /// ```
    pub async fn reindex_repo(&self, root: &Path) -> Result<IndexStats, ArgusError> {
        let files = argus_repomap::walker::walk_repo(root)?;
        let existing_paths = self.index.indexed_files()?;

        // Track which files are still present
        let mut current_paths = std::collections::HashSet::new();
        let mut changed_files = Vec::new();

        for file in &files {
            let path_str = file.path.to_string_lossy().to_string();
            current_paths.insert(path_str.clone());

            let file_hash = compute_file_hash(&file.content);
            let stored_hash = self.index.file_hash(&file.path)?;

            if stored_hash.as_deref() != Some(&file_hash) {
                // File is new or changed
                self.index.remove_file(&file.path)?;
                changed_files.push(file);
                self.index.record_file(&file.path, &file_hash)?;
            }
        }

        // Remove files that no longer exist
        for path in &existing_paths {
            if !current_paths.contains(path) {
                self.index.remove_file(Path::new(path))?;
            }
        }

        if changed_files.is_empty() {
            return self.index.stats();
        }

        // Chunk changed files
        let mut all_chunks = Vec::new();
        for file in &changed_files {
            let chunks = chunk_file(&file.path, &file.content, file.language)?;
            all_chunks.extend(chunks);
        }

        if all_chunks.is_empty() {
            return self.index.stats();
        }

        // Embed
        let texts: Vec<String> = all_chunks
            .iter()
            .map(|c| format!("{}\n\n{}", c.context_header, c.content))
            .collect();

        let embeddings = self.embedding_client.embed_batch(&texts).await?;

        // Store
        let pairs: Vec<(CodeChunk, Vec<f32>)> = all_chunks.into_iter().zip(embeddings).collect();

        self.index.insert_chunks(&pairs)?;

        self.index.stats()
    }
}

/// RRF result with combined score and chunk data.
pub struct RrfResult {
    /// The code chunk.
    pub chunk: CodeChunk,
    /// RRF combined score.
    pub score: f64,
}

/// Combine vector and keyword search results using Reciprocal Rank Fusion.
///
/// # Examples
///
/// ```
/// use argus_codelens::search::reciprocal_rank_fusion;
///
/// // Empty inputs produce empty output
/// let results = reciprocal_rank_fusion(&[], &[], 60);
/// assert!(results.is_empty());
/// ```
pub fn reciprocal_rank_fusion(
    vector_results: &[SearchHit],
    keyword_results: &[SearchHit],
    k: usize,
) -> Vec<RrfResult> {
    let mut scores: HashMap<String, f64> = HashMap::new();
    let mut chunks: HashMap<String, CodeChunk> = HashMap::new();

    for (rank, hit) in vector_results.iter().enumerate() {
        let hash = &hit.chunk.content_hash;
        *scores.entry(hash.clone()).or_default() += 1.0 / (k as f64 + rank as f64 + 1.0);
        chunks
            .entry(hash.clone())
            .or_insert_with(|| hit.chunk.clone());
    }

    for (rank, hit) in keyword_results.iter().enumerate() {
        let hash = &hit.chunk.content_hash;
        *scores.entry(hash.clone()).or_default() += 1.0 / (k as f64 + rank as f64 + 1.0);
        chunks
            .entry(hash.clone())
            .or_insert_with(|| hit.chunk.clone());
    }

    let mut results: Vec<RrfResult> = scores
        .into_iter()
        .filter_map(|(hash, score)| chunks.remove(&hash).map(|chunk| RrfResult { chunk, score }))
        .collect();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    results
}

fn compute_file_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::SearchSource;
    use std::path::PathBuf;

    fn make_hit(name: &str, hash: &str, source: SearchSource) -> SearchHit {
        SearchHit {
            chunk: CodeChunk {
                file_path: PathBuf::from("test.rs"),
                start_line: 1,
                end_line: 5,
                entity_name: name.into(),
                entity_type: "function".into(),
                language: "rust".into(),
                content: format!("fn {name}() {{}}"),
                context_header: format!("# Name: {name}"),
                content_hash: hash.into(),
            },
            score: 0.9,
            source,
        }
    }

    #[test]
    fn rrf_combines_results_correctly() {
        let vector = vec![
            make_hit("auth", "hash_auth", SearchSource::Vector),
            make_hit("parse", "hash_parse", SearchSource::Vector),
        ];
        let keyword = vec![
            make_hit("parse", "hash_parse", SearchSource::Keyword),
            make_hit("log", "hash_log", SearchSource::Keyword),
        ];

        let fused = reciprocal_rank_fusion(&vector, &keyword, 60);

        assert_eq!(fused.len(), 3);
        // "parse" appears in both, should rank highest
        assert_eq!(fused[0].chunk.entity_name, "parse");
    }

    #[test]
    fn rrf_result_in_both_ranks_higher() {
        let vector = vec![
            make_hit("unique_v", "hash_v", SearchSource::Vector),
            make_hit("shared", "hash_shared", SearchSource::Vector),
        ];
        let keyword = vec![
            make_hit("unique_k", "hash_k", SearchSource::Keyword),
            make_hit("shared", "hash_shared", SearchSource::Keyword),
        ];

        let fused = reciprocal_rank_fusion(&vector, &keyword, 60);

        // "shared" has score from both lists, should be highest
        let shared = fused
            .iter()
            .find(|r| r.chunk.entity_name == "shared")
            .unwrap();
        let unique_v = fused
            .iter()
            .find(|r| r.chunk.entity_name == "unique_v")
            .unwrap();
        assert!(shared.score > unique_v.score);
    }

    #[test]
    fn rrf_empty_inputs() {
        let fused = reciprocal_rank_fusion(&[], &[], 60);
        assert!(fused.is_empty());
    }
}
