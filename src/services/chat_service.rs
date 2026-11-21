//! Chat sessions, messages, image attachments, and LLM prompt building.

use anyhow::{Context, Result};
use base64::Engine;
use chrono::Utc;
use uuid::Uuid;

use crate::config;
use crate::db::Db;
use crate::models::character::Character;
use crate::models::chat::{
    ChatExportAttachment, ChatExportCharacter, ChatExportDocument, ChatExportFile,
    ChatExportFormat, ChatExportMessage, ChatSession, ChatSummary, NewChatSession,
    MAX_CHAT_TITLE_CHARS,
};
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
                "INSERT INTO messages
                 (id, session_id, role, content, model_used, provider_id, created_at, sequence)
                 VALUES (?1, ?2, 'assistant', ?3, NULL, NULL, ?4,
                         (SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE session_id = ?2))",
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

    /// Set a conversation title after trimming and validating user input.
    pub fn rename_session(&self, id: Uuid, title: &str) -> Result<ChatSession> {
        let title = title.trim();
        if title.is_empty() {
            anyhow::bail!("Conversation title cannot be empty");
        }
        if title.chars().count() > MAX_CHAT_TITLE_CHARS {
            anyhow::bail!("Conversation title cannot exceed {MAX_CHAT_TITLE_CHARS} characters");
        }

        let now = Utc::now();
        {
            let conn = self.db.conn();
            let conn = conn.lock().expect("db mutex poisoned");
            let updated = conn
                .execute(
                    "UPDATE chat_sessions SET title = ?2, updated_at = ?3 WHERE id = ?1",
                    rusqlite::params![id.to_string(), title, now.to_rfc3339()],
                )
                .context("rename chat session")?;
            if updated == 0 {
                anyhow::bail!("Chat session not found");
            }
        }

        self.get_session(id)?
            .ok_or_else(|| anyhow::anyhow!("Chat session not found"))
    }

    /// Export a complete conversation without reading attachment files. Both
    /// formats carry only attachment metadata, so missing or large images never
    /// become part of the download.
    pub fn export_session(
        &self,
        session_id: Uuid,
        character: &Character,
        format: ChatExportFormat,
    ) -> Result<ChatExportFile> {
        let session = self
            .get_session(session_id)?
            .ok_or_else(|| anyhow::anyhow!("Chat session not found"))?;
        if session.character_id != character.id {
            anyhow::bail!("Chat session does not belong to this character");
        }
        let messages = self.list_messages(session_id)?;
        let document = ChatExportDocument {
            schema_version: 1,
            exported_at: Utc::now(),
            session_id: session.id,
            title: session.title.clone(),
            character: ChatExportCharacter {
                id: character.id,
                name: character.name.clone(),
            },
            created_at: session.created_at,
            updated_at: session.updated_at,
            messages: messages
                .into_iter()
                .map(|message| ChatExportMessage {
                    id: message.id,
                    role: message.role,
                    content: message.content,
                    model_used: message.model_used,
                    provider_id: message.provider_id,
                    created_at: message.created_at,
                    attachments: message
                        .attachments
                        .into_iter()
                        .map(|attachment| ChatExportAttachment {
                            id: attachment.id,
                            content_type: attachment.content_type,
                            original_name: attachment.original_name,
                            file_size: attachment.file_size,
                            created_at: attachment.created_at,
                        })
                        .collect(),
                })
                .collect(),
        };

        let title = session
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .unwrap_or(&character.name);
        let stem = export_filename_stem(title);
        match format {
            ChatExportFormat::Json => {
                let mut content =
                    serde_json::to_string_pretty(&document).context("serialize chat export")?;
                content.push('\n');
                Ok(ChatExportFile {
                    filename: format!("{stem}.json"),
                    content_type: "application/json; charset=utf-8".to_string(),
                    content,
                })
            }
            ChatExportFormat::Markdown => Ok(ChatExportFile {
                filename: format!("{stem}.md"),
                content_type: "text/markdown; charset=utf-8".to_string(),
                content: export_markdown(&document),
            }),
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
                    last_message: row.get::<_, Option<String>>(5)?.map(|m| {
                        // Defensive: clean previews stored before reasoning was
                        // stripped at write time.
                        strip_reasoning(&m).to_string()
                    }),
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
            remove_attachment_file(&rel);
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
        let preview: String = strip_reasoning(last_message).chars().take(120).collect();
        conn.execute(
            "UPDATE chat_sessions SET last_message = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![session_id.to_string(), preview, Utc::now().to_rfc3339()],
        )
        .context("update session preview")?;
        Ok(())
    }

    // ---- Messages --------------------------------------------------------

    /// Insert a message, persisting any base64 image attachments to disk.
    /// New files are staged outside the database lock and removed if any
    /// filesystem or database operation fails.
    pub fn add_message(&self, new: &NewMessage) -> Result<Message> {
        if new.role != MessageRole::User && !new.image_attachments.is_empty() {
            anyhow::bail!("only user messages may have attachments");
        }
        let decoded = validate_image_uploads(&new.image_attachments)?;
        let msg_id = Uuid::new_v4();
        let now = Utc::now();
        let mut pending = PendingAttachments::stage(decoded, msg_id, now)?;
        let attachments = pending.attachments();

        let conn = self.db.conn();
        let mut conn = conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        let write_result = (|| -> Result<()> {
            tx.execute(
                "INSERT INTO messages
                 (id, session_id, role, content, model_used, provider_id, created_at, sequence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
                         (SELECT COALESCE(MAX(sequence), 0) + 1 FROM messages WHERE session_id = ?2))",
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
                insert_attachment(&tx, att)?;
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
            pending.activate()?;
            Ok(())
        })();

        if let Err(error) = write_result {
            drop(tx);
            return Err(error);
        }
        tx.commit()?;
        pending.persisted();

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

    /// Update a message's text content. Editing a user message together with
    /// attachment/truncation changes should use [`Self::edit_user_message`].
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

    /// Reconcile attachments without truncating later messages. The HTTP edit
    /// endpoint uses `edit_user_message`, which performs both operations in one
    /// transaction; this method retains the existing service API for callers
    /// that only need attachment reconciliation.
    pub fn replace_message_attachments(
        &self,
        message_id: Uuid,
        keep_ids: &[Uuid],
        new_images: &[ImageUpload],
    ) -> Result<()> {
        let content = self
            .get_message(message_id)?
            .ok_or_else(|| anyhow::anyhow!("Message not found"))?
            .content;
        self.edit_message(message_id, &content, keep_ids, new_images, false)
            .map(|_| ())
    }

    /// Edit a user message, reconcile its attachments, and remove all later
    /// turns atomically at the database level. Existing files are deleted only
    /// after commit; newly written files are removed on every failure path.
    pub fn edit_user_message(
        &self,
        message_id: Uuid,
        content: &str,
        keep_ids: &[Uuid],
        new_images: &[ImageUpload],
    ) -> Result<usize> {
        self.edit_message(message_id, content, keep_ids, new_images, true)
    }

    fn edit_message(
        &self,
        message_id: Uuid,
        content: &str,
        keep_ids: &[Uuid],
        new_images: &[ImageUpload],
        truncate_after: bool,
    ) -> Result<usize> {
        let mut unique_keep = keep_ids.to_vec();
        unique_keep.sort();
        unique_keep.dedup();
        if unique_keep.len() != keep_ids.len() {
            anyhow::bail!("duplicate attachment ids in edit");
        }

        let decoded = validate_image_uploads(new_images)?;
        let now = Utc::now();
        let mut pending = PendingAttachments::stage(decoded, message_id, now)?;
        let new_attachments = pending.attachments();
        let conn = self.db.conn();
        let mut conn = conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        let mut removed_files = Vec::new();

        let write_result = (|| -> Result<usize> {
            let (session_id, role, sequence) = tx
                .query_row(
                    "SELECT m.session_id, m.role, m.sequence
                     FROM messages m
                     JOIN chat_sessions s ON s.id = m.session_id
                     WHERE m.id = ?1",
                    rusqlite::params![message_id.to_string()],
                    |row| {
                        Ok((
                            parse_uuid(row.get::<_, String>(0)?)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => {
                        anyhow::anyhow!("Message or chat session not found")
                    }
                    other => anyhow::Error::new(other).context("load message for edit"),
                })?;
            if MessageRole::parse(&role) != Some(MessageRole::User) {
                anyhow::bail!("only user messages can be edited");
            }

            let mut current = Vec::new();
            let mut stmt = tx.prepare(
                "SELECT id, file_path FROM attachments WHERE message_id = ?1 ORDER BY created_at, id",
            )?;
            let rows = stmt.query_map(rusqlite::params![message_id.to_string()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (id, path) = row?;
                current.push((
                    id.parse::<Uuid>()
                        .map_err(|_| anyhow::anyhow!("invalid attachment id"))?,
                    path,
                ));
            }
            drop(stmt);

            for keep_id in keep_ids {
                if !current.iter().any(|(id, _)| id == keep_id) {
                    anyhow::bail!("attachment {keep_id} does not belong to this message");
                }
            }
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
            if content.trim().is_empty() && kept_count + new_images.len() == 0 {
                anyhow::bail!("Message cannot be empty");
            }

            // All validation is complete before the first database mutation.
            tx.execute(
                "UPDATE messages SET content = ?2 WHERE id = ?1 AND session_id = ?3",
                rusqlite::params![message_id.to_string(), content, session_id.to_string()],
            )
            .context("update message content")?;

            for (id, path) in &current {
                if !keep_ids.contains(id) {
                    tx.execute(
                        "DELETE FROM attachments WHERE id = ?1 AND message_id = ?2",
                        rusqlite::params![id.to_string(), message_id.to_string()],
                    )
                    .context("delete replaced attachment")?;
                    removed_files.push(path.clone());
                }
            }
            for attachment in &new_attachments {
                insert_attachment(&tx, attachment)?;
            }

            let deleted = if truncate_after {
                let mut file_stmt = tx.prepare(
                    "SELECT a.file_path FROM attachments a
                     JOIN messages m ON m.id = a.message_id
                     WHERE m.session_id = ?1 AND m.sequence > ?2",
                )?;
                let rows = file_stmt
                    .query_map(rusqlite::params![session_id.to_string(), sequence], |row| {
                        row.get::<_, String>(0)
                    })?;
                for row in rows {
                    removed_files.push(row?);
                }
                drop(file_stmt);
                tx.execute(
                    "DELETE FROM messages WHERE session_id = ?1 AND sequence > ?2",
                    rusqlite::params![session_id.to_string(), sequence],
                )
                .context("delete messages after edited message")?
            } else {
                0
            };

            if truncate_after {
                Self::update_session_preview(&tx, session_id, content)?;
            }
            pending.activate()?;
            Ok(deleted)
        })();

        let deleted = match write_result {
            Ok(deleted) => deleted,
            Err(error) => {
                drop(tx);
                return Err(error);
            }
        };
        tx.commit()?;
        pending.persisted();
        drop(conn);
        for path in removed_files {
            remove_attachment_file(&path);
        }
        Ok(deleted)
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
            remove_attachment_file(&rel);
        }
        Ok(())
    }

    /// Delete all messages after the given message using its durable,
    /// per-session sequence. The reference message must belong to `session_id`.
    pub fn delete_messages_after(&self, session_id: Uuid, message_id: Uuid) -> Result<usize> {
        let conn = self.db.conn();
        let mut conn = conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        let sequence: i64 = tx
            .query_row(
                "SELECT sequence FROM messages WHERE id = ?1 AND session_id = ?2",
                rusqlite::params![message_id.to_string(), session_id.to_string()],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    anyhow::anyhow!("Message not found in chat session")
                }
                other => other.into(),
            })?;
        let mut files = Vec::new();
        let mut stmt = tx.prepare(
            "SELECT a.file_path FROM attachments a
             JOIN messages m ON m.id = a.message_id
             WHERE m.session_id = ?1 AND m.sequence > ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![session_id.to_string(), sequence], |row| {
            row.get::<_, String>(0)
        })?;
        for row in rows {
            files.push(row?);
        }
        drop(stmt);
        let deleted = tx.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND sequence > ?2",
            rusqlite::params![session_id.to_string(), sequence],
        )?;
        let preview: Option<String> = tx.query_row(
            "SELECT content FROM messages WHERE id = ?1 AND session_id = ?2",
            rusqlite::params![message_id.to_string(), session_id.to_string()],
            |row| row.get(0),
        )?;
        Self::update_session_preview(&tx, session_id, &preview.unwrap_or_default())?;
        tx.commit()?;
        drop(conn);
        for path in files {
            remove_attachment_file(&path);
        }
        Ok(deleted)
    }

    /// Validate and prepare a regeneration. A normal regeneration may replace
    /// only the latest assistant response immediately following a user turn.
    /// After an edit, the latest message is already the edited user turn and no
    /// deletion is needed. Greetings and arbitrary older responses are not
    /// regeneration targets.
    pub fn prepare_regenerate(&self, session_id: Uuid) -> Result<()> {
        let conn = self.db.conn();
        let mut conn = conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        let messages = tx
            .prepare(
                "SELECT id, role, content FROM messages
                 WHERE session_id = ?1 ORDER BY sequence DESC LIMIT 2",
            )
            .and_then(|mut stmt| {
                let rows = stmt.query_map(rusqlite::params![session_id.to_string()], |row| {
                    Ok((
                        parse_uuid(row.get::<_, String>(0)?)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?;
                let mut messages = Vec::new();
                for row in rows {
                    messages.push(row?);
                }
                Ok(messages)
            })?;
        let Some((latest_id, latest_role, latest_content)) = messages.first() else {
            anyhow::bail!("there is no message to regenerate");
        };

        let file = match MessageRole::parse(latest_role) {
            Some(MessageRole::User) => None,
            Some(MessageRole::Assistant) => {
                let Some((_, previous_role, _)) = messages.get(1) else {
                    anyhow::bail!("a greeting or standalone response cannot be regenerated");
                };
                if MessageRole::parse(previous_role) != Some(MessageRole::User) {
                    anyhow::bail!("only the latest response to a user message can be regenerated");
                }
                let mut stmt =
                    tx.prepare("SELECT file_path FROM attachments WHERE message_id = ?1")?;
                let rows = stmt.query_map(rusqlite::params![latest_id.to_string()], |row| {
                    row.get::<_, String>(0)
                })?;
                let mut files = Vec::new();
                for row in rows {
                    files.push(row?);
                }
                Some((files, latest_content.clone()))
            }
            _ => anyhow::bail!("the latest message cannot be regenerated"),
        };

        let previous_content = if file.is_some() {
            tx.execute(
                "DELETE FROM messages WHERE id = ?1 AND session_id = ?2",
                rusqlite::params![latest_id.to_string(), session_id.to_string()],
            )?;
            messages
                .get(1)
                .map(|(_, _, content)| content.clone())
                .unwrap_or_default()
        } else {
            latest_content.clone()
        };
        Self::update_session_preview(&tx, session_id, &previous_content)?;
        tx.commit()?;
        drop(conn);
        if let Some((files, _)) = file {
            for path in files {
                remove_attachment_file(&path);
            }
        }
        Ok(())
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
             FROM messages WHERE session_id = ?1 ORDER BY sequence ASC",
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

fn export_filename_stem(title: &str) -> String {
    let mut stem = String::new();
    let mut separator = false;
    for character in title.chars() {
        if character.is_alphanumeric() {
            if separator && !stem.is_empty() {
                stem.push('-');
            }
            stem.extend(character.to_lowercase());
            separator = false;
        } else {
            separator = true;
        }
        if stem.chars().count() == 60 {
            break;
        }
    }
    let stem = stem.trim_matches('-');
    if stem.is_empty() {
        "conversation".to_string()
    } else {
        stem.to_string()
    }
}

fn export_markdown(document: &ChatExportDocument) -> String {
    let title = document
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(&document.character.name);
    let mut markdown = format!(
        "# {}\n\n- **Character:** {}\n- **Exported:** {}\n- **Conversation ID:** `{}`\n\n",
        single_line(title),
        single_line(&document.character.name),
        document.exported_at.to_rfc3339(),
        document.session_id,
    );
    markdown.push_str(
        "Attachment entries contain metadata only; no binary image data is included.\n\n---\n\n",
    );

    for message in &document.messages {
        let role = match message.role {
            MessageRole::System => "System",
            MessageRole::User => "User",
            MessageRole::Assistant => "Assistant",
        };
        markdown.push_str(&format!("## {role}\n\n"));
        markdown.push_str(&format!("_{}_", message.created_at.to_rfc3339()));
        if let Some(model) = message
            .model_used
            .as_deref()
            .filter(|model| !model.trim().is_empty())
        {
            markdown.push_str(&format!(" · Model: `{}`", single_line(model)));
        }
        markdown.push_str("\n\n");
        if message.content.is_empty() {
            markdown.push_str("_No text content._\n");
        } else {
            markdown.push_str(&message.content);
            markdown.push('\n');
        }
        if !message.attachments.is_empty() {
            markdown.push_str("\n**Attachments**\n\n");
            for attachment in &message.attachments {
                let name = attachment
                    .original_name
                    .as_deref()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or("Unnamed image");
                markdown.push_str(&format!(
                    "- {} — `{}`, {} bytes, attachment ID `{}`\n",
                    single_line(name),
                    single_line(&attachment.content_type),
                    attachment.file_size,
                    attachment.id,
                ));
            }
        }
        markdown.push_str("\n---\n\n");
    }

    markdown
}

fn single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Drop a reasoning model's leading `<think>...</think>` block so sidebar and
/// home-screen previews show the actual answer, not the model's reasoning.
/// Mirrors the chat UI's `parse_thinking`: the tag is only meaningful as a
/// prefix. An unclosed block means only reasoning has streamed so far.
fn strip_reasoning(text: &str) -> &str {
    let trimmed = text.trim_start();
    let Some(rest) = trimmed.strip_prefix("<think>") else {
        return text;
    };
    match rest.find("</think>") {
        Some(end) => rest[end + "</think>".len()..].trim_start(),
        None => "",
    }
}

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

use crate::models::message::{MAX_IMAGES_PER_MESSAGE, MAX_IMAGE_BYTES, MAX_TOTAL_IMAGE_BYTES};

/// Map a supported image MIME type to a file extension. Returns `None` for
/// anything that isn't a recognized image type (upload is rejected).
pub(crate) fn mime_to_extension(content_type: &str) -> Option<&'static str> {
    match content_type {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

#[derive(Debug)]
pub(crate) struct DecodedImage {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

/// Shared base64/MIME/size validation for attachments and avatars. Encoded
/// length is checked before allocating a decoded buffer.
pub(crate) fn decode_image(
    data: &str,
    content_type: &str,
    max_bytes: usize,
) -> Result<DecodedImage> {
    let extension = mime_to_extension(content_type)
        .ok_or_else(|| anyhow::anyhow!("unsupported attachment type: {content_type}"))?;
    let max_encoded = crate::models::message::max_base64_len(max_bytes);
    if data.len() > max_encoded {
        anyhow::bail!(
            "encoded image is too large: {} bytes (max {} base64 bytes)",
            data.len(),
            max_encoded
        );
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .context("decode attachment base64")?;
    if bytes.len() > max_bytes {
        anyhow::bail!(
            "image too large: {} bytes (max {max_bytes} bytes)",
            bytes.len()
        );
    }
    Ok(DecodedImage { bytes, extension })
}

fn validate_total_image_bytes(sizes: &[usize]) -> Result<usize> {
    let total = sizes.iter().try_fold(0usize, |total, size| {
        total
            .checked_add(*size)
            .ok_or_else(|| anyhow::anyhow!("aggregate attachment size overflow"))
    })?;
    if total > MAX_TOTAL_IMAGE_BYTES {
        anyhow::bail!(
            "attachments are too large together: {total} bytes (max {MAX_TOTAL_IMAGE_BYTES} bytes)"
        );
    }
    Ok(total)
}

/// Validate a complete message batch before any file is written.
fn validate_image_uploads(images: &[ImageUpload]) -> Result<Vec<(ImageUpload, DecodedImage)>> {
    if images.len() > MAX_IMAGES_PER_MESSAGE {
        anyhow::bail!(
            "too many attachments: {} (max {})",
            images.len(),
            MAX_IMAGES_PER_MESSAGE
        );
    }
    let mut decoded = Vec::with_capacity(images.len());
    let mut sizes = Vec::with_capacity(images.len());
    for image in images {
        let image_decoded = decode_image(&image.data, &image.content_type, MAX_IMAGE_BYTES)?;
        sizes.push(image_decoded.bytes.len());
        decoded.push((image.clone(), image_decoded));
    }
    validate_total_image_bytes(&sizes)?;
    Ok(decoded)
}

struct PendingAttachment {
    attachment: Attachment,
    staged_path: std::path::PathBuf,
    final_path: std::path::PathBuf,
}

/// Files are invisible to the rest of the app while staged. Drop cleans both
/// staging and activated paths unless `persisted` is called after DB commit.
struct PendingAttachments {
    files: Vec<PendingAttachment>,
    persisted: bool,
}

impl PendingAttachments {
    fn stage(
        images: Vec<(ImageUpload, DecodedImage)>,
        message_id: Uuid,
        now: chrono::DateTime<Utc>,
    ) -> Result<Self> {
        let mut pending = Self {
            files: Vec::with_capacity(images.len()),
            persisted: false,
        };
        for (image, decoded) in images {
            let id = Uuid::new_v4();
            let filename = format!("{id}.{}", decoded.extension);
            let final_path = config::attachment_path(&filename)?;
            let staged_path = config::attachment_path(&format!(".{id}.staging"))?;
            pending.files.push(PendingAttachment {
                attachment: Attachment {
                    id,
                    message_id,
                    content_type: image.content_type,
                    file_path: format!("attachments/{filename}"),
                    original_name: image.original_name,
                    file_size: decoded.bytes.len() as u64,
                    created_at: now,
                },
                staged_path,
                final_path,
            });
            let staged = &pending.files.last().expect("just pushed").staged_path;
            std::fs::write(staged, &decoded.bytes).context("stage attachment file")?;
        }
        Ok(pending)
    }

    fn attachments(&self) -> Vec<Attachment> {
        self.files
            .iter()
            .map(|file| file.attachment.clone())
            .collect()
    }

    fn activate(&mut self) -> Result<()> {
        for file in &self.files {
            std::fs::rename(&file.staged_path, &file.final_path)
                .context("activate staged attachment")?;
        }
        Ok(())
    }

    fn persisted(&mut self) {
        self.persisted = true;
    }
}

impl Drop for PendingAttachments {
    fn drop(&mut self) {
        if self.persisted {
            return;
        }
        for file in &self.files {
            let _ = std::fs::remove_file(&file.staged_path);
            let _ = std::fs::remove_file(&file.final_path);
        }
    }
}

fn insert_attachment(conn: &rusqlite::Connection, att: &Attachment) -> Result<()> {
    conn.execute(
        "INSERT INTO attachments
         (id, message_id, content_type, file_path, original_name, file_size, created_at)
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
    Ok(())
}

pub(crate) fn remove_attachment_file(rel: &str) {
    use std::path::{Component, Path};
    let path = Path::new(rel);
    let mut components = path.components();
    let safe = matches!(components.next(), Some(Component::Normal(dir)) if dir == "attachments")
        && components.all(|component| matches!(component, Component::Normal(_)));
    if !safe {
        tracing::warn!("refusing to remove unsafe attachment path {rel}");
        return;
    }
    if let Ok(dir) = config::attachments_dir() {
        if let Some(relative) = rel.strip_prefix("attachments/") {
            let path = dir.join(relative);
            if path.exists() {
                if let Err(e) = std::fs::remove_file(&path) {
                    tracing::warn!("failed to remove attachment {}: {e}", path.display());
                }
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

    fn assistant_message(session_id: Uuid, content: &str) -> NewMessage {
        NewMessage {
            session_id,
            role: MessageRole::Assistant,
            content: content.to_string(),
            image_attachments: Vec::new(),
            model_used: Some("test-model".to_string()),
            provider_id: None,
        }
    }

    #[test]
    fn strip_reasoning_drops_only_a_leading_think_block() {
        assert_eq!(
            strip_reasoning("<think>pondering</think>The answer is 42."),
            "The answer is 42."
        );
        // Unclosed block: only reasoning has streamed so far.
        assert_eq!(strip_reasoning("<think>still thinking"), "");
        // No reasoning: content is returned unchanged.
        assert_eq!(
            strip_reasoning("Just a normal reply."),
            "Just a normal reply."
        );
        // A `<think>` that is not a prefix is left alone.
        assert_eq!(
            strip_reasoning("Here is code: <think> is a tag"),
            "Here is code: <think> is a tag"
        );
    }

    #[test]
    fn assistant_reasoning_is_excluded_from_the_sidebar_preview() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db.clone());
        let session = new_session(&chat, character.id, "reasoning preview");

        chat.add_message(&user_message(session.id, "What is 6 times 7?"))
            .unwrap();
        chat.add_message(&assistant_message(
            session.id,
            "<think>6*7 is 42</think>The answer is 42.",
        ))
        .unwrap();

        let reloaded = chat.get_session(session.id).unwrap().unwrap();
        assert_eq!(reloaded.last_message.as_deref(), Some("The answer is 42."));
    }

    fn image(bytes: &[u8]) -> ImageUpload {
        ImageUpload {
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
            content_type: "image/png".to_string(),
            original_name: Some("test.png".to_string()),
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
    fn rename_session_trims_and_validates_titles() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db);
        let session = new_session(&chat, character.id, "Original");

        let renamed = chat.rename_session(session.id, "  A new title  ").unwrap();
        assert_eq!(renamed.title.as_deref(), Some("A new title"));
        assert_eq!(
            chat.get_session(session.id)
                .unwrap()
                .unwrap()
                .title
                .as_deref(),
            Some("A new title")
        );

        assert!(chat.rename_session(session.id, " \n\t ").is_err());
        let too_long = "x".repeat(MAX_CHAT_TITLE_CHARS + 1);
        assert!(chat.rename_session(session.id, &too_long).is_err());
        assert!(chat
            .rename_session(Uuid::new_v4(), "Missing conversation")
            .is_err());
        assert_eq!(
            chat.get_session(session.id)
                .unwrap()
                .unwrap()
                .title
                .as_deref(),
            Some("A new title"),
            "rejected renames must leave the stored title unchanged"
        );
    }

    #[test]
    fn exports_include_messages_and_attachment_metadata_without_image_data() {
        let _config = crate::db::TestConfigDir::new();
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db);
        let session = new_session(&chat, character.id, "Portable conversation");
        let mut message = user_message(session.id, "Look at this image");
        message.image_attachments = vec![image(b"do not export these bytes")];
        chat.add_message(&message).unwrap();
        chat.add_message(&assistant_message(session.id, "A useful answer"))
            .unwrap();

        let json = chat
            .export_session(session.id, &character, ChatExportFormat::Json)
            .unwrap();
        assert_eq!(json.filename, "portable-conversation.json");
        assert_eq!(json.content_type, "application/json; charset=utf-8");
        let document: ChatExportDocument = serde_json::from_str(&json.content).unwrap();
        assert_eq!(document.schema_version, 1);
        assert_eq!(document.title.as_deref(), Some("Portable conversation"));
        assert_eq!(document.character.name, "Luna");
        assert_eq!(document.messages.len(), 2);
        assert_eq!(document.messages[0].attachments.len(), 1);
        assert_eq!(
            document.messages[0].attachments[0].original_name.as_deref(),
            Some("test.png")
        );
        assert!(!json.content.contains("file_path"));
        assert!(!json.content.contains("ZG8gbm90IGV4cG9ydCB0aGVzZSBieXRlcw"));

        let markdown = chat
            .export_session(session.id, &character, ChatExportFormat::Markdown)
            .unwrap();
        assert_eq!(markdown.filename, "portable-conversation.md");
        assert_eq!(markdown.content_type, "text/markdown; charset=utf-8");
        assert!(markdown.content.contains("# Portable conversation"));
        assert!(markdown.content.contains("Look at this image"));
        assert!(markdown.content.contains("test.png"));
        assert!(markdown
            .content
            .contains("no binary image data is included"));
        assert!(!markdown
            .content
            .contains("ZG8gbm90IGV4cG9ydCB0aGVzZSBieXRlcw"));
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
    fn sequence_orders_and_truncates_same_timestamp_turns() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db.clone());
        let session = new_session(&chat, character.id, "timestamps");

        let first = chat
            .add_message(&user_message(session.id, "first"))
            .unwrap();
        chat.add_message(&user_message(session.id, "second"))
            .unwrap();
        chat.add_message(&user_message(session.id, "third"))
            .unwrap();
        {
            let conn = db.conn();
            let conn = conn.lock().unwrap();
            conn.execute(
                "UPDATE messages SET created_at = '2026-01-01T00:00:00Z' WHERE session_id = ?1",
                rusqlite::params![session.id.to_string()],
            )
            .unwrap();
        }

        let before = chat.list_messages(session.id).unwrap();
        assert_eq!(
            before
                .iter()
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second", "third"]
        );
        let other_session = new_session(&chat, character.id, "other");
        assert!(chat
            .delete_messages_after(other_session.id, first.id)
            .is_err());
        assert_eq!(chat.list_messages(session.id).unwrap().len(), 3);
        assert_eq!(chat.delete_messages_after(session.id, first.id).unwrap(), 2);
        let remaining = chat.list_messages(session.id).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].content, "first");
    }

    #[test]
    fn same_length_user_edit_replaces_content_and_truncates_following_turns() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db);
        let session = new_session(&chat, character.id, "edit");
        let first = chat.add_message(&user_message(session.id, "abc")).unwrap();
        chat.add_message(&assistant_message(session.id, "old"))
            .unwrap();
        chat.add_message(&user_message(session.id, "later"))
            .unwrap();

        let deleted = chat.edit_user_message(first.id, "xyz", &[], &[]).unwrap();
        assert_eq!(deleted, 2);
        let messages = chat.list_messages(session.id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "xyz");
    }

    #[test]
    fn editing_assistant_message_is_rejected_without_mutation() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db);
        let session = new_session(&chat, character.id, "role validation");
        chat.add_message(&user_message(session.id, "question"))
            .unwrap();
        let response = chat
            .add_message(&assistant_message(session.id, "answer"))
            .unwrap();

        assert!(chat
            .edit_user_message(response.id, "changed", &[], &[])
            .is_err());
        assert_eq!(chat.list_messages(session.id).unwrap().len(), 2);
        assert_eq!(
            chat.get_message(response.id).unwrap().unwrap().content,
            "answer"
        );
    }

    #[test]
    fn edit_delete_failure_rolls_back_db_and_cleans_new_files() {
        let _config = crate::db::TestConfigDir::new();
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db.clone());
        let session = new_session(&chat, character.id, "atomic edit");
        let mut original = user_message(session.id, "abc");
        original.image_attachments = vec![image(b"old image")];
        let first = chat.add_message(&original).unwrap();
        chat.add_message(&assistant_message(session.id, "answer"))
            .unwrap();
        let mut later = user_message(session.id, "later");
        later.image_attachments = vec![image(b"later image")];
        chat.add_message(&later).unwrap();
        let messages = chat.list_messages(session.id).unwrap();
        let old_path = config::config_dir()
            .unwrap()
            .join(&messages[0].attachments[0].file_path);
        let later_path = config::config_dir()
            .unwrap()
            .join(&messages[2].attachments[0].file_path);
        {
            let conn = db.conn();
            let conn = conn.lock().unwrap();
            conn.execute_batch(
                "CREATE TRIGGER fail_later_delete BEFORE DELETE ON messages
                 WHEN OLD.content = 'later'
                 BEGIN SELECT RAISE(ABORT, 'injected truncation failure'); END;",
            )
            .unwrap();
        }

        let error = chat
            .edit_user_message(first.id, "xyz", &[], &[image(b"new image")])
            .unwrap_err();
        assert!(error.to_string().contains("delete messages after"));
        let rolled_back = chat.list_messages(session.id).unwrap();
        assert_eq!(rolled_back.len(), 3);
        assert_eq!(rolled_back[0].content, "abc");
        assert_eq!(rolled_back[0].attachments.len(), 1);
        assert!(old_path.exists());
        assert!(later_path.exists());
        assert_eq!(
            std::fs::read_dir(config::attachments_dir().unwrap())
                .unwrap()
                .count(),
            2
        );
        {
            let conn = db.conn();
            let conn = conn.lock().unwrap();
            conn.execute_batch("DROP TRIGGER fail_later_delete")
                .unwrap();
        }

        assert_eq!(
            chat.edit_user_message(first.id, "xyz", &[], &[image(b"new image")])
                .unwrap(),
            2
        );
        assert!(!old_path.exists());
        assert!(!later_path.exists());
        let committed = chat.list_messages(session.id).unwrap();
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].content, "xyz");
        let new_path = config::config_dir()
            .unwrap()
            .join(&committed[0].attachments[0].file_path);
        assert!(new_path.exists());
    }

    #[test]
    fn failed_message_insert_removes_staged_attachments() {
        let _config = crate::db::TestConfigDir::new();
        let chat = ChatService::new(test_db());
        let mut message = user_message(Uuid::new_v4(), "orphan");
        message.image_attachments = vec![image(b"new image")];

        assert!(chat.add_message(&message).is_err());
        assert_eq!(
            std::fs::read_dir(config::attachments_dir().unwrap())
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn aggregate_attachment_limit_has_no_per_image_loop_hole() {
        assert_eq!(
            validate_total_image_bytes(&[MAX_IMAGE_BYTES; MAX_IMAGES_PER_MESSAGE]).unwrap(),
            MAX_TOTAL_IMAGE_BYTES
        );
        let mut over = vec![MAX_IMAGE_BYTES; MAX_IMAGES_PER_MESSAGE];
        over[MAX_IMAGES_PER_MESSAGE - 1] += 1;
        let error = validate_total_image_bytes(&over).unwrap_err().to_string();
        assert!(error.contains("too large together"));
    }

    #[test]
    fn regenerate_only_removes_latest_response_after_user_turn() {
        let db = test_db();
        let character = make_character(&db, None);
        let chat = ChatService::new(db);
        let session = new_session(&chat, character.id, "regenerate");
        chat.add_message(&user_message(session.id, "question"))
            .unwrap();
        chat.add_message(&assistant_message(session.id, "first answer"))
            .unwrap();

        chat.prepare_regenerate(session.id).unwrap();
        let messages = chat.list_messages(session.id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, MessageRole::User);
        // Edited-user regeneration starts from the already truncated user turn.
        chat.prepare_regenerate(session.id).unwrap();
        assert_eq!(chat.list_messages(session.id).unwrap().len(), 1);
    }

    #[test]
    fn regenerate_rejects_greetings_and_consecutive_responses() {
        let db = test_db();
        let character = make_character(&db, Some("Greeting"));
        let chat = ChatService::new(db);
        let greeting_session = chat
            .create_session(
                &NewChatSession {
                    character_id: character.id,
                    title: None,
                },
                character.greeting.clone(),
            )
            .unwrap();
        assert!(chat.prepare_regenerate(greeting_session.id).is_err());
        assert_eq!(chat.list_messages(greeting_session.id).unwrap().len(), 1);

        let session = new_session(&chat, character.id, "invalid regeneration");
        chat.add_message(&user_message(session.id, "question"))
            .unwrap();
        chat.add_message(&assistant_message(session.id, "one"))
            .unwrap();
        chat.add_message(&assistant_message(session.id, "two"))
            .unwrap();
        assert!(chat.prepare_regenerate(session.id).is_err());
        assert_eq!(chat.list_messages(session.id).unwrap().len(), 3);
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
        let err = decode_image(&img.data, &img.content_type, MAX_IMAGE_BYTES).unwrap_err();
        assert!(err.to_string().contains("unsupported attachment type"));
    }
}
