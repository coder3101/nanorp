//! Chat sessions, messages, image attachments, and LLM prompt building.

use anyhow::{Context, Result};
use base64::Engine;
use chrono::Utc;
use uuid::Uuid;

use crate::config;
use crate::db::Db;
use crate::models::character::Character;
use crate::models::chat::{ChatSession, ChatSummary, NewChatSession};
use crate::models::message::{
    Attachment, ImageUpload, LlmImage, LlmMessage, Message, MessageRole, NewMessage,
};

pub struct ChatService {
    db: Db,
}

impl ChatService {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    // ---- Sessions --------------------------------------------------------

    /// Create a session. If the character has a greeting, insert it as the
    /// first assistant message.
    pub fn create_session(
        &self,
        new: &NewChatSession,
        greeting: Option<String>,
    ) -> Result<ChatSession> {
        let now = Utc::now();
        let session = ChatSession {
            id: Uuid::new_v4(),
            character_id: new.character_id,
            title: new.title.clone(),
            last_message: greeting.clone(),
            created_at: now,
            updated_at: now,
        };

        let conn = self.db.conn();
        let mut conn = conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO chat_sessions (id, character_id, title, last_message, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                session.id.to_string(),
                session.character_id.to_string(),
                session.title,
                session.last_message,
                session.created_at.to_rfc3339(),
                session.updated_at.to_rfc3339(),
            ],
        )
        .context("insert chat session")?;

        if let Some(greeting) = greeting.filter(|g| !g.trim().is_empty()) {
            tx.execute(
                "INSERT INTO messages (id, session_id, role, content, model_used, provider_id, created_at)
                 VALUES (?1, ?2, 'assistant', ?3, NULL, NULL, ?4)",
                rusqlite::params![
                    Uuid::new_v4().to_string(),
                    session.id.to_string(),
                    greeting,
                    now.to_rfc3339(),
                ],
            )
            .context("insert greeting message")?;
        }

