//! The `LlmProvider` trait — the contract every LLM backend implements.

use crate::models::message::LlmMessage;
use crate::models::provider::{ConnectionStatus, ModelInfo, ProviderType};
use crate::models::settings::SamplingParams;
use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

/// Events emitted by the streaming chat response.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// A chunk of the final answer (one or more tokens).
    Delta(String),
    /// A chunk of the model's reasoning / "thinking" output. Emitted by
    /// reasoning models either via a dedicated API field (Ollama `thinking`,
    /// OpenAI-compatible `reasoning_content`) — inline `<think>` tags in the
    /// answer text are handled by the UI instead.
    Reasoning(String),
    /// Generation is complete.
    Done,
    /// An error occurred during streaming.
    Error(String),
}

/// The core trait that all LLM provider backends must implement.
///
/// Implementations exist for:
/// - `OllamaProvider` (ollama.rs) — Ollama local server
/// - `OpenAiProvider` (openai.rs) — Any OpenAI-compatible API
///
/// Providers are constructed with their connection config and a shared
/// reqwest::Client, then registered in the ProviderRegistry.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Fetch the list of available models from this provider.
    async fn list_models(&self) -> Result<Vec<ModelInfo>>;

    /// Stream a chat completion response.
    ///
    /// # Arguments
    /// - `messages`: The full conversation history (system + user + assistant messages)
    ///   with placeholders already resolved. Messages may include images via
    ///   `LlmMessage.images` for vision-capable models.
    /// - `model`: The model ID to use (e.g. "llama3:8b", "gpt-4o").
    ///
    /// # Returns
    /// A stream of `StreamEvent` items. The stream ends with `StreamEvent::Done`.
    /// Dropping the stream cancels the request.
    ///
    /// # Image handling
    /// When a message has non-empty `images`, the provider must format them
    /// according to its API:
    /// - Ollama: `"images": ["base64data1", "base64data2"]` in the message object
    /// - OpenAI: multipart content array with `{"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}}` entries
    async fn stream_chat(
        &self,
        messages: Vec<LlmMessage>,
        model: &str,
        params: &SamplingParams,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>>;

    /// Perform a non-streaming chat completion that returns structured (JSON)
    /// output. Used for things like AI-assisted character generation. The
    /// prompt is expected to instruct the model to respond with a single JSON
    /// object; providers that support it switch into a JSON mode via their API.
    ///
    /// Returns the raw JSON text as produced by the model.
    async fn chat_json(
        &self,
        messages: Vec<LlmMessage>,
        model: &str,
        params: &SamplingParams,
    ) -> Result<String>;

    /// Test whether this provider is reachable.
    async fn test_connection(&self) -> Result<ConnectionStatus>;

    /// Returns the type of this provider.
    fn provider_type(&self) -> ProviderType;
}

/// Strip a surrounding ```json ... ``` / ``` ... ``` code fence from model
/// output, so providers that wrap their JSON in markdown still parse cleanly.
pub(crate) fn strip_code_fence(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        // Drop a language tag on the opening fence if present (e.g. ```json).
        let body = rest
            .split_once('\n')
            .map(|(_, b)| b)
            .unwrap_or(rest.trim_start_matches("json"));
        if let Some(end) = body.rfind("```") {
            return body[..end].trim().to_string();
        }
        return body.trim().to_string();
    }
    trimmed.to_string()
}
