//! Chat sessions and the denormalized summaries shown in the sidebar.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::message::MessageRole;

/// Maximum length of a user-set conversation title, counted in characters.
pub const MAX_CHAT_TITLE_CHARS: usize = 100;

/// A chat session (conversation) with a character.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: Uuid,
    /// The character this conversation is with
    pub character_id: Uuid,
    /// Session title — auto-generated or user-set
    pub title: Option<String>,
    /// Preview of the last message (for sidebar display)
    pub last_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for creating a new chat session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewChatSession {
    pub character_id: Uuid,
    /// Optional title. If None, auto-generated from first user message.
    pub title: Option<String>,
}

/// Lightweight summary for sidebar display.
/// Joins ChatSession with Character data for rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSummary {
    pub session_id: Uuid,
    pub character_id: Uuid,
    pub character_name: String,
    pub character_avatar_path: Option<String>,
    pub title: Option<String>,
    pub last_message: Option<String>,
    pub updated_at: DateTime<Utc>,
}

/// File format for a portable, text-only conversation export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatExportFormat {
    Json,
    Markdown,
}

/// Text returned by the server and downloaded by the browser as one file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatExportFile {
    pub filename: String,
    pub content_type: String,
    pub content: String,
}

/// Stable JSON representation used by the versioned conversation export.
/// Attachment entries deliberately contain metadata, never image bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatExportDocument {
    pub schema_version: u32,
    pub exported_at: DateTime<Utc>,
    pub session_id: Uuid,
    pub title: Option<String>,
    pub character: ChatExportCharacter,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub messages: Vec<ChatExportMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatExportCharacter {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatExportMessage {
    pub id: Uuid,
    pub role: MessageRole,
    pub content: String,
    pub model_used: Option<String>,
    pub provider_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub attachments: Vec<ChatExportAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatExportAttachment {
    pub id: Uuid,
    pub content_type: String,
    pub original_name: Option<String>,
    pub file_size: u64,
    pub created_at: DateTime<Utc>,
}