        tx.commit()?;
        Ok(session)
    }

    pub fn get_session(&self, id: Uuid) -> Result<Option<ChatSession>> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, character_id, title, last_message, created_at, updated_at
             FROM chat_sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_session(row)?)),
            None => Ok(None),
        }
    }

    /// Newest-first page of conversations, optionally narrowed to one character
    /// and/or to a free-text `query` matched against the character name, the
    /// session title, and the last-message preview.
    pub fn list_sessions(
        &self,
        character_id: Option<Uuid>,
        query: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ChatSummary>> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        let char_filter = character_id.map(|c| c.to_string());
        let search = query
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(like_pattern);
        let mut stmt = conn.prepare(
            "SELECT s.id, s.character_id, c.name, c.avatar_path, s.title, s.last_message, s.updated_at
             FROM chat_sessions s
             JOIN characters c ON s.character_id = c.id
             WHERE (?1 IS NULL OR s.character_id = ?1)
               AND (?2 IS NULL
                    OR c.name LIKE ?2 ESCAPE '\\'
                    OR s.title LIKE ?2 ESCAPE '\\'
                    OR s.last_message LIKE ?2 ESCAPE '\\')
             ORDER BY s.updated_at DESC
             LIMIT ?3 OFFSET ?4",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![char_filter, search, limit, offset],
            |row| {
                Ok(ChatSummary {
                    session_id: parse_uuid(row.get::<_, String>(0)?)?,
                    character_id: parse_uuid(row.get::<_, String>(1)?)?,
                    character_name: row.get(2)?,
                    character_avatar_path: row.get(3)?,
                    title: row.get(4)?,
                    last_message: row.get(5)?,
                    updated_at: parse_dt(row.get::<_, String>(6)?)?,
                })
            },
        )?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn delete_session(&self, id: Uuid) -> Result<()> {
        // Collect attachment files first (CASCADE will remove the DB rows).
        let files = self.attachment_paths_for_session(id)?;
        {
            let conn = self.db.conn();
            let conn = conn.lock().expect("db mutex poisoned");
            conn.execute(
                "DELETE FROM chat_sessions WHERE id = ?1",
                rusqlite::params![id.to_string()],
            )
            .context("delete chat session")?;
        }
        for rel in files {
            remove_file(&rel);
        }
        Ok(())
    }

    fn attachment_paths_for_session(&self, session_id: Uuid) -> Result<Vec<String>> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT a.file_path FROM attachments a
             JOIN messages m ON a.message_id = m.id
             WHERE m.session_id = ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![session_id.to_string()], |row| {
            row.get::<_, String>(0)
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    fn update_session_preview(
        conn: &rusqlite::Connection,
        session_id: Uuid,
        last_message: &str,
    ) -> Result<()> {
        let preview: String = last_message.chars().take(120).collect();
        conn.execute(
            "UPDATE chat_sessions SET last_message = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![session_id.to_string(), preview, Utc::now().to_rfc3339()],
        )
        .context("update session preview")?;
        Ok(())
    }

    // ---- Messages --------------------------------------------------------

    /// Insert a message, persisting any base64 image attachments to disk.
    pub fn add_message(&self, new: &NewMessage) -> Result<Message> {
        if new.image_attachments.len() > MAX_IMAGES_PER_MESSAGE {
            anyhow::bail!(
                "too many attachments: {} (max {})",
                new.image_attachments.len(),
                MAX_IMAGES_PER_MESSAGE
            );
        }

        let msg_id = Uuid::new_v4();
        let now = Utc::now();

        // Write attachment files (outside the DB lock).
        let mut attachments: Vec<Attachment> = Vec::new();
        for img in &new.image_attachments {
            attachments.push(save_image_upload(img, msg_id, now)?);
        }

        let conn = self.db.conn();
        let mut conn = conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;

        tx.execute(
            "INSERT INTO messages (id, session_id, role, content, model_used, provider_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                msg_id.to_string(),
                new.session_id.to_string(),
                new.role.to_string(),
                new.content,
                new.model_used,
                new.provider_id.map(|p| p.to_string()),
                now.to_rfc3339(),
            ],
        )
        .context("insert message")?;

        for att in &attachments {
            tx.execute(
                "INSERT INTO attachments (id, message_id, content_type, file_path, original_name, file_size, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    att.id.to_string(),
                    att.message_id.to_string(),
                    att.content_type,
                    att.file_path,
                    att.original_name,
                    att.file_size as i64,
                    att.created_at.to_rfc3339(),
                ],
            )
            .context("insert attachment")?;
        }

        // Update sidebar preview (skip for system messages).
        if new.role != MessageRole::System && !new.content.trim().is_empty() {
            Self::update_session_preview(&tx, new.session_id, &new.content)?;
        }

        // Auto-title from the first user message if none set.
        if new.role == MessageRole::User {
            let title: String = new.content.chars().take(50).collect();
            tx.execute(
                "UPDATE chat_sessions SET title = ?2 WHERE id = ?1 AND (title IS NULL OR title = '')",
                rusqlite::params![new.session_id.to_string(), title],
            )
            .context("auto-title session")?;
        }

        tx.commit()?;

        Ok(Message {
            id: msg_id,
            session_id: new.session_id,
            role: new.role.clone(),
            content: new.content.clone(),
            attachments,
            model_used: new.model_used.clone(),
            provider_id: new.provider_id,
            created_at: now,
        })
    }

    /// Fetch a single message (without attachments).
    pub fn get_message(&self, message_id: Uuid) -> Result<Option<Message>> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, model_used, provider_id, created_at
             FROM messages WHERE id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![message_id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(Message {
                id: parse_uuid(row.get::<_, String>(0)?)?,
                session_id: parse_uuid(row.get::<_, String>(1)?)?,
                role: MessageRole::parse(&row.get::<_, String>(2)?)
                    .unwrap_or(MessageRole::Assistant),
                content: row.get(3)?,
                attachments: Vec::new(),
                model_used: row.get(4)?,
                provider_id: row
                    .get::<_, Option<String>>(5)?
                    .and_then(|s| s.parse().ok()),
                created_at: parse_dt(row.get::<_, String>(6)?)?,
            })),
            None => Ok(None),
        }
    }

    /// Update a message's text content (used when editing a user message).
    pub fn update_message_content(&self, message_id: Uuid, content: &str) -> Result<()> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        conn.execute(
            "UPDATE messages SET content = ?2 WHERE id = ?1",
            rusqlite::params![message_id.to_string(), content],
        )
        .context("update message content")?;
        Ok(())
    }

    /// Reconcile a message's attachments to match an edit: remove any current
    /// attachments not in `keep_ids` (deleting their files), and add `new_images`.
    pub fn replace_message_attachments(
        &self,
        message_id: Uuid,
        keep_ids: &[Uuid],
        new_images: &[ImageUpload],
    ) -> Result<()> {
        // 1. Load current attachments for the message.
        let current: Vec<(Uuid, String)> = {
            let conn = self.db.conn();
            let conn = conn.lock().expect("db mutex poisoned");
            let mut stmt =
                conn.prepare("SELECT id, file_path FROM attachments WHERE message_id = ?1")?;
            let rows = stmt.query_map(rusqlite::params![message_id.to_string()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut out = Vec::new();
            for r in rows {
                let (id, path) = r?;
                if let Ok(id) = id.parse::<Uuid>() {
                    out.push((id, path));
                }
            }
            out
        };

        // Enforce the per-message cap before mutating anything, so a rejected
        // edit leaves the existing attachments untouched.
        let kept_count = current
            .iter()
            .filter(|(id, _)| keep_ids.contains(id))
            .count();
        if kept_count + new_images.len() > MAX_IMAGES_PER_MESSAGE {
            anyhow::bail!(
                "too many attachments: {} (max {})",
                kept_count + new_images.len(),
                MAX_IMAGES_PER_MESSAGE
            );
        }

        // 2. Delete the ones no longer kept (DB rows + files).
        let mut removed_files = Vec::new();
        {
            let conn = self.db.conn();
            let conn = conn.lock().expect("db mutex poisoned");
            for (id, path) in &current {
                if !keep_ids.contains(id) {
                    conn.execute(
                        "DELETE FROM attachments WHERE id = ?1",
                        rusqlite::params![id.to_string()],
                    )
                    .context("delete attachment")?;
                    removed_files.push(path.clone());
                }
            }
        }
        for rel in removed_files {
            remove_file(&rel);
        }

        // 3. Add the newly-uploaded images.
        let now = Utc::now();
        for img in new_images {
            let att = save_image_upload(img, message_id, now)?;

            let conn = self.db.conn();
            let conn = conn.lock().expect("db mutex poisoned");
            conn.execute(
                "INSERT INTO attachments (id, message_id, content_type, file_path, original_name, file_size, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    att.id.to_string(),
                    att.message_id.to_string(),
                    att.content_type,
                    att.file_path,
                    att.original_name,
                    att.file_size as i64,
                    att.created_at.to_rfc3339(),
                ],
            )
            .context("insert attachment")?;
        }

        Ok(())
    }

    /// Delete a single message and its attachment files.
    pub fn delete_message(&self, message_id: Uuid) -> Result<()> {
        let files = self.attachment_paths_for_message(message_id)?;
        {
            let conn = self.db.conn();
            let conn = conn.lock().expect("db mutex poisoned");
            conn.execute(
                "DELETE FROM messages WHERE id = ?1",
                rusqlite::params![message_id.to_string()],
            )
            .context("delete message")?;
        }
        for rel in files {
            remove_file(&rel);
        }
        Ok(())
    }

    /// Delete all messages in a session created strictly after the given
    /// message (by timestamp, tie-broken by id). Used for edit + regenerate.
    /// Returns the number of deleted rows.
    pub fn delete_messages_after(&self, session_id: Uuid, message_id: Uuid) -> Result<usize> {
        // Find the reference message's timestamp.
        let anchor = self
            .get_message(message_id)?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?;

        // Everything created strictly after the anchor gets removed.
        let to_delete: Vec<Uuid> = {
            let conn = self.db.conn();
            let conn = conn.lock().expect("db mutex poisoned");
            let mut stmt =
                conn.prepare("SELECT id FROM messages WHERE session_id = ?1 AND created_at > ?2")?;
            let rows = stmt.query_map(
                rusqlite::params![session_id.to_string(), anchor.created_at.to_rfc3339()],
                |row| row.get::<_, String>(0),
            )?;
            let mut ids = Vec::new();
            for r in rows {
                if let Ok(id) = r?.parse() {
                    ids.push(id);
                }
            }
            ids
        };

        let mut count = 0;
        for id in to_delete {
            self.delete_message(id)?;
            count += 1;
        }
        Ok(count)
    }

    fn attachment_paths_for_message(&self, message_id: Uuid) -> Result<Vec<String>> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare("SELECT file_path FROM attachments WHERE message_id = ?1")?;
        let rows = stmt.query_map(rusqlite::params![message_id.to_string()], |row| {
            row.get::<_, String>(0)
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn list_messages(&self, session_id: Uuid) -> Result<Vec<Message>> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");

        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, model_used, provider_id, created_at
             FROM messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let msg_rows = stmt.query_map(rusqlite::params![session_id.to_string()], |row| {
            Ok(Message {
                id: parse_uuid(row.get::<_, String>(0)?)?,
                session_id: parse_uuid(row.get::<_, String>(1)?)?,
                role: MessageRole::parse(&row.get::<_, String>(2)?)
                    .unwrap_or(MessageRole::Assistant),
                content: row.get(3)?,
                attachments: Vec::new(),
                model_used: row.get(4)?,
                provider_id: row
                    .get::<_, Option<String>>(5)?
                    .and_then(|s| s.parse().ok()),
                created_at: parse_dt(row.get::<_, String>(6)?)?,
            })
        })?;
        let mut messages: Vec<Message> = Vec::new();
        for r in msg_rows {
            messages.push(r?);
        }

        // Load all attachments for the session and group them by message.
        let mut att_stmt = conn.prepare(
            "SELECT a.id, a.message_id, a.content_type, a.file_path, a.original_name, a.file_size, a.created_at
             FROM attachments a
             JOIN messages m ON a.message_id = m.id
             WHERE m.session_id = ?1
             ORDER BY a.created_at ASC",
        )?;
        let att_rows = att_stmt.query_map(rusqlite::params![session_id.to_string()], |row| {
            Ok(Attachment {
                id: parse_uuid(row.get::<_, String>(0)?)?,
                message_id: parse_uuid(row.get::<_, String>(1)?)?,
                content_type: row.get(2)?,
                file_path: row.get(3)?,
                original_name: row.get(4)?,
                file_size: row.get::<_, i64>(5)? as u64,
                created_at: parse_dt(row.get::<_, String>(6)?)?,
            })
        })?;
        for r in att_rows {
            let att = r?;
            if let Some(m) = messages.iter_mut().find(|m| m.id == att.message_id) {
                m.attachments.push(att);
            }
        }

        Ok(messages)
    }

    // ---- Prompt building -------------------------------------------------

    /// Build the LLM prompt from the session history: a composed system prompt
    /// followed by the conversation. Images are loaded from disk into base64.
    pub fn build_prompt(
        &self,
        session_id: Uuid,
        character: &Character,
        user_name: &str,
        default_system_prompt: &str,
    ) -> Result<Vec<LlmMessage>> {
        let system = build_system_prompt(character, user_name, default_system_prompt);

        let mut out = vec![LlmMessage {
            role: MessageRole::System,
            content: system,
            images: Vec::new(),
        }];

        for msg in self.list_messages(session_id)? {
            // Skip any persisted system messages — the system prompt is rebuilt.
            if msg.role == MessageRole::System {
                continue;
            }
            let content = resolve_placeholders(&msg.content, &character.name, user_name);
            let images = load_images(&msg.attachments)?;
            out.push(LlmMessage {
                role: msg.role,
                content,
                images,
            });
        }

        Ok(out)
    }
}

// ---- helpers -------------------------------------------------------------

/// Compose the full system message from all of a character's fields so the
/// model always receives its name, role, and personality — not just whatever
/// happens to be in the free-form `system_prompt`.
///
/// Layout:
///   <base system prompt (character's or the app default)>
///
///   Name: <name>
///   Role: <role>
///
///   Personality:
///   <personality>
///
/// Placeholders ({{char}}, {{user}}, <BOT>, <USER>) are resolved throughout.
fn build_system_prompt(
    character: &Character,
    user_name: &str,
    default_system_prompt: &str,
) -> String {
    let base = character
        .system_prompt
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(default_system_prompt);

    let mut sections: Vec<String> = vec![base.trim().to_string()];

    // A small "character card" block that always carries the structured fields.
    let mut card = format!("Name: {}", character.name);
    if let Some(role) = character
        .role
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        card.push_str(&format!("\nRole: {role}"));
    }
    sections.push(card);

    if let Some(personality) = character
        .personality
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sections.push(format!("Personality:\n{personality}"));
    }

    let composed = sections.join("\n\n");
    resolve_placeholders(&composed, &character.name, user_name)
}

