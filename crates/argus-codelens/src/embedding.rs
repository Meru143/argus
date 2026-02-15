//! Multi-provider embedding client for code chunks.
//!
//! Supports Voyage, Gemini, and OpenAI embedding APIs. The provider is
//! selected via [`EmbeddingConfig`]. Same interface, different API calls.

use argus_core::{ArgusError, EmbeddingConfig};
use serde::{Deserialize, Serialize};

/// Embedding provider variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provider {
    Voyage,
    Gemini,
    OpenAi,
}

/// Client for embedding code via Voyage, Gemini, or OpenAI APIs.
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
    model: String,
    provider: Provider,
}

impl std::fmt::Debug for EmbeddingClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingClient")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

// --- Provider constants ---

const VOYAGE_BATCH_SIZE: usize = 64;
const VOYAGE_DELAY_MS: u64 = 200;

const GEMINI_BATCH_SIZE: usize = 100;
const GEMINI_DELAY_MS: u64 = 100;

const OPENAI_BATCH_SIZE: usize = 64;
const OPENAI_DELAY_MS: u64 = 200;

// --- Voyage request/response ---

#[derive(Serialize)]
struct VoyageRequest {
    model: String,
    input: Vec<String>,
    input_type: String,
}

#[derive(Deserialize)]
struct VoyageResponse {
    data: Vec<VoyageDataItem>,
}

#[derive(Deserialize)]
struct VoyageDataItem {
    embedding: Vec<f32>,
}

// --- Gemini request/response ---

#[derive(Serialize)]
struct GeminiBatchRequest {
    requests: Vec<GeminiEmbedRequest>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiEmbedRequest {
    model: String,
    content: GeminiContent,
    task_type: String,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Deserialize)]
struct GeminiBatchResponse {
    embeddings: Vec<GeminiEmbedding>,
}

#[derive(Deserialize)]
struct GeminiEmbedding {
    values: Vec<f32>,
}

// --- OpenAI request/response ---

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    data: Vec<OpenAiDataItem>,
}

#[derive(Deserialize)]
struct OpenAiDataItem {
    embedding: Vec<f32>,
}

