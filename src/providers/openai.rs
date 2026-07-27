//! OpenAI-compatible backend: `/v1/*` endpoints, SSE streaming. Works with
//! OpenAI, LM Studio, Groq, Together, vLLM, etc.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use crate::models::message::LlmMessage;
use crate::models::provider::{ConnectionStatus, ModelInfo, ProviderType};
use crate::models::settings::SamplingParams;
use crate::providers::traits::{LlmProvider, StreamEvent};

pub struct OpenAiProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl OpenAiProvider {
    pub fn new(client: reqwest::Client, base_url: String, api_key: Option<String>) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.filter(|k| !k.is_empty()),
        }
    }

    /// Build a `/v1/...` URL, avoiding a doubled `/v1` when the configured base
    /// already ends with it (e.g. LM Studio's `http://localhost:1234/v1`).
    fn api_url(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        if self.base_url.ends_with("/v1") {
            format!("{}/{}", self.base_url, path.trim_start_matches("v1/"))
        } else {
            format!("{}/{}", self.base_url, path)
        }
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => req.bearer_auth(key),
            None => req,
        }
    }
}

// ---- OpenAI API serde types ----

#[derive(Deserialize)]
struct OpenAiModelsResponse {
    #[serde(default)]
    data: Vec<OpenAiModel>,
}

#[derive(Deserialize)]
struct OpenAiModel {
    id: String,
}

#[derive(Serialize)]
struct OpenAiChatRequest {
    model: String,
    messages: Vec<OpenAiChatMessage>,
    stream: bool,
    temperature: f32,
    top_p: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Serialize)]
struct OpenAiChatMessage {
    role: String,
    content: OpenAiContent,
}

#[derive(Serialize)]
#[serde(untagged)]
enum OpenAiContent {
    Text(String),
    Parts(Vec<OpenAiContentPart>),
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum OpenAiContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: OpenAiImageUrl },
}

#[derive(Serialize)]
struct OpenAiImageUrl {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiStreamChunk {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    #[serde(default)]
    delta: OpenAiDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
    /// Reasoning models (e.g. DeepSeek) stream their thinking here.
    #[serde(default)]
    reasoning_content: Option<String>,
}

fn to_openai_message(msg: &LlmMessage) -> OpenAiChatMessage {
    if msg.images.is_empty() {
        OpenAiChatMessage {
            role: msg.role.to_string(),
            content: OpenAiContent::Text(msg.content.clone()),
        }
    } else {
        let mut parts = vec![OpenAiContentPart::Text {
            text: msg.content.clone(),
        }];
        for img in &msg.images {
            parts.push(OpenAiContentPart::ImageUrl {
                image_url: OpenAiImageUrl {
                    url: format!("data:{};base64,{}", img.content_type, img.base64_data),
                    detail: Some("auto".to_string()),
                },
            });
        }
        OpenAiChatMessage {
            role: msg.role.to_string(),
            content: OpenAiContent::Parts(parts),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let url = self.api_url("v1/models");
        let resp = self
            .auth(self.client.get(&url))
            .send()
            .await
            .with_context(|| format!("Cannot connect to {}", self.base_url))?;

        // Some servers (e.g. LM Studio variants) may not implement /v1/models.
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            tracing::warn!("provider {} has no /v1/models endpoint", self.base_url);
            return Ok(Vec::new());
        }
        if !resp.status().is_success() {
            return Err(anyhow!(
                "Provider returned {} for /v1/models",
                resp.status()
            ));
        }

        let parsed: OpenAiModelsResponse =
            resp.json().await.context("parse /v1/models response")?;
        let mut models: Vec<ModelInfo> = parsed
            .data
            .into_iter()
            .map(|m| ModelInfo {
                id: m.id.clone(),
                name: m.id,
                size: None,
            })
            .collect();
        models.sort_by_key(|a| a.name.to_lowercase());
        Ok(models)
    }

    async fn stream_chat(
        &self,
        messages: Vec<LlmMessage>,
        model: &str,
        params: &SamplingParams,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let url = self.api_url("v1/chat/completions");
        let body = OpenAiChatRequest {
            model: model.to_string(),
            messages: messages.iter().map(to_openai_message).collect(),
            stream: true,
            temperature: params.temperature,
            top_p: params.top_p,
            max_tokens: params.max_tokens,
        };

        let resp = self
            .auth(self.client.post(&url).json(&body))
            .send()
            .await
            .with_context(|| format!("Cannot connect to {}", self.base_url))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let reason = if status == reqwest::StatusCode::UNAUTHORIZED {
                "Invalid API key".to_string()
            } else if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                "Rate limited by provider".to_string()
            } else {
                format!("{status}: {text}")
            };
            return Err(anyhow!("Chat request failed ({reason})"));
        }

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

                while let Some(nl) = buffer.find('\n') {
                    let line: String = buffer.drain(..=nl).collect();
                    let line = line.trim();
                    if line.is_empty() || !line.starts_with("data:") {
                        continue;
                    }
                    let data = line["data:".len()..].trim();
                    if data == "[DONE]" {
                        yield Ok(StreamEvent::Done);
                        return;
                    }
                    if let Ok(parsed) = serde_json::from_str::<OpenAiStreamChunk>(data) {
                        if let Some(choice) = parsed.choices.into_iter().next() {
                            if let Some(reasoning) = choice.delta.reasoning_content {
                                if !reasoning.is_empty() {
                                    yield Ok(StreamEvent::Reasoning(reasoning));
                                }
                            }
                            if let Some(content) = choice.delta.content {
                                if !content.is_empty() {
                                    yield Ok(StreamEvent::Delta(content));
                                }
                            }
                            if choice.finish_reason.as_deref() == Some("stop") {
                                yield Ok(StreamEvent::Done);
                                return;
                            }
                        }
                    }
                }
            }
            yield Ok(StreamEvent::Done);
        };

        Ok(Box::pin(stream))
    }

    async fn test_connection(&self) -> Result<ConnectionStatus> {
        let url = self.api_url("v1/models");
        match self.auth(self.client.get(&url)).send().await {
            Ok(resp) if resp.status().is_success() => Ok(ConnectionStatus::Connected),
            Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => {
                Ok(ConnectionStatus::Failed("Invalid API key".to_string()))
            }
            Ok(resp) => Ok(ConnectionStatus::Failed(format!(
                "Server returned {}",
                resp.status()
            ))),
            Err(e) => Ok(ConnectionStatus::Failed(if e.is_connect() {
                format!("Cannot connect to {}", self.base_url)
            } else {
                format!("Request failed: {e}")
            })),
        }
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenAiCompatible
    }
}