fn resolve_placeholders(text: &str, char_name: &str, user_name: &str) -> String {
    text.replace("{{char}}", char_name)
        .replace("{{user}}", user_name)
        .replace("<BOT>", char_name)
        .replace("<bot>", char_name)
        .replace("<USER>", user_name)
        .replace("<user>", user_name)
}

fn load_images(attachments: &[Attachment]) -> Result<Vec<LlmImage>> {
    let mut images = Vec::new();
    for att in attachments {
        let full = config::config_dir()?.join(&att.file_path);
        let bytes =
            std::fs::read(&full).with_context(|| format!("read attachment {}", att.file_path))?;
        images.push(LlmImage {
            base64_data: base64::engine::general_purpose::STANDARD.encode(&bytes),
            content_type: att.content_type.clone(),
        });
    }
    Ok(images)
}

use crate::models::message::{MAX_IMAGES_PER_MESSAGE, MAX_IMAGE_BYTES};

/// Map a supported image MIME type to a file extension. Returns `None` for
/// anything that isn't a recognized image type (upload is rejected).
fn mime_to_extension(content_type: &str) -> Option<&'static str> {
    match content_type {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

/// Validate an uploaded image (MIME type, decoded size) and write it to the
/// attachments directory. Returns the attachment metadata for DB insertion.
fn save_image_upload(
    img: &ImageUpload,
    message_id: Uuid,
    now: chrono::DateTime<Utc>,
) -> Result<Attachment> {
    let ext = mime_to_extension(&img.content_type)
        .ok_or_else(|| anyhow::anyhow!("unsupported attachment type: {}", img.content_type))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(img.data.as_bytes())
        .context("decode attachment base64")?;
    if bytes.len() > MAX_IMAGE_BYTES {
        anyhow::bail!(
            "image too large: {} bytes (max {} bytes)",
            bytes.len(),
            MAX_IMAGE_BYTES
        );
    }

    let att_id = Uuid::new_v4();
    let filename = format!("{att_id}.{ext}");
    let full = config::attachment_path(&filename)?;
    std::fs::write(&full, &bytes).context("write attachment file")?;

    Ok(Attachment {
        id: att_id,
        message_id,
        content_type: img.content_type.clone(),
        file_path: format!("attachments/{filename}"),
        original_name: img.original_name.clone(),
        file_size: bytes.len() as u64,
        created_at: now,
    })
}

fn remove_file(rel: &str) {
    if let Ok(dir) = config::config_dir() {
        let path = dir.join(rel);
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!("failed to remove attachment {}: {e}", path.display());
            }
        }
    }
}

fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<ChatSession> {
    Ok(ChatSession {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        character_id: parse_uuid(row.get::<_, String>(1)?)?,
        title: row.get(2)?,
        last_message: row.get(3)?,
        created_at: parse_dt(row.get::<_, String>(4)?)?,
        updated_at: parse_dt(row.get::<_, String>(5)?)?,
    })
}

fn parse_uuid(s: String) -> rusqlite::Result<Uuid> {
    s.parse().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            "invalid uuid".into(),
        )
    })
}

/// Wraps a search term in `%` for a substring `LIKE`, escaping the characters
/// SQL treats as wildcards so that typing `%` or `_` searches for that
/// character instead of matching everything. Pair with `ESCAPE '\'`.
fn like_pattern(query: &str) -> String {
    let mut pattern = String::with_capacity(query.len() + 2);
    pattern.push('%');
    for ch in query.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern.push('%');
    pattern
}

fn parse_dt(s: String) -> rusqlite::Result<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                "invalid timestamp".into(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::character::NewCharacter;
    use crate::services::character_service::CharacterService;

    fn test_db() -> Db {
        let db = Db::open_in_memory().expect("open in-memory db");
        db.run_migrations().expect("run migrations");
        db
    }

    fn make_character(db: &Db, greeting: Option<&str>) -> Character {
        CharacterService::new(db.clone())
            .create(&NewCharacter {
                name: "Luna".to_string(),
                role: Some("A wise old wizard".to_string()),
                personality: Some("Patient and curious".to_string()),
                system_prompt: Some("You are {{char}}, talking to {{user}}.".to_string()),
                greeting: greeting.map(str::to_string),
            })
            .expect("create character")
    }

    fn user_message(session_id: Uuid, content: &str) -> NewMessage {
        NewMessage {
            session_id,
            role: MessageRole::User,
            content: content.to_string(),
            image_attachments: Vec::new(),
            model_used: None,
            provider_id: None,
        }
    }

    fn make_named_character(db: &Db, name: &str) -> Character {
        CharacterService::new(db.clone())
            .create(&NewCharacter {
                name: name.to_string(),
                role: None,
                personality: None,
                system_prompt: None,
                greeting: None,
            })
            .expect("create character")
    }

    fn new_session(chat: &ChatService, character_id: Uuid, title: &str) -> ChatSession {
        chat.create_session(
            &NewChatSession {
                character_id,
                title: Some(title.to_string()),
            },
            None,
        )
        .expect("create session")
    }

    #[test]
    fn list_sessions_pages_without_repeating_rows() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db);
        for i in 0..3 {
            new_session(&chat, character.id, &format!("Session {i}"));
        }

        let first = chat.list_sessions(None, None, 2, 0).unwrap();
        let second = chat.list_sessions(None, None, 2, 2).unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 1);

        let mut ids: Vec<_> = first
            .iter()
            .chain(&second)
            .map(|s| s.session_id)
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 3, "pages must not repeat a session");
    }

    #[test]
    fn list_sessions_searches_title_and_character_name() {
        let db = test_db();
        let chat = ChatService::new(db.clone());
        // `make_character` names its character "Luna".
        new_session(&chat, make_character(&db, None).id, "Dragon hunt");
        new_session(&chat, make_named_character(&db, "Borgnine").id, "Tea party");

        let hits = chat.list_sessions(None, Some("dragon"), 50, 0).unwrap();
        assert_eq!(hits.len(), 1, "matches on title, case-insensitively");
        assert_eq!(hits[0].title.as_deref(), Some("Dragon hunt"));

        let hits = chat.list_sessions(None, Some("borg"), 50, 0).unwrap();
        assert_eq!(hits.len(), 1, "matches on character name");
        assert_eq!(hits[0].character_name, "Borgnine");

        assert_eq!(
            chat.list_sessions(None, Some("   "), 50, 0).unwrap().len(),
            2,
            "a blank query is not a filter"
        );
        assert!(chat
            .list_sessions(None, Some("nonexistent"), 50, 0)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn list_sessions_search_treats_wildcards_literally() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db);
        new_session(&chat, character.id, "100% mine");
        new_session(&chat, character.id, "plain");

        // Unescaped, this pattern would match every row.
        let hits = chat.list_sessions(None, Some("%"), 50, 0).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title.as_deref(), Some("100% mine"));
    }

    #[test]
    fn create_session_inserts_greeting_as_first_message() {
        let db = test_db();
        let character = make_character(&db, Some("Hello, traveler!"));
        let chat = ChatService::new(db);

        let session = chat
            .create_session(
                &NewChatSession {
                    character_id: character.id,
                    title: None,
                },
                character.greeting.clone(),
            )
            .unwrap();

        let messages = chat.list_messages(session.id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, MessageRole::Assistant);
        assert_eq!(messages[0].content, "Hello, traveler!");
    }

    #[test]
    fn create_session_skips_blank_greeting() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db);

        let session = chat
            .create_session(
                &NewChatSession {
                    character_id: character.id,
                    title: None,
                },
                Some("   ".to_string()),
            )
            .unwrap();

        assert!(chat.list_messages(session.id).unwrap().is_empty());
    }

    #[test]
    fn first_user_message_auto_titles_session() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db);
        let session = chat
            .create_session(
                &NewChatSession {
                    character_id: character.id,
                    title: None,
                },
                None,
            )
            .unwrap();

        chat.add_message(&user_message(session.id, "Tell me a story about dragons"))
            .unwrap();

        let reloaded = chat.get_session(session.id).unwrap().unwrap();
        assert_eq!(
            reloaded.title.as_deref(),
            Some("Tell me a story about dragons")
        );
        assert_eq!(
            reloaded.last_message.as_deref(),
            Some("Tell me a story about dragons")
        );
    }

    #[test]
    fn delete_messages_after_removes_trailing_messages() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db);
        let session = chat
            .create_session(
                &NewChatSession {
                    character_id: character.id,
                    title: None,
                },
                None,
            )
            .unwrap();

        // Timestamps need to be strictly increasing for the "after" cutoff.
        let first = chat
            .add_message(&user_message(session.id, "first"))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        chat.add_message(&user_message(session.id, "second"))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        chat.add_message(&user_message(session.id, "third"))
            .unwrap();

        let deleted = chat.delete_messages_after(session.id, first.id).unwrap();
        assert_eq!(deleted, 2);

        let remaining = chat.list_messages(session.id).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].content, "first");
    }

    #[test]
    fn delete_session_cascades_messages() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db.clone());
        let session = chat
            .create_session(
                &NewChatSession {
                    character_id: character.id,
                    title: None,
                },
                None,
            )
            .unwrap();
        chat.add_message(&user_message(session.id, "hello"))
            .unwrap();

        chat.delete_session(session.id).unwrap();

        assert!(chat.get_session(session.id).unwrap().is_none());
        assert!(chat.list_messages(session.id).unwrap().is_empty());
    }

    #[test]
    fn build_prompt_resolves_placeholders_and_skips_system_messages() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db);
        let session = chat
            .create_session(
                &NewChatSession {
                    character_id: character.id,
                    title: None,
                },
                None,
            )
            .unwrap();
        chat.add_message(&user_message(session.id, "Hi {{char}}, I'm {{user}}."))
            .unwrap();

        let prompt = chat
            .build_prompt(session.id, &character, "Sam", "Default prompt.")
            .unwrap();

        assert_eq!(prompt.len(), 2);
        assert_eq!(prompt[0].role, MessageRole::System);
        assert!(prompt[0].content.contains("You are Luna, talking to Sam."));
        assert!(prompt[0].content.contains("Name: Luna"));
        assert!(prompt[0]
            .content
            .contains("Personality:\nPatient and curious"));
        assert_eq!(prompt[1].content, "Hi Luna, I'm Sam.");
    }

    #[test]
    fn system_prompt_falls_back_to_default() {
        let character = Character {
            id: Uuid::new_v4(),
            name: "Rex".to_string(),
            role: None,
            personality: None,
            system_prompt: None,
            greeting: None,
            avatar_path: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let composed = build_system_prompt(&character, "Sam", "Be helpful, {{user}}.");
        assert!(composed.starts_with("Be helpful, Sam."));
        assert!(composed.contains("Name: Rex"));
    }

    #[test]
    fn resolve_placeholders_handles_all_variants() {
        let out = resolve_placeholders("{{char}} <BOT> <bot> / {{user}} <USER> <user>", "C", "U");
        assert_eq!(out, "C C C / U U U");
    }

    #[test]
    fn unsupported_mime_types_are_rejected() {
        assert_eq!(mime_to_extension("image/png"), Some("png"));
        assert_eq!(mime_to_extension("image/jpeg"), Some("jpg"));
        assert_eq!(mime_to_extension("application/pdf"), None);
        assert_eq!(mime_to_extension("text/html"), None);

        let img = ImageUpload {
            data: String::new(),
            content_type: "application/x-msdownload".to_string(),
            original_name: Some("evil.exe".to_string()),
        };
        let err = save_image_upload(&img, Uuid::new_v4(), Utc::now()).unwrap_err();
        assert!(err.to_string().contains("unsupported attachment type"));
    }
}
