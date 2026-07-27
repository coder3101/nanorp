//! Chat messages and image attachments.
//!
//! `content` stores raw markdown with `{{char}}`/`{{user}}` placeholders left
//! unresolved; rendering and placeholder resolution happen at display /
//! prompt-build time. Attachment files live on disk; the DB keeps metadata.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Maximum images that may be attached to a single message. Checked in the UI
/// for immediate feedback and re-checked server-side, which is the boundary
/// that actually enforces it.
pub const MAX_IMAGES_PER_MESSAGE: usize = 5;

/// Maximum decoded size of a single attached image.
pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// The role of a message sender, matching LLM API conventions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// System instructions (not shown to user in chat UI, but sent to LLM)
    System,
    /// User's message
    User,
    /// AI assistant's response
    Assistant,
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::System => write!(f, "system"),
            MessageRole::User => write!(f, "user"),
            MessageRole::Assistant => write!(f, "assistant"),
        }
    }
}

impl MessageRole {
    /// Parse from string (for DB round-tripping).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "system" => Some(Self::System),
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            _ => None,
        }
    }
}

/// An individual message in a chat session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub session_id: Uuid,
    pub role: MessageRole,
    /// Raw markdown content ({{char}} / {{user}} placeholders NOT resolved)
    pub content: String,
    /// Image attachments associated with this message (loaded from attachments table)
    pub attachments: Vec<Attachment>,
    /// The model that generated this message (None for user messages)
    pub model_used: Option<String>,
    /// The provider that generated this message (None for user messages)
    pub provider_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// An image attachment on a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: Uuid,
    pub message_id: Uuid,
    /// MIME type (e.g. "image/png", "image/jpeg", "image/webp", "image/gif")
    pub content_type: String,
    /// Relative path within config dir (e.g. "attachments/{uuid}.png")
    pub file_path: String,
    /// Original filename from the upload (for display purposes)
    pub original_name: Option<String>,
    /// File size in bytes
    pub file_size: u64,
    pub created_at: DateTime<Utc>,
}

/// Input for creating a new message. ID and timestamp generated server-side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMessage {
    pub session_id: Uuid,
    pub role: MessageRole,
    pub content: String,
    /// Base64-encoded image data to attach (processed server-side into files).
    /// Each entry is (base64_data, content_type, original_filename).
    /// Only used for user messages. Empty for assistant messages.
    pub image_attachments: Vec<ImageUpload>,
    pub model_used: Option<String>,
    pub provider_id: Option<Uuid>,
}

/// Image data uploaded from the client, before being saved to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUpload {
    /// Base64-encoded image data (without the "data:image/png;base64," prefix)
    pub data: String,
    /// MIME type (e.g. "image/png")
    pub content_type: String,
    /// Original filename if available
    pub original_name: Option<String>,
}

/// Payload emitted when a user message is edited in the UI.
#[derive(Debug, Clone)]
pub struct EditPayload {
    pub message_id: Uuid,
    pub content: String,
    /// Existing attachment ids to keep (others are removed).
    pub keep_attachment_ids: Vec<Uuid>,
    /// Newly added images to attach.
    pub new_images: Vec<ImageUpload>,
}

/// A message prepared for sending to the LLM API.
/// Placeholders are resolved, and content + images are ready for the provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: MessageRole,
    pub content: String,
    /// Base64-encoded images to include with this message (for vision models).
    /// Each entry is (base64_data, mime_type).
    /// Empty for text-only messages.
    pub images: Vec<LlmImage>,
}

/// An image prepared for inclusion in an LLM API request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmImage {
    /// Base64-encoded image data
    pub base64_data: String,
    /// MIME type (e.g. "image/png", "image/jpeg")
    pub content_type: String,
}
