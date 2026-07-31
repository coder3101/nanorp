//! Ollama backend: native `/api/*` endpoints, NDJSON streaming.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use crate::models::message::LlmMessage;
use crate::models::provider::{ConnectionStatus, ModelInfo, ProviderType};
use crate::models::settings::SamplingParams;
use crate::providers::traits::{strip_code_fence, LlmProvider, StreamEvent};

pub struct OllamaProvider {
    client: reqwest::Client,
    base_url: String,
}

impl OllamaProvider {
    pub fn new(client: reqwest::Client, base_url: String) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }
}

// ---- Ollama API serde types ----

#[derive(Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaChatMessage>,
    stream: bool,
    options: OllamaOptions,
    /// JSON mode: when Some, Ollama is asked to reply with a single JSON value.
    #[serde(skip_serializing_if = "Option::is_none")]
    format: Option<String>,
}

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    top_p: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<u32>,
}

#[derive(Serialize)]
struct OllamaChatMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct OllamaChatChunk {
    #[serde(default)]
    message: Option<OllamaChunkMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

/// Non-streaming chat response (used by `chat_json`).
#[derive(Deserialize)]
struct OllamaChatResponse {
    #[serde(default)]
    message: Option<OllamaChunkMessage>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct OllamaChunkMessage {
    #[serde(default)]
    content: String,
    /// Present on reasoning models (e.g. when the request enables thinking).
    #[serde(default)]
    thinking: Option<String>,
}

fn to_ollama_message(msg: &LlmMessage) -> OllamaChatMessage {
    OllamaChatMessage {
        role: msg.role.to_string(),
        content: msg.content.clone(),
        images: if msg.images.is_empty() {
            None
        } else {
            Some(msg.images.iter().map(|i| i.base64_data.clone()).collect())
        },
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let url = format!("{}/api/tags", self.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Cannot connect to Ollama at {}", self.base_url))?;

        if !resp.status().is_success() {
            return Err(anyhow!("Ollama returned {} for /api/tags", resp.status()));
        }

        let parsed: OllamaTagsResponse = resp.json().await.context("parse /api/tags response")?;
        Ok(parsed
            .models
            .into_iter()
            .map(|m| ModelInfo {
                id: m.name.clone(),
                name: m.name,
                size: m.size,
            })
            .collect())
    }

    async fn stream_chat(
        &self,
        messages: Vec<LlmMessage>,
        model: &str,
        params: &SamplingParams,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let url = format!("{}/api/chat", self.base_url);
        let body = OllamaChatRequest {
            model: model.to_string(),
            messages: messages.iter().map(to_ollama_message).collect(),
            stream: true,
            options: OllamaOptions {
                temperature: params.temperature,
                top_p: params.top_p,
                num_predict: params.max_tokens,
            },
            format: None,
        };

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Cannot connect to Ollama at {}", self.base_url))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama chat failed ({status}): {text}"));
        }

        // NDJSON stream → StreamEvent stream, buffering partial lines.
        let byte_stream = resp.bytes_stream();
        let mut buffer = String::new();

        let stream = async_stream::stream! {
            futures::pin_mut!(byte_stream);
            while let Some(chunk) = byte_stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        yield Ok(StreamEvent::Error(format!("stream error: {e}")));
                        return;
                    }
                };
                buffer.push_str(&String::from_utf8_lossy(&bytes));

                // Process complete lines.
                while let Some(nl) = buffer.find('\n') {
                    let line: String = buffer.drain(..=nl).collect();
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<OllamaChatChunk>(line) {
                        Ok(parsed) => {
                            if let Some(err) = parsed.error {
                                yield Ok(StreamEvent::Error(err));
                                return;
                            }
                            if let Some(m) = parsed.message {
                                if let Some(t) = m.thinking {
                                    if !t.is_empty() {
                                        yield Ok(StreamEvent::Reasoning(t));
                                    }
                                }
                                if !m.content.is_empty() {
                                    yield Ok(StreamEvent::Delta(m.content));
                                }
                            }
                            if parsed.done {
                                yield Ok(StreamEvent::Done);
                                return;
                            }
                        }
                        Err(_) => {
                            // Ignore unparseable partial lines.
                        }
                    }
                }
            }
            // Flush any trailing buffered JSON line.
            let rest = buffer.trim();
            if !rest.is_empty() {
                if let Ok(parsed) = serde_json::from_str::<OllamaChatChunk>(rest) {
                    if let Some(m) = parsed.message {
                        if !m.content.is_empty() {
                            yield Ok(StreamEvent::Delta(m.content));
                        }
                    }
                }
            }
            yield Ok(StreamEvent::Done);
        };

        Ok(Box::pin(stream))
    }

    async fn chat_json(
        &self,
        messages: Vec<LlmMessage>,
        model: &str,
        params: &SamplingParams,
    ) -> Result<String> {
        let url = format!("{}/api/chat", self.base_url);
        let body = OllamaChatRequest {
            model: model.to_string(),
            messages: messages.iter().map(to_ollama_message).collect(),
            stream: false,
            options: OllamaOptions {
                temperature: params.temperature,
                top_p: params.top_p,
                num_predict: params.max_tokens,
            },
            format: Some("json".to_string()),
        };

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Cannot connect to Ollama at {}", self.base_url))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama chat failed ({status}): {text}"));
        }

        let parsed: OllamaChatResponse = resp.json().await.context("parse /api/chat response")?;
        if let Some(err) = parsed.error {
            return Err(anyhow!("Ollama error: {err}"));
        }
        let content = parsed.message.map(|m| m.content).unwrap_or_default();
        Ok(strip_code_fence(&content))
    }

    async fn test_connection(&self) -> Result<ConnectionStatus> {
        let url = format!("{}/api/tags", self.base_url);
        match self.client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => Ok(ConnectionStatus::Connected),
            Ok(resp) => Ok(ConnectionStatus::Failed(format!(
                "Server returned {}",
                resp.status()
            ))),
            Err(e) => Ok(ConnectionStatus::Failed(if e.is_connect() {
                format!("Cannot connect to Ollama at {}", self.base_url)
            } else {
                format!("Request failed: {e}")
            })),
        }
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::Ollama
    }
}
