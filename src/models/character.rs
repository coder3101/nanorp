//! Roleplay characters: the persona the model embodies during a chat.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A roleplay character that the AI embodies during chat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Character {
    pub id: Uuid,
    pub name: String,
    /// Brief role description (e.g. "A wise old wizard")
    pub role: Option<String>,
    /// Detailed personality traits
    pub personality: Option<String>,
    /// System prompt sent to the LLM. Supports {{char}} and {{user}} placeholders.
    pub system_prompt: Option<String>,
    /// Character's first message in a new chat session
    pub greeting: Option<String>,
    /// Relative path to avatar image (e.g. "avatars/{id}.webp"), None for default avatar
    pub avatar_path: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for creating a new character. ID and timestamps generated server-side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewCharacter {
    pub name: String,
    pub role: Option<String>,
    pub personality: Option<String>,
    pub system_prompt: Option<String>,
    pub greeting: Option<String>,
    // Avatar is handled separately via file upload endpoint, not through this struct.
}

/// Input for updating a character.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCharacter {
    pub id: Uuid,
    pub name: Option<String>,
    pub role: Option<String>,
    pub personality: Option<String>,
    pub system_prompt: Option<String>,
    pub greeting: Option<String>,
    // Avatar updates handled separately.
}
