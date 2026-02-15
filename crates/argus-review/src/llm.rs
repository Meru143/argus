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

/// Supported LLM API providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provider {
    OpenAi,
    Anthropic,
    Gemini,
}

/// Multi-provider LLM chat client.
///
/// Supports OpenAI-compatible (`/v1/chat/completions`), Anthropic
/// (`/v1/messages`), and Gemini (`generateContent`) endpoints. The
/// provider is determined by `LlmConfig.provider`.
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
    provider: Provider,
    api_key: Option<String>,
    model: String,
    base_url: Option<String>,
}

impl std::fmt::Debug for LlmClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmClient")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl LlmClient {
    /// Create a new LLM client from configuration.
    ///
    /// Resolves the API key from config, falling back to provider-specific
    /// env vars (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or `GEMINI_API_KEY`).
    /// When the provider changes but the model is still a default from another
    /// provider, auto-switches to the current provider's default model.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Llm`] if the provider is unknown or the HTTP
    /// client cannot be built.
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
        let provider = match config.provider.as_str() {
            "openai" => Provider::OpenAi,
            "anthropic" => Provider::Anthropic,
            "gemini" => Provider::Gemini,
            other => {
                return Err(ArgusError::Llm(format!(
                    "Unknown LLM provider: '{other}'. Supported: openai, anthropic, gemini"
                )));
            }
        };

        let env_var = match provider {
            Provider::OpenAi => "OPENAI_API_KEY",
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::Gemini => "GEMINI_API_KEY",
        };

        let api_key = config
            .api_key
            .clone()
            .or_else(|| std::env::var(env_var).ok());

        // Auto-switch default model when provider changes
        let model = match provider {
            Provider::Anthropic if config.model == "gpt-4o" => {
                "claude-sonnet-4-5".to_string()
            }
            Provider::Gemini
                if config.model == "gpt-4o" || config.model == "claude-sonnet-4-5" =>
            {
                "gemini-2.0-flash".to_string()
            }
            _ => config.model.clone(),
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| ArgusError::Llm(format!("failed to create HTTP client: {e}")))?;

        Ok(Self {
            client,
            provider,
            api_key,
            model,
            base_url: config.base_url.clone(),
        })
    }

    /// Return the model name from the configuration.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Send a chat request and return the text response.
    ///
    /// Dispatches to the OpenAI, Anthropic, or Gemini API based on the
    /// configured provider. For Anthropic, system messages are extracted to
    /// a top-level `"system"` field and consecutive user messages are
    /// concatenated. For Gemini, system messages become `systemInstruction`.
    ///
    /// # Errors
    ///
    /// Returns [`ArgusError::Llm`] on HTTP errors or response parsing failures.
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String, ArgusError> {
        match self.provider {
            Provider::OpenAi => self.chat_openai(messages).await,
            Provider::Anthropic => self.chat_anthropic(messages).await,
            Provider::Gemini => self.chat_gemini(messages).await,
        }
    }

    async fn chat_openai(&self, messages: Vec<ChatMessage>) -> Result<String, ArgusError> {
        let api_key = self.api_key.as_deref().ok_or_else(|| {
            ArgusError::Llm(
                "OpenAI API key required. Set it in .argus.toml or export OPENAI_API_KEY".into(),
            )
        })?;

        let base_url = self
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com");
        let url = format!("{base_url}/v1/chat/completions");

        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "temperature": 0.1,
            "response_format": { "type": "json_object" },
        });

        let mut request = self.client.post(&url);
        request = request.header("Authorization", format!("Bearer {api_key}"));
        request = request.header("Content-Type", "application/json");

        let response = request
            .json(&body)
            .send()
            .await
            .map_err(|e| ArgusError::Llm(format!("OpenAI request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            return Err(ArgusError::Llm(format!(
                "OpenAI API error {status}: {body_text}"
            )));
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ArgusError::Llm(format!("failed to parse OpenAI response: {e}")))?;

        let content = response_body
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                ArgusError::Llm(format!(
                    "unexpected OpenAI response structure: {response_body}"
                ))
            })?;

        Ok(content.to_string())
    }

    async fn chat_anthropic(&self, messages: Vec<ChatMessage>) -> Result<String, ArgusError> {
        let api_key = self.api_key.as_deref().ok_or_else(|| {
            ArgusError::Llm(
                "Anthropic API key required. Set it in .argus.toml or export ANTHROPIC_API_KEY"
                    .into(),
            )
        })?;

        let base_url = self
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com");
        let url = format!("{base_url}/v1/messages");

        // Extract system message(s) and non-system messages
        let mut system_parts: Vec<String> = Vec::new();
        let mut chat_messages: Vec<ChatMessage> = Vec::new();
        for msg in messages {
            if msg.role == Role::System {
                system_parts.push(msg.content);
            } else {
                chat_messages.push(msg);
            }
        }
        let system_text = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        };

        // Merge consecutive same-role messages (Anthropic requires alternation)
        let merged = merge_consecutive_messages(chat_messages);

        // Build message array for the API
        let api_messages: Vec<serde_json::Value> = merged
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": api_messages,
        });
        if let Some(system) = &system_text {
            body["system"] = serde_json::Value::String(system.clone());
        }

        let mut request = self.client.post(&url);
        request = request.header("x-api-key", api_key);
        request = request
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json");

        let response = request
            .json(&body)
            .send()
            .await
            .map_err(|e| ArgusError::Llm(format!("Anthropic request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            // Try to extract Anthropic's structured error message
            if let Ok(err_json) = serde_json::from_str::<serde_json::Value>(&body_text) {
                if let Some(msg) = err_json
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                {
                    return Err(ArgusError::Llm(format!(
                        "Anthropic API error {status}: {msg}"
                    )));
                }
            }
            return Err(ArgusError::Llm(format!(
                "Anthropic API error {status}: {body_text}"
            )));
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ArgusError::Llm(format!("failed to parse Anthropic response: {e}")))?;

        // Iterate content blocks to find the first "text" type, skipping "thinking" blocks
        let content_array = response_body
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                ArgusError::Llm(format!(
                    "unexpected Anthropic response structure: {response_body}"
                ))
            })?;

        let text = content_array
            .iter()
            .find(|block| block.get("type").and_then(|t| t.as_str()) == Some("text"))
            .and_then(|block| block.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| {
                ArgusError::Llm("No text content in Anthropic response".into())
            })?;

        Ok(text.to_string())
    }

    async fn chat_gemini(&self, messages: Vec<ChatMessage>) -> Result<String, ArgusError> {
        let api_key = self.api_key.as_deref().ok_or_else(|| {
            ArgusError::Llm(
                "Gemini API key required. Set it in .argus.toml or export GEMINI_API_KEY".into(),
            )
        })?;

        let base_url = self
            .base_url
            .as_deref()
            .unwrap_or("https://generativelanguage.googleapis.com");

        let url = format!(
            "{base_url}/v1beta/models/{}:generateContent?key={api_key}",
            self.model,
        );

        // Redact the API key from error messages to prevent leaking it via
        // URLs embedded in reqwest errors.
        let redact = |msg: String| -> String { msg.replace(api_key, "[REDACTED]") };

        // Extract system messages and build contents array
        let mut system_parts: Vec<String> = Vec::new();
        let mut contents: Vec<serde_json::Value> = Vec::new();
        for msg in messages {
            if msg.role == Role::System {
                system_parts.push(msg.content);
            } else {
                let role = match msg.role {
                    Role::User => "user",
                    Role::Assistant => "model",
                    Role::System => unreachable!(),
                };
                contents.push(serde_json::json!({
                    "role": role,
                    "parts": [{"text": msg.content}],
                }));
            }
        }

        let mut body = serde_json::json!({
            "contents": contents,
            "generationConfig": {
                "temperature": 0.1,
                "maxOutputTokens": 4096,
            },
        });
        if !system_parts.is_empty() {
            let system_text = system_parts.join("\n\n");
            body["systemInstruction"] = serde_json::json!({
                "parts": [{"text": system_text}],
            });
        }

        // Gemini uses key in URL, no Authorization header needed
        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ArgusError::Llm(redact(format!("Gemini request failed: {e}"))))?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            if let Ok(err_json) = serde_json::from_str::<serde_json::Value>(&body_text) {
                if let Some(msg) = err_json
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                {
                    return Err(ArgusError::Llm(redact(format!(
                        "Gemini API error {status}: {msg}"
                    ))));
                }
            }
            return Err(ArgusError::Llm(redact(format!(
                "Gemini API error {status}: {body_text}"
            ))));
        }

        let response_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ArgusError::Llm(redact(format!("failed to parse Gemini response: {e}"))))?;

        let text = response_body
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| {
                ArgusError::Llm(redact(format!(
                    "unexpected Gemini response structure: {response_body}"
                )))
            })?;

        Ok(text.to_string())
    }
}

