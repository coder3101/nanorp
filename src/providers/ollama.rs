//! Ollama backend: native `/api/*` endpoints, NDJSON streaming.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use crate::models::message::LlmMessage;
use crate::models::provider::{ConnectionStatus, ModelInfo, ProviderType};
use crate::models::settings::SamplingParams;
use crate::providers::buffering::LineBuffer;
use crate::providers::http::{
    self, JSON_REQUEST_TIMEOUT, MODELS_TIMEOUT, STREAM_IDLE_TIMEOUT, TEST_TIMEOUT,
};
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
        let resp = http::send(self.client.get(&url), MODELS_TIMEOUT)
            .await
            .with_context(|| format!("Cannot connect to Ollama at {}", self.base_url))?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Ollama returned {} for /api/tags; check the base URL",
                resp.status()
            ));
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

        let resp = http::start_stream(self.client.post(&url).json(&body))
            .await
            .with_context(|| format!("Cannot connect to Ollama at {}", self.base_url))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama chat failed ({status}): {text}"));
        }

        // NDJSON stream → StreamEvent stream, buffering bytes until whole lines
        // are present so split UTF-8 sequences remain intact.
        let byte_stream = resp.bytes_stream();
        let mut buffer = LineBuffer::default();

        let stream = async_stream::stream! {
            futures::pin_mut!(byte_stream);
            loop {
                let next = tokio::time::timeout(STREAM_IDLE_TIMEOUT, byte_stream.next()).await;
                let Some(chunk) = (match next {
                    Ok(chunk) => chunk,
                    Err(_) => {
                        yield Ok(StreamEvent::Error("Ollama stopped sending stream data for 90 seconds; retry the request".to_string()));
                        return;
                    }
                }) else { break; };
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        yield Ok(StreamEvent::Error(format!("stream error: {e}")));
                        return;
                    }
                };
                let lines = match buffer.push(&bytes) {
                    Ok(lines) => lines,
                    Err(e) => {
                        yield Ok(StreamEvent::Error(e.to_string()));
                        return;
                    }
                };

                for line in lines {
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
                        Err(e) => {
                            yield Ok(StreamEvent::Error(format!("Ollama returned an invalid NDJSON line: {e}")));
                            return;
                        }
                    }
                }
            }
            // Flush a valid trailing NDJSON record even without a final \n.
            match buffer.finish() {
                Ok(Some(rest)) if !rest.trim().is_empty() => {
                    match serde_json::from_str::<OllamaChatChunk>(rest.trim()) {
                        Ok(parsed) => {
                            if let Some(error) = parsed.error {
                                yield Ok(StreamEvent::Error(error));
                                return;
                            }
                            if let Some(m) = parsed.message {
                                if let Some(thinking) = m.thinking.filter(|t| !t.is_empty()) {
                                    yield Ok(StreamEvent::Reasoning(thinking));
                                }
                                if !m.content.is_empty() {
                                    yield Ok(StreamEvent::Delta(m.content));
                                }
                            }
                        }
                        Err(e) => {
                            yield Ok(StreamEvent::Error(format!("Ollama returned invalid trailing NDJSON: {e}")));
                            return;
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    yield Ok(StreamEvent::Error(e.to_string()));
                    return;
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

        let resp = http::send(self.client.post(&url).json(&body), JSON_REQUEST_TIMEOUT)
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
        match http::send(self.client.get(&url), TEST_TIMEOUT).await {
            Ok(resp) if resp.status().is_success() => Ok(ConnectionStatus::Connected),
            Ok(resp) => Ok(ConnectionStatus::Failed(format!(
                "Ollama returned {} for /api/tags. Check that the API URL points to the Ollama server root.",
                resp.status()
            ))),
            Err(e) => Ok(ConnectionStatus::Failed(http::connection_error(&self.base_url, &e))),
        }
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::Ollama
    }
}
