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

/// The schema the model is asked to fill when generating a character. Fields
/// mirror [`NewCharacter`]; all but `name` are optional so a model that omits
/// one still yields a usable draft for the user to review.
#[cfg(feature = "ssr")]
#[derive(Debug, Clone, serde::Deserialize)]
struct GeneratedCharacter {
    name: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    personality: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    greeting: Option<String>,
}

#[cfg(feature = "ssr")]
impl GeneratedCharacter {
    fn into_new(self) -> NewCharacter {
        fn clean(v: Option<String>) -> Option<String> {
            v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
        }
        NewCharacter {
            name: self.name.trim().to_string(),
            role: clean(self.role),
            personality: clean(self.personality),
            system_prompt: clean(self.system_prompt),
            greeting: clean(self.greeting),
        }
    }
}

/// Ask an LLM to design a character from a plain-text description. Uses the
/// provider's structured-output mode and returns a draft (`name` / `role` /
/// `personality` / `system_prompt` / `greeting`) that the caller can review —
/// it is NOT saved; the user must create it separately.
///
/// `provider_id`/`model` select which configured provider + model to talk to.
#[server(GenerateCharacter, "/api")]
pub async fn generate_character(
    provider_id: Uuid,
    model: String,
    description: String,
) -> Result<NewCharacter, ServerFnError> {
    use crate::db::Db;
    use crate::models::message::{LlmMessage, MessageRole};
    use crate::models::settings::SamplingParams;
    use crate::providers::registry::build_provider;

    let description = description.trim().to_string();
    if description.is_empty() {
        return Err(ServerFnError::new("Describe the character first"));
    }
    if model.trim().is_empty() {
        return Err(ServerFnError::new("Choose a model first"));
    }

    let system_prompt = include_str!("../prompts/character_generation.txt");

    let user_prompt = format!(
        "Design a roleplay character based on this description (if it mentions an existing \
character or lesson, incorporate it):\n\n{description}"
    );

    let messages = vec![
        LlmMessage {
            role: MessageRole::System,
            content: system_prompt.to_string(),
            images: Vec::new(),
        },
        LlmMessage {
            role: MessageRole::User,
            content: user_prompt,
            images: Vec::new(),
        },
    ];

    // Load the provider config off the async runtime.
    let db = use_context::<Db>().ok_or_else(|| ServerFnError::new("Database is not available"))?;
    let provider = {
        let db = db.clone();
        tokio::task::spawn_blocking(move || {
            use crate::services::provider_service::ProviderService;
            ProviderService::new(db).get(provider_id)
        })
        .await
        .map_err(|e| ServerFnError::new(format!("Task failed: {e}")))?
        .map_err(|e| ServerFnError::new(format!("Failed to load provider: {e}")))?
        .ok_or_else(|| ServerFnError::new("Provider not found"))?
    };

    let llm = build_provider(&provider);
    let raw = llm
        .chat_json(messages, &model, &SamplingParams::default())
        .await
        .map_err(|e| ServerFnError::new(format!("Generation failed: {e}")))?;

    let generated: GeneratedCharacter = serde_json::from_str(&raw)
        .map_err(|_| ServerFnError::new("The model's response wasn't valid character JSON"))?;

    if generated.name.trim().is_empty() {
        return Err(ServerFnError::new(
            "The model didn't provide a character name",
        ));
    }

    Ok(generated.into_new())
}
