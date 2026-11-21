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
use crate::providers::buffering::{LineBuffer, SseDecoder};
use crate::providers::http::{
    self, JSON_REQUEST_TIMEOUT, MODELS_TIMEOUT, STREAM_IDLE_TIMEOUT, TEST_TIMEOUT,
};
use crate::providers::traits::{strip_code_fence, LlmProvider, StreamEvent};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<OpenAiResponseFormat>,
}

#[derive(Serialize)]
struct OpenAiResponseFormat {
    /// "json_object" is the most widely supported structured-output mode across
    /// OpenAI and its compatible clones (Groq, vLLM, LM Studio, etc.).
    r#type: String,
}

/// Non-streaming chat completion response (used by `chat_json`).
#[derive(Deserialize)]
struct OpenAiChatResponse {
    #[serde(default)]
    choices: Vec<OpenAiResponseChoice>,
}

#[derive(Deserialize)]
struct OpenAiResponseChoice {
    message: OpenAiResponseMessage,
}

#[derive(Deserialize)]
struct OpenAiResponseMessage {
    content: String,
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
        let resp = http::send(self.auth(self.client.get(&url)), MODELS_TIMEOUT)
            .await
            .with_context(|| format!("Cannot connect to {}", self.base_url))?;

        // Model discovery is optional for some compatible servers. Keep the
        // error explicit; the selector offers a manual model-ID fallback.
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            tracing::warn!("provider {} has no /v1/models endpoint", self.base_url);
            return Err(anyhow!(
                "Model discovery is not supported at {url}. Verify the base URL or use an exact model ID"
            ));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let guidance = match status {
                reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
                    "; check the saved API key and endpoint permissions"
                }
                _ => "; verify the OpenAI-compatible base URL",
            };
            return Err(anyhow!(
                "Provider returned {status} for /v1/models{guidance}"
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
            response_format: None,
        };

        let resp = http::start_stream(self.auth(self.client.post(&url).json(&body)))
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
        let mut lines = LineBuffer::default();
        let mut events = SseDecoder::default();

        let stream = async_stream::stream! {
            futures::pin_mut!(byte_stream);
            loop {
                let next = tokio::time::timeout(STREAM_IDLE_TIMEOUT, byte_stream.next()).await;
                let Some(chunk) = (match next {
                    Ok(chunk) => chunk,
                    Err(_) => {
                        yield Ok(StreamEvent::Error("Provider stopped sending stream data for 90 seconds; retry the request".to_string()));
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
                let decoded_lines = match lines.push(&bytes) {
                    Ok(lines) => lines,
                    Err(e) => {
                        yield Ok(StreamEvent::Error(e.to_string()));
                        return;
                    }
                };

                for line in decoded_lines {
                    let Some(data) = events.push_line(&line) else { continue; };
                    if data == "[DONE]" {
                        yield Ok(StreamEvent::Done);
                        return;
                    }
                    if let Ok(parsed) = serde_json::from_str::<OpenAiStreamChunk>(&data) {
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
                            if choice.finish_reason.is_some() {
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

    async fn chat_json(
        &self,
        messages: Vec<LlmMessage>,
        model: &str,
        params: &SamplingParams,
    ) -> Result<String> {
        let url = self.api_url("v1/chat/completions");
        let body = OpenAiChatRequest {
            model: model.to_string(),
            messages: messages.iter().map(to_openai_message).collect(),
            stream: false,
            temperature: params.temperature,
            top_p: params.top_p,
            max_tokens: params.max_tokens,
            response_format: Some(OpenAiResponseFormat {
                r#type: "json_object".to_string(),
            }),
        };

        let resp = http::send(
            self.auth(self.client.post(&url).json(&body)),
            JSON_REQUEST_TIMEOUT,
        )
        .await
        .with_context(|| format!("Cannot connect to {}", self.base_url))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Structured completion failed ({status}): {text}"));
        }

        let parsed: OpenAiChatResponse = resp
            .json()
            .await
            .context("parse chat completion response")?;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();
        Ok(strip_code_fence(&content))
    }

    async fn test_connection(&self) -> Result<ConnectionStatus> {
        let url = self.api_url("v1/models");
        match http::send(self.auth(self.client.get(&url)), TEST_TIMEOUT).await {
            Ok(resp) if resp.status().is_success() => Ok(ConnectionStatus::Connected),
            Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => {
                Ok(ConnectionStatus::Failed("Invalid or missing API key. Replace or clear the saved key in Settings.".to_string()))
            }
            Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => {
                Ok(ConnectionStatus::Failed(format!("The server was reached, but {url} was not found. Check whether the base URL should include /v1; you can still try a manual model ID.")))
            }
            Ok(resp) => Ok(ConnectionStatus::Failed(format!(
                "Server returned {}. Check the base URL and provider permissions.",
                resp.status()
            ))),
            Err(e) => Ok(ConnectionStatus::Failed(http::connection_error(&self.base_url, &e))),
        }
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenAiCompatible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_urls_work_with_root_and_v1_base_urls() {
        let client = reqwest::Client::new();
        let root = OpenAiProvider::new(client.clone(), "https://example.com/".into(), None);
        assert_eq!(root.api_url("v1/models"), "https://example.com/v1/models");

        let v1 = OpenAiProvider::new(client, "https://example.com/v1/".into(), None);
        assert_eq!(v1.api_url("v1/models"), "https://example.com/v1/models");
        assert_eq!(
            v1.api_url("v1/chat/completions"),
            "https://example.com/v1/chat/completions"
        );
    }
}
