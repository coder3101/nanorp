//! Builds a live provider from its stored config on demand, reusing one
//! shared `reqwest::Client` for connection pooling. No global timeout: chat
//! streams are long-lived; timeouts are applied per request where needed.

use std::sync::Arc;
use std::sync::OnceLock;

use crate::models::provider::{Provider, ProviderType};
use crate::providers::ollama::OllamaProvider;
use crate::providers::openai::OpenAiProvider;
use crate::providers::traits::LlmProvider;

/// A process-wide shared HTTP client (connection pooling).
fn shared_client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build reqwest client")
        })
        .clone()
}

/// Build a live `LlmProvider` instance from a stored `Provider` config.
pub fn build_provider(provider: &Provider) -> Arc<dyn LlmProvider> {
    let client = shared_client();
    match provider.provider_type {
        ProviderType::Ollama => {
            Arc::new(OllamaProvider::new(client, provider.api_url.clone()))
        }
        ProviderType::OpenAiCompatible => Arc::new(OpenAiProvider::new(
            client,
            provider.api_url.clone(),
            provider.api_key.clone(),
        )),
    }
}
