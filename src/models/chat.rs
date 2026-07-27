//! Chat sessions and the denormalized summaries shown in the sidebar.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
