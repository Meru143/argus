//! Voyage Code 3 API client for embedding code chunks.
//!
//! Provides batch and single-query embedding via the Voyage AI API.
//! Uses `input_type: "document"` for indexing and `input_type: "query"` for searching.

use argus_core::{ArgusError, EmbeddingConfig};
use serde::{Deserialize, Serialize};

/// Client for the Voyage Code 3 embedding API.
///
/// # Examples
///
/// ```
/// use argus_codelens::embedding::EmbeddingClient;
///
/// let client = EmbeddingClient::new("test-key");
/// assert_eq!(client.model(), "voyage-code-3");
/// ```
pub struct EmbeddingClient {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl std::fmt::Debug for EmbeddingClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingClient")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

const DEFAULT_BASE_URL: &str = "https://api.voyageai.com/v1";
const DEFAULT_MODEL: &str = "voyage-code-3";
const BATCH_SIZE: usize = 64;
const BATCH_DELAY_MS: u64 = 200;

#[derive(Serialize)]
struct EmbedRequest {
    model: String,
    input: Vec<String>,
    input_type: String,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedDataItem>,
}

#[derive(Deserialize)]
struct EmbedDataItem {
    embedding: Vec<f32>,
}

impl EmbeddingClient {
    /// Create a new client with the given API key.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_codelens::embedding::EmbeddingClient;
    ///
    /// let client = EmbeddingClient::new("my-key");
    /// ```
    pub fn new(api_key: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
        }
    }

    /// Create a client from an [`EmbeddingConfig`].
    ///
    /// Falls back to `VOYAGE_API_KEY` env var if no key in config.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Config`] if no API key is available.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use argus_core::EmbeddingConfig;
    /// use argus_codelens::embedding::EmbeddingClient;
    ///
    /// let config = EmbeddingConfig::default();
    /// let client = EmbeddingClient::with_config(&config).unwrap();
    /// ```
    pub fn with_config(config: &EmbeddingConfig) -> Result<Self, ArgusError> {
        let api_key = config
            .api_key
            .clone()
            .or_else(|| std::env::var("VOYAGE_API_KEY").ok())
            .ok_or_else(|| {
                ArgusError::Config(
                    "embedding API key not found: set embedding.api_key in .argus.toml or VOYAGE_API_KEY env var".into(),
                )
            })?;

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            base_url: DEFAULT_BASE_URL.to_string(),
            model: config.model.clone(),
        })
    }

    /// Get the model name.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Embed a batch of texts. Returns vectors in the same order.
    ///
    /// Voyage supports up to 128 texts per batch, max 320K tokens total.
    /// This method splits into sub-batches of 64 with 200ms delays for rate limiting.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Embedding`] if the API call fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use argus_codelens::embedding::EmbeddingClient;
    ///
    /// # async fn example() {
    /// let client = EmbeddingClient::new("key");
    /// let texts = vec!["fn main() {}".to_string()];
    /// let embeddings = client.embed_batch(&texts).await.unwrap();
    /// assert_eq!(embeddings.len(), 1);
    /// # }
    /// ```
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ArgusError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_embeddings = Vec::with_capacity(texts.len());

        for (i, batch) in texts.chunks(BATCH_SIZE).enumerate() {
            if i > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(BATCH_DELAY_MS)).await;
            }

            let request = EmbedRequest {
                model: self.model.clone(),
                input: batch.to_vec(),
                input_type: "document".to_string(),
            };

            let response = self
                .client
                .post(format!("{}/embeddings", self.base_url))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&request)
                .send()
                .await
                .map_err(|e| ArgusError::Embedding(format!("HTTP request failed: {e}")))?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "unable to read response body".into());
                return Err(ArgusError::Embedding(format!(
                    "Voyage API returned {status}: {body}"
                )));
            }

            let embed_response: EmbedResponse = response
                .json()
                .await
                .map_err(|e| ArgusError::Embedding(format!("failed to parse response: {e}")))?;

            for item in embed_response.data {
                all_embeddings.push(item.embedding);
            }
        }

        Ok(all_embeddings)
    }

    /// Embed a single query (uses `input_type: "query"`).
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Embedding`] if the API call fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use argus_codelens::embedding::EmbeddingClient;
    ///
    /// # async fn example() {
    /// let client = EmbeddingClient::new("key");
    /// let embedding = client.embed_query("authentication logic").await.unwrap();
    /// assert!(!embedding.is_empty());
    /// # }
    /// ```
    pub async fn embed_query(&self, query: &str) -> Result<Vec<f32>, ArgusError> {
        let request = EmbedRequest {
            model: self.model.clone(),
            input: vec![query.to_string()],
            input_type: "query".to_string(),
        };

        let response = self
            .client
            .post(format!("{}/embeddings", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| ArgusError::Embedding(format!("HTTP request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read response body".into());
            return Err(ArgusError::Embedding(format!(
                "Voyage API returned {status}: {body}"
            )));
        }

        let embed_response: EmbedResponse = response
            .json()
            .await
            .map_err(|e| ArgusError::Embedding(format!("failed to parse response: {e}")))?;

        let first = embed_response
            .data
            .into_iter()
            .next()
            .ok_or_else(|| ArgusError::Embedding("empty response from Voyage API".into()))?;

        Ok(first.embedding)
    }

    /// Build the JSON request body for a batch embed call (for testing).
    #[cfg(test)]
    fn build_request(&self, texts: &[String], input_type: &str) -> EmbedRequest {
        EmbedRequest {
            model: self.model.clone(),
            input: texts.to_vec(),
            input_type: input_type.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_format_is_correct() {
        let client = EmbeddingClient::new("test-key");
        let texts = vec!["fn main() {}".to_string(), "struct Foo {}".to_string()];
        let request = client.build_request(&texts, "document");

        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["model"], "voyage-code-3");
        assert_eq!(json["input_type"], "document");
        assert_eq!(json["input"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn response_parsing_works() {
        let json = r#"{
            "data": [
                {"embedding": [0.1, 0.2, 0.3]},
                {"embedding": [0.4, 0.5, 0.6]}
            ]
        }"#;
        let response: EmbedResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].embedding, vec![0.1, 0.2, 0.3]);
        assert_eq!(response.data[1].embedding, vec![0.4, 0.5, 0.6]);
    }

    #[test]
    fn batch_splitting_calculates_correctly() {
        // Verify that texts would split into correct number of batches
        let n = 150;
        let texts: Vec<String> = (0..n).map(|i| format!("text {i}")).collect();
        let batches: Vec<&[String]> = texts.chunks(BATCH_SIZE).collect();
        assert_eq!(batches.len(), 3); // 64 + 64 + 22
        assert_eq!(batches[0].len(), 64);
        assert_eq!(batches[1].len(), 64);
        assert_eq!(batches[2].len(), 22);
    }

    #[test]
    fn missing_api_key_gives_clear_error() {
        // Remove env var if set
        std::env::remove_var("VOYAGE_API_KEY");
        let config = EmbeddingConfig {
            api_key: None,
            ..EmbeddingConfig::default()
        };
        let result = EmbeddingClient::with_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("API key"),
            "error should mention API key: {err}"
        );
    }

    #[test]
    fn query_request_uses_query_input_type() {
        let client = EmbeddingClient::new("test-key");
        let request = client.build_request(&["auth logic".to_string()], "query");
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["input_type"], "query");
    }
}