/// Merge consecutive messages with the same role into single messages.
///
/// Anthropic requires strict user/assistant alternation. This concatenates
/// adjacent messages of the same role with double newlines.
fn merge_consecutive_messages(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut merged: Vec<ChatMessage> = Vec::new();
    for msg in messages {
        let should_merge = merged
            .last()
            .map(|prev| prev.role == msg.role)
            .unwrap_or(false);
        if should_merge {
            let last = merged.last_mut().unwrap();
            last.content.push_str("\n\n");
            last.content.push_str(&msg.content);
        } else {
            merged.push(msg);
        }
    }
    merged
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

    #[test]
    fn unknown_provider_returns_error() {
        let config = LlmConfig {
            provider: "cohere".into(),
            api_key: Some("key".into()),
            ..LlmConfig::default()
        };
        let result = LlmClient::new(&config);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown LLM provider"));
        assert!(err.contains("cohere"));
        assert!(err.contains("openai, anthropic, gemini"));
    }

    #[test]
    fn anthropic_provider_auto_switches_default_model() {
        let config = LlmConfig {
            provider: "anthropic".into(),
            api_key: Some("key".into()),
            ..LlmConfig::default() // model defaults to gpt-4o
        };
        let client = LlmClient::new(&config).unwrap();
        assert_eq!(client.model(), "claude-sonnet-4-5");
    }

    #[test]
    fn anthropic_provider_preserves_custom_model() {
        let config = LlmConfig {
            provider: "anthropic".into(),
            api_key: Some("key".into()),
            model: "claude-opus-4".into(),
            ..LlmConfig::default()
        };
        let client = LlmClient::new(&config).unwrap();
        assert_eq!(client.model(), "claude-opus-4");
    }

    #[test]
    fn openai_provider_keeps_default_model() {
        let config = LlmConfig::default();
        let client = LlmClient::new(&config).unwrap();
        assert_eq!(client.model(), "gpt-4o");
    }

    #[test]
    fn env_var_fallback_openai() {
        std::env::remove_var("OPENAI_API_KEY");
        let config = LlmConfig {
            api_key: None,
            ..LlmConfig::default()
        };
        let client = LlmClient::new(&config).unwrap();
        // No key set, should be None
        assert!(client.api_key.is_none());
    }

    #[test]
    fn env_var_fallback_anthropic() {
        std::env::remove_var("ANTHROPIC_API_KEY");
        let config = LlmConfig {
            provider: "anthropic".into(),
            api_key: None,
            ..LlmConfig::default()
        };
        let client = LlmClient::new(&config).unwrap();
        assert!(client.api_key.is_none());
    }

    #[test]
    fn config_api_key_takes_precedence() {
        std::env::set_var("OPENAI_API_KEY", "env-key");
        let config = LlmConfig {
            api_key: Some("config-key".into()),
            ..LlmConfig::default()
        };
        let client = LlmClient::new(&config).unwrap();
        assert_eq!(client.api_key.as_deref(), Some("config-key"));
        std::env::remove_var("OPENAI_API_KEY");
    }

    #[test]
    fn merge_consecutive_user_messages() {
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: "first".into(),
            },
            ChatMessage {
                role: Role::User,
                content: "second".into(),
            },
            ChatMessage {
                role: Role::Assistant,
                content: "reply".into(),
            },
            ChatMessage {
                role: Role::User,
                content: "third".into(),
            },
        ];
        let merged = merge_consecutive_messages(messages);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].content, "first\n\nsecond");
        assert_eq!(merged[0].role, Role::User);
        assert_eq!(merged[1].content, "reply");
        assert_eq!(merged[1].role, Role::Assistant);
        assert_eq!(merged[2].content, "third");
    }

    #[test]
    fn system_message_extraction() {
        // Verify system messages are separated from chat messages
        let messages = vec![
            ChatMessage {
                role: Role::System,
                content: "You are a code reviewer.".into(),
            },
            ChatMessage {
                role: Role::User,
                content: "Review this code".into(),
            },
        ];

        let mut system_parts: Vec<String> = Vec::new();
        let mut chat_messages: Vec<ChatMessage> = Vec::new();
        for msg in messages {
            if msg.role == Role::System {
                system_parts.push(msg.content);
            } else {
                chat_messages.push(msg);
            }
        }

        assert_eq!(system_parts.len(), 1);
        assert_eq!(system_parts[0], "You are a code reviewer.");
        assert_eq!(chat_messages.len(), 1);
        assert_eq!(chat_messages[0].role, Role::User);
    }

    #[test]
    fn anthropic_request_body_format() {
        // Verify the Anthropic request body structure
        let system_text = "You are a reviewer.";
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": "Review this",
        })];

        let mut body = serde_json::json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 4096,
            "messages": messages,
        });
        body["system"] = serde_json::Value::String(system_text.to_string());

        assert_eq!(body["model"], "claude-sonnet-4-5");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["system"], "You are a reviewer.");
        assert!(body.get("temperature").is_none());
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn anthropic_response_parsing() {
        let response = serde_json::json!({
            "content": [{"type": "text", "text": "{\"comments\":[]}"}],
            "model": "claude-sonnet-4-5",
            "role": "assistant",
        });

        let content = response
            .get("content")
            .and_then(|c| c.as_array())
            .unwrap()
            .iter()
            .find(|block| block.get("type").and_then(|t| t.as_str()) == Some("text"))
            .and_then(|block| block.get("text"))
            .and_then(|t| t.as_str())
            .unwrap();

        assert_eq!(content, "{\"comments\":[]}");
    }

    #[test]
    fn anthropic_thinking_response_parsing() {
        let response = serde_json::json!({
            "content": [
                {"type": "thinking", "thinking": "Let me analyze this code..."},
                {"type": "text", "text": "{\"comments\":[{\"file\":\"a.rs\"}]}"}
            ],
            "model": "claude-sonnet-4-5-thinking",
            "role": "assistant",
        });

        let content = response
            .get("content")
            .and_then(|c| c.as_array())
            .unwrap()
            .iter()
            .find(|block| block.get("type").and_then(|t| t.as_str()) == Some("text"))
            .and_then(|block| block.get("text"))
            .and_then(|t| t.as_str())
            .unwrap();

        assert_eq!(content, "{\"comments\":[{\"file\":\"a.rs\"}]}");
    }

    #[test]
    fn anthropic_multiple_thinking_blocks() {
        let response = serde_json::json!({
            "content": [
                {"type": "thinking", "thinking": "First thought..."},
                {"type": "thinking", "thinking": "Second thought..."},
                {"type": "text", "text": "{\"comments\":[]}"}
            ],
        });

        let content = response
            .get("content")
            .and_then(|c| c.as_array())
            .unwrap()
            .iter()
            .find(|block| block.get("type").and_then(|t| t.as_str()) == Some("text"))
            .and_then(|block| block.get("text"))
            .and_then(|t| t.as_str())
            .unwrap();

        assert_eq!(content, "{\"comments\":[]}");
    }

    #[test]
    fn anthropic_no_text_block_errors() {
        let response = serde_json::json!({
            "content": [
                {"type": "thinking", "thinking": "Just thinking..."}
            ],
        });

        let result: Option<&str> = response
            .get("content")
            .and_then(|c| c.as_array())
            .unwrap()
            .iter()
            .find(|block| block.get("type").and_then(|t| t.as_str()) == Some("text"))
            .and_then(|block| block.get("text"))
            .and_then(|t| t.as_str());

        assert!(result.is_none());
    }

    #[test]
    fn anthropic_error_parsing() {
        let error_body = serde_json::json!({
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "model: field required"
            }
        });

        let msg = error_body
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap();

        assert_eq!(msg, "model: field required");
    }

    #[test]
    fn gemini_provider_auto_switches_from_openai_default() {
        let config = LlmConfig {
            provider: "gemini".into(),
            api_key: Some("key".into()),
            ..LlmConfig::default() // model defaults to gpt-4o
        };
        let client = LlmClient::new(&config).unwrap();
        assert_eq!(client.model(), "gemini-2.0-flash");
    }

    #[test]
    fn gemini_provider_auto_switches_from_anthropic_default() {
        let config = LlmConfig {
            provider: "gemini".into(),
            api_key: Some("key".into()),
            model: "claude-sonnet-4-5".into(),
            ..LlmConfig::default()
        };
        let client = LlmClient::new(&config).unwrap();
        assert_eq!(client.model(), "gemini-2.0-flash");
    }

    #[test]
    fn gemini_provider_preserves_custom_model() {
        let config = LlmConfig {
            provider: "gemini".into(),
            api_key: Some("key".into()),
            model: "gemini-2.5-pro".into(),
            ..LlmConfig::default()
        };
        let client = LlmClient::new(&config).unwrap();
        assert_eq!(client.model(), "gemini-2.5-pro");
    }

    #[test]
    fn gemini_env_var_fallback() {
        std::env::remove_var("GEMINI_API_KEY");
        let config = LlmConfig {
            provider: "gemini".into(),
            api_key: None,
            ..LlmConfig::default()
        };
        let client = LlmClient::new(&config).unwrap();
        assert!(client.api_key.is_none());
    }

    #[test]
    fn gemini_request_body_format() {
        let system_text = "You are a reviewer.";
        let contents = vec![serde_json::json!({
            "role": "user",
            "parts": [{"text": "Review this"}],
        })];

        let mut body = serde_json::json!({
            "contents": contents,
            "generationConfig": {
                "temperature": 0.1,
                "maxOutputTokens": 4096,
            },
        });
        body["systemInstruction"] = serde_json::json!({
            "parts": [{"text": system_text}],
        });

        assert_eq!(body["generationConfig"]["temperature"], 0.1);
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 4096);
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "You are a reviewer.");
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(body["contents"][0]["parts"][0]["text"], "Review this");
    }

    #[test]
    fn gemini_response_parsing() {
        let response = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{"text": "{\"comments\":[]}"}],
                    "role": "model",
                },
            }],
        });

        let text = response
            .get("candidates")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.get(0))
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
            .unwrap();

        assert_eq!(text, "{\"comments\":[]}");
    }

    #[test]
    fn gemini_error_parsing() {
        let error_body = serde_json::json!({
            "error": {
                "code": 400,
                "message": "API key not valid. Please pass a valid API key.",
                "status": "INVALID_ARGUMENT"
            }
        });

        let msg = error_body
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap();

        assert!(msg.contains("API key not valid"));
    }

    #[test]
    fn gemini_role_mapping() {
        // Gemini uses "model" instead of "assistant"
        let messages = vec![
            ChatMessage {
                role: Role::User,
                content: "hello".into(),
            },
            ChatMessage {
                role: Role::Assistant,
                content: "hi".into(),
            },
        ];

        let mut contents: Vec<serde_json::Value> = Vec::new();
        for msg in &messages {
            let role = match msg.role {
                Role::User => "user",
                Role::Assistant => "model",
                Role::System => "system",
            };
            contents.push(serde_json::json!({
                "role": role,
                "parts": [{"text": &msg.content}],
            }));
        }

        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[1]["role"], "model");
    }
}
