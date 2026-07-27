//! Application settings, persisted as JSON-encoded key/value rows.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Application-wide settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppSettings {
    /// Default system prompt used when a character doesn't define one.
    /// Supports {{char}} and {{user}} placeholders.
    pub default_system_prompt: String,

    /// User's display name, used for {{user}} placeholder replacement.
    pub user_name: String,

    /// UUID of the default provider for new chats (None = no default set yet).
    pub default_provider_id: Option<Uuid>,

    /// Default model ID (e.g. "llama3:8b"). None = user must select.
    pub default_model: Option<String>,

    /// UI theme: "light", "dark", or "system"
    pub theme: String,

    /// Whether to render the model's reasoning ("thinking") in chat bubbles.
    pub render_thinking: bool,

    /// Sampling temperature (0.0 – 2.0). Higher = more random.
    pub temperature: f32,

    /// Nucleus sampling probability (0.0 – 1.0).
    pub top_p: f32,

    /// Maximum tokens to generate (None = provider default / unlimited).
    pub max_tokens: Option<u32>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_system_prompt: String::from(
                "You are {{char}}. Stay in character at all times. \
                 Respond naturally and creatively to {{user}}'s messages. \
                 Be descriptive and engaging.",
            ),
            user_name: String::from("User"),
            default_provider_id: None,
            default_model: None,
            theme: String::from("system"),
            render_thinking: true,
            temperature: 0.8,
            top_p: 0.95,
            max_tokens: None,
        }
    }
}

/// LLM sampling parameters passed to a provider for a single generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingParams {
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens: Option<u32>,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: 0.8,
            top_p: 0.95,
            max_tokens: None,
        }
    }
}

impl AppSettings {
    /// The sampling parameters derived from the current settings.
    pub fn sampling(&self) -> SamplingParams {
        SamplingParams {
            temperature: self.temperature,
            top_p: self.top_p,
            max_tokens: self.max_tokens,
        }
    }
}
