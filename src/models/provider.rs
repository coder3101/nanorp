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

/// A configured LLM provider endpoint, including its decrypted credentials.
///
/// This type is server-internal. Browser-facing server functions return
/// [`ProviderDto`] instead so an API key can never be serialized accidentally.
#[derive(Debug, Clone)]
pub struct Provider {
    pub id: Uuid,
    pub name: String,
    pub provider_type: ProviderType,
    /// Base URL without trailing slash (e.g. "http://localhost:11434")
    pub api_url: String,
    /// Decrypted API key. Never expose this type from a browser-facing API.
    pub api_key: Option<String>,
    /// Whether this is the default provider for new chats
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Public provider configuration returned to the browser.
///
/// `has_api_key` lets the settings UI offer keep/replace/clear behavior without
/// sending the existing secret back to the client, even in masked form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDto {
    pub id: Uuid,
    pub name: String,
    pub provider_type: ProviderType,
    pub api_url: String,
    pub has_api_key: bool,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Provider> for ProviderDto {
    fn from(provider: Provider) -> Self {
        Self {
            id: provider.id,
            name: provider.name,
            provider_type: provider.provider_type,
            api_url: provider.api_url,
            has_api_key: provider.api_key.is_some(),
            is_default: provider.is_default,
            created_at: provider.created_at,
            updated_at: provider.updated_at,
        }
    }
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
