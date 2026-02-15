use std::time::Duration;

use argus_core::{ArgusError, LlmConfig};
use serde::{Deserialize, Serialize};

/// A message in a chat conversation with the LLM.
///
/// # Examples
///
/// ```
/// use argus_review::llm::{ChatMessage, Role};
///
/// let msg = ChatMessage {
///     role: Role::User,
///     content: "Review this code".into(),
/// };
/// assert!(matches!(msg.role, Role::User));
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    /// Role of the message sender.
    pub role: Role,
    /// Text content of the message.
    pub content: String,
}

/// Role in the chat conversation.
///
/// # Examples
///
/// ```
/// use argus_review::llm::Role;
///
/// let role = Role::System;
/// assert_eq!(serde_json::to_string(&role).unwrap(), "\"system\"");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// System-level instructions.
    System,
    /// User input.
    User,
    /// Assistant response.
    Assistant,
}

/// OpenAI-compatible chat completions client.
///
/// Works with any provider that exposes the `/v1/chat/completions` endpoint:
/// OpenAI, Ollama, vLLM, LiteLLM, etc.
///
/// # Examples
///
/// ```
/// use argus_core::LlmConfig;
/// use argus_review::llm::LlmClient;
///
/// let config = LlmConfig {
///     api_key: Some("test-key".into()),
///     ..LlmConfig::default()
/// };
/// let client = LlmClient::new(&config).unwrap();
/// ```
pub struct LlmClient {
    client: reqwest::Client,
    config: LlmConfig,
}

impl LlmClient {
    /// Create a new LLM client from configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Llm`] if the HTTP client cannot be built.
    ///
    /// # Examples
    ///
    /// ```
    /// use argus_core::LlmConfig;
    /// use argus_review::llm::LlmClient;
    ///
    /// let client = LlmClient::new(&LlmConfig::default()).unwrap();
    /// ```
    pub fn new(config: &LlmConfig) -> Result<Self, ArgusError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| ArgusError::Llm(format!("failed to create HTTP client: {e}")))?;
        Ok(Self {
            client,
            config: config.clone(),
        })
    }

    /// Return the model name from the configuration.
    pub fn model(&self) -> &str {
        &self.config.model
    }

    /// Send a chat completion request and return the text response.
    ///
    /// Builds a request to `{base_url}/v1/chat/completions` with the given
    /// messages, temperature 0.1, and JSON response format.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Llm`] on HTTP errors or response parsing failures.
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String, ArgusError> {
        let base_url = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com");
        let url = format!("{base_url}/v1/chat/completions");

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "temperature": 0.1,
            "response_format": { "type": "json_object" },
        });

        let mut request = self.client.post(&url);
        if let Some(api_key) = &self.config.api_key {
            request = request.header("Authorization", format!("Bearer {api_key}"));
        }
        request = request.header("Content-Type", "application/json");

        let response = request
            .json(&body)
            .send()
            .await
            .map_err(|e| ArgusError::Llm(format!("request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(ArgusError::Llm(format!(
                "LLM API error {status}: {body_text}"
            )));
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ArgusError::Llm(format!("failed to parse response: {e}")))?;

        let content = response_body
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                ArgusError::Llm(format!("unexpected response structure: {response_body}"))
            })?;

        Ok(content.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_core::LlmConfig;

    #[test]
    fn client_construction_succeeds() {
        let config = LlmConfig::default();
        let client = LlmClient::new(&config);
        assert!(client.is_ok());
    }

    #[test]
    fn model_returns_config_model() {
        let config = LlmConfig {
            model: "gpt-4o-mini".into(),
            ..LlmConfig::default()
        };
        let client = LlmClient::new(&config).unwrap();
        assert_eq!(client.model(), "gpt-4o-mini");
    }

    #[test]
    fn chat_message_serializes() {
        let msg = ChatMessage {
            role: Role::System,
            content: "hello".into(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "system");
        assert_eq!(json["content"], "hello");
    }
}
