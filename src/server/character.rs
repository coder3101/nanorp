//! Character server functions: CRUD plus base64 avatar upload/removal.

use leptos::prelude::*;
use uuid::Uuid;

use crate::models::character::{Character, NewCharacter, UpdateCharacter};

#[server(ListCharacters, "/api")]
pub async fn list_characters() -> Result<Vec<Character>, ServerFnError> {
    use crate::db::Db;
    use crate::services::character_service::CharacterService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || CharacterService::new(db).list())
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to list characters: {e}")))
}

#[server(GetCharacter, "/api")]
pub async fn get_character(id: Uuid) -> Result<Option<Character>, ServerFnError> {
    use crate::db::Db;
    use crate::services::character_service::CharacterService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || CharacterService::new(db).get(id))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to load character: {e}")))
}

#[server(CreateCharacter, "/api")]
pub async fn create_character(new: NewCharacter) -> Result<Character, ServerFnError> {
    use crate::db::Db;
    use crate::services::character_service::CharacterService;

    let name = new.name.trim();
    if name.is_empty() {
        return Err(ServerFnError::new("Character name is required"));
    }
    if name.chars().count() > 100 {
        return Err(ServerFnError::new("Character name is too long (max 100)"));
    }

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || CharacterService::new(db).create(&new))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to create character: {e}")))
}

#[server(UpdateCharacterFn, "/api")]
pub async fn update_character(update: UpdateCharacter) -> Result<Character, ServerFnError> {
    use crate::db::Db;
    use crate::services::character_service::CharacterService;

    if let Some(name) = &update.name {
        if name.trim().is_empty() {
            return Err(ServerFnError::new("Character name cannot be empty"));
        }
    }

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || CharacterService::new(db).update(&update))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to update character: {e}")))
}

#[server(DeleteCharacter, "/api")]
pub async fn delete_character(id: Uuid) -> Result<(), ServerFnError> {
    use crate::db::Db;
    use crate::services::character_service::CharacterService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || CharacterService::new(db).delete(id))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to delete character: {e}")))
}

/// Upload (or replace) a character's avatar. `data` is raw base64 (no data-URL
/// prefix). Returns the updated character with its new `avatar_path`.
#[server(UploadCharacterAvatar, "/api")]
pub async fn upload_character_avatar(
    id: Uuid,
    data: String,
    content_type: String,
) -> Result<Character, ServerFnError> {
    use crate::config;
    use crate::db::Db;
    use crate::services::character_service::CharacterService;
    use base64::Engine;

    // Validate MIME type and derive a file extension.
    let ext = match content_type.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        other => {
            return Err(ServerFnError::new(format!(
                "Unsupported image type: {other}"
            )))
        }
    };

    // Decode base64.
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|_| ServerFnError::new("Invalid image data"))?;

    // Size guard: 5 MB.
    if bytes.len() > 5 * 1024 * 1024 {
        return Err(ServerFnError::new("Image is too large (max 5 MB)"));
    }

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    let id_str = id.to_string();
    let rel_path = format!("avatars/{}.{}", id_str, ext);

    tokio::task::spawn_blocking(move || -> anyhow::Result<Character> {
        // Remove any previous avatar files for this id (different extensions).
        for e in ["png", "jpg", "webp", "gif"] {
            if let Ok(p) = config::avatar_path_ext(&id_str, e) {
                let _ = std::fs::remove_file(p);
            }
        }
        // Write the new file.
        let path = config::avatar_path_ext(&id_str, ext)?;
        std::fs::write(&path, &bytes)?;

        // Update DB.
        CharacterService::new(db).set_avatar_path(id, Some(rel_path))
    })
    .await
    .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
    .map_err(|e| ServerFnError::new(format!("Failed to save avatar: {e}")))
}

/// Remove a character's avatar, deleting the file and clearing `avatar_path`.
#[server(RemoveCharacterAvatar, "/api")]
pub async fn remove_character_avatar(id: Uuid) -> Result<Character, ServerFnError> {
    use crate::db::Db;
    use crate::services::character_service::CharacterService;

    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;

    tokio::task::spawn_blocking(move || CharacterService::new(db).set_avatar_path(id, None))
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to remove avatar: {e}")))
}
