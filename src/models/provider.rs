//! LLM provider configuration: Ollama or any OpenAI-compatible endpoint.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The type of LLM provider, determines which API format to use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    /// Ollama local server — uses `/api/chat` endpoint with its own JSON format.
    Ollama,
    /// Any OpenAI-compatible API — uses `/v1/chat/completions` endpoint.
    /// Works with: OpenAI, LM Studio, Groq, Together.ai, vLLM, Anthropic-compatible proxies, etc.
    OpenAiCompatible,
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderType::Ollama => write!(f, "Ollama"),
            ProviderType::OpenAiCompatible => write!(f, "OpenAI Compatible"),
        }
    }
}

/// A configured LLM provider endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: Uuid,
    pub name: String,
    pub provider_type: ProviderType,
    /// Base URL without trailing slash (e.g. "http://localhost:11434")
    pub api_url: String,
    /// API key — required for OpenAI-compatible, None for Ollama
    pub api_key: Option<String>,
    /// Whether this is the default provider for new chats
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for creating a new provider. ID and timestamps are generated server-side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewProvider {
    pub name: String,
    pub provider_type: ProviderType,
    pub api_url: String,
    pub api_key: Option<String>,
    pub is_default: bool,
}

/// Input for updating an existing provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProvider {
    pub id: Uuid,
    pub name: Option<String>,
    pub provider_type: Option<ProviderType>,
    pub api_url: Option<String>,
    pub api_key: Option<Option<String>>, // Some(None) = clear key, Some(Some(x)) = set key, None = no change
    pub is_default: Option<bool>,
}

/// Information about an available model from a provider.
/// Fetched live from the provider API, not persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model identifier used in API calls (e.g. "llama3:8b", "gpt-4o")
    pub id: String,
    /// Human-readable name (often same as id, but providers may provide a display name)
    pub name: String,
    /// Size in bytes if available (Ollama provides this)
    pub size: Option<u64>,
}

/// Result of testing a connection to a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionStatus {
    /// Connection successful, provider is reachable
    Connected,
    /// Connection failed with an error message
    Failed(String),
    /// Currently testing the connection
    Testing,
}
