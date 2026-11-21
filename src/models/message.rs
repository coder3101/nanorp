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

/// Maximum combined decoded size of all images on one message.
pub const MAX_TOTAL_IMAGE_BYTES: usize = MAX_IMAGES_PER_MESSAGE * MAX_IMAGE_BYTES;

/// Maximum decoded size of a character avatar.
pub const MAX_AVATAR_BYTES: usize = 5 * 1024 * 1024;

/// Smallest base64 string capacity that can hold `decoded_bytes`.
pub const fn max_base64_len(decoded_bytes: usize) -> usize {
    decoded_bytes.div_ceil(3) * 4
}

/// Base64 budget for one maximally-sized image.
pub const MAX_IMAGE_BASE64_BYTES: usize = max_base64_len(MAX_IMAGE_BYTES);

/// Combined base64 budget for a message's attachments. Each image is padded
/// independently, so this is the sum of per-image budgets rather than one
/// encoding of their concatenated bytes.
pub const MAX_TOTAL_IMAGE_BASE64_BYTES: usize = MAX_IMAGES_PER_MESSAGE * MAX_IMAGE_BASE64_BYTES;

/// HTTP body budget for chat requests containing base64 attachments. The extra
/// mebibyte covers JSON field names, message text, and attachment metadata.
pub const MAX_CHAT_REQUEST_BODY_BYTES: usize = MAX_TOTAL_IMAGE_BASE64_BYTES + 1024 * 1024;

/// Marks the final text-stream item as a generation control frame rather than
/// assistant content. The remainder of that item is a JSON `GenerationReceipt`.
pub const GENERATION_TERMINAL_PREFIX: &str = "\u{1e}nanorp-generation:";

/// Final outcome of a background generation. `persisted` reports whether the
/// accumulated assistant reply was saved; blank stopped replies therefore use
/// `persisted: false` without implying a persistence failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationReceipt {
    pub outcome: GenerationOutcome,
    pub persisted: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationOutcome {
    Completed,
    Stopped,
    Failed,
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_receipt_terminal_frame_is_prefix_and_json() {
        let receipt = GenerationReceipt {
            outcome: GenerationOutcome::Failed,
            persisted: true,
            error: Some("Provider disconnected".to_string()),
        };
        let frame = format!(
            "{GENERATION_TERMINAL_PREFIX}{}",
            serde_json::to_string(&receipt).unwrap()
        );
        let json = frame.strip_prefix(GENERATION_TERMINAL_PREFIX).unwrap();
        let decoded: GenerationReceipt = serde_json::from_str(json).unwrap();
        assert_eq!(decoded, receipt);
        assert!(json.contains("\"outcome\":\"failed\""));
        assert!(!json.contains("Completed"));
    }

    #[test]
    fn body_limit_accepts_five_maximum_base64_images() {
        assert_eq!(MAX_IMAGE_BASE64_BYTES, 13_981_016);
        assert_eq!(MAX_TOTAL_IMAGE_BASE64_BYTES, 69_905_080);
        assert_eq!(
            MAX_IMAGES_PER_MESSAGE * MAX_IMAGE_BASE64_BYTES,
            MAX_TOTAL_IMAGE_BASE64_BYTES
        );
        const {
            assert!(MAX_CHAT_REQUEST_BODY_BYTES > MAX_TOTAL_IMAGE_BASE64_BYTES);
            assert!(MAX_CHAT_REQUEST_BODY_BYTES <= MAX_TOTAL_IMAGE_BASE64_BYTES + 1024 * 1024);
        }
    }
}
