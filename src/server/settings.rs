//! Settings server functions.

use leptos::prelude::*;

use crate::models::settings::AppSettings;

#[server(GetSettings, "/api")]
pub async fn get_settings() -> Result<AppSettings, ServerFnError> {
    use crate::db::Db;
    use crate::services::settings_service::SettingsService;

    let db = use_context::<Db>()
        .ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || SettingsService::new(db).get_all())
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to load settings: {e}")))
}

#[server(UpdateSettings, "/api")]
pub async fn update_settings(settings: AppSettings) -> Result<(), ServerFnError> {
    use crate::db::Db;
    use crate::services::settings_service::SettingsService;

    let db = use_context::<Db>()
        .ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || SettingsService::new(db).set_all(&settings))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to save settings: {e}")))
}
