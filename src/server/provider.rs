//! Provider server functions: CRUD, live model listing, connection test.

use leptos::prelude::*;
use uuid::Uuid;

use crate::models::provider::{
    ConnectionStatus, ModelInfo, NewProvider, ProviderDto, UpdateProvider,
};

#[server(ListProviders, "/api")]
pub async fn list_providers() -> Result<Vec<ProviderDto>, ServerFnError> {
    use crate::db::Db;
    use crate::services::provider_service::ProviderService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || ProviderService::new(db).list())
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map(|providers| providers.into_iter().map(ProviderDto::from).collect())
        .map_err(|e| ServerFnError::new(format!("Failed to list providers: {e}")))
}

#[server(CreateProvider, "/api")]
pub async fn create_provider(new: NewProvider) -> Result<ProviderDto, ServerFnError> {
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
        .map(ProviderDto::from)
        .map_err(|e| ServerFnError::new(format!("Failed to create provider: {e}")))
}

#[server(UpdateProviderFn, "/api")]
pub async fn update_provider(update: UpdateProvider) -> Result<ProviderDto, ServerFnError> {
    use crate::db::Db;
    use crate::services::provider_service::ProviderService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || ProviderService::new(db).update(&update))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map(ProviderDto::from)
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

    let provider_name = provider.name.clone();
    let llm = build_provider(&provider);
    llm.list_models().await.map_err(|e| {
        ServerFnError::new(format!(
            "Could not discover models for {provider_name}: {e:#}. Check the endpoint and credentials, retry, or enter a model ID manually."
        ))
    })
}

/// Probe through the provider adapter so connection tests use exactly the same
/// URL normalization, authentication, and HTTP behavior as real requests.
#[server(TestProviderConnection, "/api")]
pub async fn test_provider_connection(id: Uuid) -> Result<ConnectionStatus, ServerFnError> {
    use crate::db::Db;
    use crate::providers::registry::build_provider;
    use crate::services::provider_service::ProviderService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    let provider = tokio::task::spawn_blocking(move || ProviderService::new(db).get(id))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to load provider: {e}")))?;

    let Some(provider) = provider else {
        return Ok(ConnectionStatus::Failed(
            "Provider not found. Reload settings and try again.".into(),
        ));
    };

    let llm = build_provider(&provider);
    llm.test_connection()
        .await
        .map_err(|e| ServerFnError::new(format!("Connection test could not be completed: {e:#}")))
}