impl EmbeddingClient {
    /// Create a new Voyage client with the given API key.
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
            model: "voyage-code-3".to_string(),
            provider: Provider::Voyage,
        }
    }

    /// Create a client from an [`EmbeddingConfig`].
    ///
    /// The provider is determined by `config.provider`. Falls back to
    /// provider-specific env vars if no API key is in config:
    /// - `"voyage"` -> `VOYAGE_API_KEY`
    /// - `"gemini"` -> `GEMINI_API_KEY`
    /// - `"openai"` -> `OPENAI_API_KEY`
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Config`] if no API key is available or
    /// the provider is unknown.
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
        let provider = match config.provider.as_str() {
            "voyage" => Provider::Voyage,
            "gemini" => Provider::Gemini,
            "openai" => Provider::OpenAi,
            other => {
                return Err(ArgusError::Config(format!(
                    "unknown embedding provider: {other}. Supported: voyage, gemini, openai"
                )));
            }
        };

        let env_var = match provider {
            Provider::Voyage => "VOYAGE_API_KEY",
            Provider::Gemini => "GEMINI_API_KEY",
            Provider::OpenAi => "OPENAI_API_KEY",
        };

        let api_key = config
            .api_key
            .clone()
            .or_else(|| std::env::var(env_var).ok())
            .ok_or_else(|| {
                ArgusError::Config(format!(
                    "embedding API key not found: set embedding.api_key in .argus.toml or {env_var} env var"
                ))
            })?;

        let model = if !is_model_compatible(&config.model, provider) {
            let provider_default = default_model(provider);
            eprintln!(
                "warning: model '{}' is not compatible with {} provider, switching to '{}'",
                config.model,
                config.provider,
                provider_default,
            );
            provider_default.to_string()
        } else {
            config.model.clone()
        };

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            provider,
        })
    }

    /// Get the model name.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Get the provider name.
    pub fn provider_name(&self) -> &str {
        match self.provider {
            Provider::Voyage => "voyage",
            Provider::Gemini => "gemini",
            Provider::OpenAi => "openai",
        }
    }

    /// Default embedding dimensions for this client's provider.
    pub fn default_dimensions(&self) -> usize {
        default_dimensions(self.provider)
    }

    /// Embed a batch of texts. Returns vectors in the same order.
    ///
    /// Splits into sub-batches with rate-limiting delays between batches.
    /// Batch sizes vary by provider: Voyage/OpenAI=64, Gemini=100.
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

        let (batch_size, delay_ms) = match self.provider {
            Provider::Voyage => (VOYAGE_BATCH_SIZE, VOYAGE_DELAY_MS),
            Provider::Gemini => (GEMINI_BATCH_SIZE, GEMINI_DELAY_MS),
            Provider::OpenAi => (OPENAI_BATCH_SIZE, OPENAI_DELAY_MS),
        };

        let mut all_embeddings = Vec::with_capacity(texts.len());

        for (i, batch) in texts.chunks(batch_size).enumerate() {
            if i > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
            }

            let batch_result = match self.provider {
                Provider::Voyage => self.embed_batch_voyage(batch, "document").await?,
                Provider::Gemini => {
                    self.embed_batch_gemini(batch, "RETRIEVAL_DOCUMENT")
                        .await?
                }
                Provider::OpenAi => self.embed_batch_openai(batch).await?,
            };

            all_embeddings.extend(batch_result);
        }

        Ok(all_embeddings)
    }

    /// Embed a single query text.
    ///
    /// Uses query-specific task types where supported:
    /// - Voyage: `input_type: "query"`
    /// - Gemini: `taskType: "RETRIEVAL_QUERY"`
    /// - OpenAI: same as document embedding
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
        let result = match self.provider {
            Provider::Voyage => {
                self.embed_batch_voyage(&[query.to_string()], "query")
                    .await?
            }
            Provider::Gemini => {
                self.embed_batch_gemini(&[query.to_string()], "RETRIEVAL_QUERY")
                    .await?
            }
            Provider::OpenAi => self.embed_batch_openai(&[query.to_string()]).await?,
        };

        result
            .into_iter()
            .next()
            .ok_or_else(|| ArgusError::Embedding("empty response from embedding API".into()))
    }

    // --- Voyage ---

    async fn embed_batch_voyage(
        &self,
        texts: &[String],
        input_type: &str,
    ) -> Result<Vec<Vec<f32>>, ArgusError> {
        let request = VoyageRequest {
            model: self.model.clone(),
            input: texts.to_vec(),
            input_type: input_type.to_string(),
        };

        let response = self
            .client
            .post("https://api.voyageai.com/v1/embeddings")
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

        let parsed: VoyageResponse = response
            .json()
            .await
            .map_err(|e| ArgusError::Embedding(format!("failed to parse response: {e}")))?;

        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }

    // --- Gemini ---

    async fn embed_batch_gemini(
        &self,
        texts: &[String],
        task_type: &str,
    ) -> Result<Vec<Vec<f32>>, ArgusError> {
        let requests: Vec<GeminiEmbedRequest> = texts
            .iter()
            .map(|text| GeminiEmbedRequest {
                model: format!("models/{}", self.model),
                content: GeminiContent {
                    parts: vec![GeminiPart {
                        text: text.clone(),
                    }],
                },
                task_type: task_type.to_string(),
            })
            .collect();

        let request = GeminiBatchRequest { requests };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:batchEmbedContents?key={}",
            self.model, self.api_key,
        );

        let response = self
            .client
            .post(&url)
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
                "Gemini API returned {status}: {body}"
            )));
        }

        let parsed: GeminiBatchResponse = response
            .json()
            .await
            .map_err(|e| ArgusError::Embedding(format!("failed to parse response: {e}")))?;

        Ok(parsed.embeddings.into_iter().map(|e| e.values).collect())
    }

    // --- OpenAI ---

    async fn embed_batch_openai(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ArgusError> {
        let request = OpenAiRequest {
            model: self.model.clone(),
            input: texts.to_vec(),
        };

        let response = self
            .client
            .post("https://api.openai.com/v1/embeddings")
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
                "OpenAI API returned {status}: {body}"
            )));
        }

        let parsed: OpenAiResponse = response
            .json()
            .await
            .map_err(|e| ArgusError::Embedding(format!("failed to parse response: {e}")))?;

        Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
    }
}

fn default_model(provider: Provider) -> &'static str {
    match provider {
        Provider::Voyage => "voyage-code-3",
        Provider::Gemini => "text-embedding-004",
        Provider::OpenAi => "text-embedding-3-small",
    }
}

fn default_dimensions(provider: Provider) -> usize {
    match provider {
        Provider::Voyage => 1024,
        Provider::Gemini => 768,
        Provider::OpenAi => 1536,
    }
}

/// Check if a model name is compatible with the given provider.
///
/// Heuristic: Voyage models start with "voyage", Gemini and OpenAI models
/// contain "embedding".
fn is_model_compatible(model: &str, provider: Provider) -> bool {
    match provider {
        Provider::Voyage => model.starts_with("voyage"),
        Provider::Gemini => model.contains("embedding"),
        Provider::OpenAi => model.contains("embedding"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Voyage tests ---

    #[test]
    fn voyage_request_format_is_correct() {
        let client = EmbeddingClient::new("test-key");
        let texts = vec!["fn main() {}".to_string(), "struct Foo {}".to_string()];
        let request = VoyageRequest {
            model: client.model.clone(),
            input: texts,
            input_type: "document".to_string(),
        };

        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["model"], "voyage-code-3");
        assert_eq!(json["input_type"], "document");
        assert_eq!(json["input"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn voyage_response_parsing_works() {
        let json = r#"{
            "data": [
                {"embedding": [0.1, 0.2, 0.3]},
                {"embedding": [0.4, 0.5, 0.6]}
            ]
        }"#;
        let response: VoyageResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].embedding, vec![0.1, 0.2, 0.3]);
        assert_eq!(response.data[1].embedding, vec![0.4, 0.5, 0.6]);
    }

    #[test]
    fn voyage_query_request_uses_query_input_type() {
        let request = VoyageRequest {
            model: "voyage-code-3".into(),
            input: vec!["auth logic".to_string()],
            input_type: "query".to_string(),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["input_type"], "query");
    }

    #[test]
    fn voyage_batch_splitting() {
        let n = 150;
        let texts: Vec<String> = (0..n).map(|i| format!("text {i}")).collect();
        let batches: Vec<&[String]> = texts.chunks(VOYAGE_BATCH_SIZE).collect();
        assert_eq!(batches.len(), 3); // 64 + 64 + 22
        assert_eq!(batches[0].len(), 64);
        assert_eq!(batches[1].len(), 64);
        assert_eq!(batches[2].len(), 22);
    }

    // --- Gemini tests ---

    #[test]
    fn gemini_request_format_is_correct() {
        let request = GeminiBatchRequest {
            requests: vec![GeminiEmbedRequest {
                model: "models/text-embedding-004".into(),
                content: GeminiContent {
                    parts: vec![GeminiPart {
                        text: "fn main() {}".into(),
                    }],
                },
                task_type: "RETRIEVAL_DOCUMENT".into(),
            }],
        };

        let json = serde_json::to_value(&request).unwrap();
        let req = &json["requests"][0];
        assert_eq!(req["model"], "models/text-embedding-004");
        assert_eq!(req["content"]["parts"][0]["text"], "fn main() {}");
        assert_eq!(req["taskType"], "RETRIEVAL_DOCUMENT");
    }

    #[test]
    fn gemini_response_parsing_works() {
        let json = r#"{
            "embeddings": [
                {"values": [0.1, 0.2, 0.3]},
                {"values": [0.4, 0.5, 0.6]}
            ]
        }"#;
        let response: GeminiBatchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.embeddings.len(), 2);
        assert_eq!(response.embeddings[0].values, vec![0.1, 0.2, 0.3]);
        assert_eq!(response.embeddings[1].values, vec![0.4, 0.5, 0.6]);
    }

    #[test]
    fn gemini_query_uses_retrieval_query() {
        let request = GeminiEmbedRequest {
            model: "models/text-embedding-004".into(),
            content: GeminiContent {
                parts: vec![GeminiPart {
                    text: "search query".into(),
                }],
            },
            task_type: "RETRIEVAL_QUERY".into(),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["taskType"], "RETRIEVAL_QUERY");
    }

    #[test]
    fn gemini_batch_splitting() {
        let n = 250;
        let texts: Vec<String> = (0..n).map(|i| format!("text {i}")).collect();
        let batches: Vec<&[String]> = texts.chunks(GEMINI_BATCH_SIZE).collect();
        assert_eq!(batches.len(), 3); // 100 + 100 + 50
        assert_eq!(batches[0].len(), 100);
        assert_eq!(batches[1].len(), 100);
        assert_eq!(batches[2].len(), 50);
    }

    // --- OpenAI tests ---

    #[test]
    fn openai_request_format_is_correct() {
        let request = OpenAiRequest {
            model: "text-embedding-3-small".into(),
            input: vec!["fn main() {}".into(), "struct Foo {}".into()],
        };

        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["model"], "text-embedding-3-small");
        assert_eq!(json["input"].as_array().unwrap().len(), 2);
        // OpenAI does not use input_type field
        assert!(json.get("input_type").is_none());
    }

    #[test]
    fn openai_response_parsing_works() {
        let json = r#"{
            "data": [
                {"embedding": [0.1, 0.2, 0.3]},
                {"embedding": [0.4, 0.5, 0.6]}
            ]
        }"#;
        let response: OpenAiResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].embedding, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn openai_batch_splitting() {
        let n = 150;
        let texts: Vec<String> = (0..n).map(|i| format!("text {i}")).collect();
        let batches: Vec<&[String]> = texts.chunks(OPENAI_BATCH_SIZE).collect();
        assert_eq!(batches.len(), 3);
    }

    // --- Config / env var tests ---

    #[test]
    fn missing_api_key_gives_clear_error() {
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
        assert!(
            err.contains("VOYAGE_API_KEY"),
            "error should mention env var: {err}"
        );
    }

    #[test]
    fn gemini_env_var_fallback() {
        std::env::remove_var("GEMINI_API_KEY");
        let config = EmbeddingConfig {
            provider: "gemini".into(),
            api_key: None,
            model: "text-embedding-004".into(),
            dimensions: 768,
        };
        let result = EmbeddingClient::with_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("GEMINI_API_KEY"));
    }

    #[test]
    fn openai_env_var_fallback() {
        std::env::remove_var("OPENAI_API_KEY");
        let config = EmbeddingConfig {
            provider: "openai".into(),
            api_key: None,
            model: "text-embedding-3-small".into(),
            dimensions: 1536,
        };
        let result = EmbeddingClient::with_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("OPENAI_API_KEY"));
    }

    #[test]
    fn gemini_config_uses_default_model() {
        let config = EmbeddingConfig {
            provider: "gemini".into(),
            api_key: Some("test-key".into()),
            ..EmbeddingConfig::default() // model defaults to voyage-code-3
        };
        let client = EmbeddingClient::with_config(&config).unwrap();
        assert_eq!(client.model(), "text-embedding-004");
        assert_eq!(client.default_dimensions(), 768);
    }

    #[test]
    fn openai_config_uses_default_model() {
        let config = EmbeddingConfig {
            provider: "openai".into(),
            api_key: Some("test-key".into()),
            ..EmbeddingConfig::default()
        };
        let client = EmbeddingClient::with_config(&config).unwrap();
        assert_eq!(client.model(), "text-embedding-3-small");
        assert_eq!(client.default_dimensions(), 1536);
    }

    #[test]
    fn voyage_config_keeps_default_model() {
        let config = EmbeddingConfig {
            provider: "voyage".into(),
            api_key: Some("test-key".into()),
            ..EmbeddingConfig::default()
        };
        let client = EmbeddingClient::with_config(&config).unwrap();
        assert_eq!(client.model(), "voyage-code-3");
        assert_eq!(client.default_dimensions(), 1024);
    }

    #[test]
    fn custom_model_is_preserved() {
        let config = EmbeddingConfig {
            provider: "openai".into(),
            api_key: Some("test-key".into()),
            model: "text-embedding-3-large".into(),
            dimensions: 3072,
        };
        let client = EmbeddingClient::with_config(&config).unwrap();
        assert_eq!(client.model(), "text-embedding-3-large");
    }

    #[test]
    fn unknown_provider_returns_error() {
        let config = EmbeddingConfig {
            provider: "cohere".into(),
            api_key: Some("key".into()),
            ..EmbeddingConfig::default()
        };
        let result = EmbeddingClient::with_config(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cohere"));
    }

    #[test]
    fn gemini_model_auto_corrects_when_switching_to_voyage() {
        // Bug 1: switching Gemini→Voyage would send "text-embedding-004" to Voyage API
        let config = EmbeddingConfig {
            provider: "voyage".into(),
            api_key: Some("test-key".into()),
            model: "text-embedding-004".into(),
            dimensions: 768,
        };
        let client = EmbeddingClient::with_config(&config).unwrap();
        assert_eq!(client.model(), "voyage-code-3");
    }

    #[test]
    fn openai_model_auto_corrects_when_switching_to_voyage() {
        let config = EmbeddingConfig {
            provider: "voyage".into(),
            api_key: Some("test-key".into()),
            model: "text-embedding-3-small".into(),
            dimensions: 1536,
        };
        let client = EmbeddingClient::with_config(&config).unwrap();
        assert_eq!(client.model(), "voyage-code-3");
    }

    #[test]
    fn model_compatibility_check() {
        assert!(is_model_compatible("voyage-code-3", Provider::Voyage));
        assert!(is_model_compatible("voyage-3", Provider::Voyage));
        assert!(!is_model_compatible("text-embedding-004", Provider::Voyage));

        assert!(is_model_compatible("text-embedding-004", Provider::Gemini));
        assert!(!is_model_compatible("voyage-code-3", Provider::Gemini));

        assert!(is_model_compatible("text-embedding-3-small", Provider::OpenAi));
        assert!(!is_model_compatible("voyage-code-3", Provider::OpenAi));
    }
}
