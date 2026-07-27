//! Provider server functions: CRUD, live model listing, connection test.

use leptos::prelude::*;
use uuid::Uuid;

use crate::models::provider::{ConnectionStatus, ModelInfo, NewProvider, Provider, UpdateProvider};

#[server(ListProviders, "/api")]
pub async fn list_providers() -> Result<Vec<Provider>, ServerFnError> {
    use crate::db::Db;
    use crate::services::provider_service::ProviderService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || ProviderService::new(db).list())
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to list providers: {e}")))
}

#[server(CreateProvider, "/api")]
pub async fn create_provider(new: NewProvider) -> Result<Provider, ServerFnError> {
    use crate::db::Db;
    use crate::services::provider_service::ProviderService;

    if new.name.trim().is_empty() {
        return Err(ServerFnError::new("Provider name is required"));
    }
    if new.api_url.trim().is_empty() {
        return Err(ServerFnError::new("API URL is required"));
    }

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || ProviderService::new(db).create(&new))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to create provider: {e}")))
}

#[server(UpdateProviderFn, "/api")]
pub async fn update_provider(update: UpdateProvider) -> Result<Provider, ServerFnError> {
    use crate::db::Db;
    use crate::services::provider_service::ProviderService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || ProviderService::new(db).update(&update))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to update provider: {e}")))
}

#[server(DeleteProvider, "/api")]
pub async fn delete_provider(id: Uuid) -> Result<(), ServerFnError> {
    use crate::db::Db;
    use crate::services::provider_service::ProviderService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || ProviderService::new(db).delete(id))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to delete provider: {e}")))
}

/// Fetch the models available from a provider (live query).
#[server(ListProviderModels, "/api")]
pub async fn list_provider_models(id: Uuid) -> Result<Vec<ModelInfo>, ServerFnError> {
    use crate::db::Db;
    use crate::providers::registry::build_provider;
    use crate::services::provider_service::ProviderService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    let provider = tokio::task::spawn_blocking(move || ProviderService::new(db).get(id))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to load provider: {e}")))?
        .ok_or_else(|| ServerFnError::new("Provider not found"))?;

    let llm = build_provider(&provider);
    llm.list_models()
        .await
        .map_err(|e| ServerFnError::new(format!("Failed to list models: {e}")))
}

/// Probe a provider's endpoint to verify reachability & credentials.
#[server(TestProviderConnection, "/api")]
pub async fn test_provider_connection(id: Uuid) -> Result<ConnectionStatus, ServerFnError> {
    use crate::db::Db;
    use crate::models::provider::ProviderType;
    use crate::services::provider_service::ProviderService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    // Load the provider config off the async runtime.
    let db_for_lookup = db.clone();
    let provider = tokio::task::spawn_blocking(move || ProviderService::new(db_for_lookup).get(id))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to load provider: {e}")))?;

    let Some(provider) = provider else {
        return Ok(ConnectionStatus::Failed("Provider not found".into()));
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| ServerFnError::new(format!("HTTP client error: {e}")))?;

    let url = match provider.provider_type {
        ProviderType::Ollama => format!("{}/api/tags", provider.api_url),
        ProviderType::OpenAiCompatible => format!("{}/v1/models", provider.api_url),
    };

    let mut req = client.get(&url);
    if let Some(key) = provider.api_key.as_ref().filter(|k| !k.is_empty()) {
        req = req.bearer_auth(key);
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => Ok(ConnectionStatus::Connected),
        Ok(resp) => Ok(ConnectionStatus::Failed(format!(
            "Server returned {}",
            resp.status()
        ))),
        Err(e) => {
            let reason = if e.is_timeout() {
                "Connection timed out".to_string()
            } else if e.is_connect() {
                "Could not connect to endpoint".to_string()
            } else {
                format!("Request failed: {e}")
            };
            Ok(ConnectionStatus::Failed(reason))
        }
    }
}
