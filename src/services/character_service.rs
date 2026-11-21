//! Character CRUD, including avatar file cleanup.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rusqlite::OptionalExtension;
use uuid::Uuid;

use crate::config;
use crate::db::Db;
use crate::models::character::{Character, NewCharacter, UpdateCharacter};

pub struct CharacterService {
    db: Db,
}

impl CharacterService {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub fn create(&self, new: &NewCharacter) -> Result<Character> {
        let now = Utc::now();
        let character = Character {
            id: Uuid::new_v4(),
            name: new.name.clone(),
            role: clean(new.role.clone()),
            personality: clean(new.personality.clone()),
            system_prompt: clean(new.system_prompt.clone()),
            greeting: clean(new.greeting.clone()),
            avatar_path: None,
            created_at: now,
            updated_at: now,
        };

        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO characters
                (id, name, role, personality, system_prompt, greeting, avatar_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                character.id.to_string(),
                character.name,
                character.role,
                character.personality,
                character.system_prompt,
                character.greeting,
                character.avatar_path,
                character.created_at.to_rfc3339(),
                character.updated_at.to_rfc3339(),
            ],
        )
        .context("insert character")?;

        Ok(character)
    }

    pub fn get(&self, id: Uuid) -> Result<Option<Character>> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(SELECT_ONE)?;
        let mut rows = stmt.query(rusqlite::params![id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_character(row)?)),
            None => Ok(None),
        }
    }

    pub fn list(&self) -> Result<Vec<Character>> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, name, role, personality, system_prompt, greeting, avatar_path, created_at, updated_at
             FROM characters ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_character)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn update(&self, update: &UpdateCharacter) -> Result<Character> {
        let conn = self.db.conn();
        let mut conn = conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;

        let existing: Character = {
            let mut stmt = tx.prepare(SELECT_ONE)?;
            let mut rows = stmt.query(rusqlite::params![update.id.to_string()])?;
            match rows.next()? {
                Some(row) => row_to_character(row)?,
                None => return Err(anyhow!("Character not found")),
            }
        };

        let merged = Character {
            id: existing.id,
            name: update.name.clone().unwrap_or(existing.name),
            role: update.role.clone().map(opt_clean).unwrap_or(existing.role),
            personality: update
                .personality
                .clone()
                .map(opt_clean)
                .unwrap_or(existing.personality),
            system_prompt: update
                .system_prompt
                .clone()
                .map(opt_clean)
                .unwrap_or(existing.system_prompt),
            greeting: update
                .greeting
                .clone()
                .map(opt_clean)
                .unwrap_or(existing.greeting),
            avatar_path: existing.avatar_path,
            created_at: existing.created_at,
            updated_at: Utc::now(),
        };

        tx.execute(
            "UPDATE characters SET
                name = ?2, role = ?3, personality = ?4, system_prompt = ?5,
                greeting = ?6, updated_at = ?7
             WHERE id = ?1",
            rusqlite::params![
                merged.id.to_string(),
                merged.name,
                merged.role,
                merged.personality,
                merged.system_prompt,
                merged.greeting,
                merged.updated_at.to_rfc3339(),
            ],
        )
        .context("update character")?;

        tx.commit()?;
        Ok(merged)
    }

    pub fn delete(&self, id: Uuid) -> Result<()> {
        let conn = self.db.conn();
        let mut conn = conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;
        let avatar = tx
            .query_row(
                "SELECT avatar_path FROM characters WHERE id = ?1",
                rusqlite::params![id.to_string()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        let mut attachments = Vec::new();
        let mut stmt = tx.prepare(
            "SELECT a.file_path FROM attachments a
             JOIN messages m ON m.id = a.message_id
             JOIN chat_sessions s ON s.id = m.session_id
             WHERE s.character_id = ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![id.to_string()], |row| {
            row.get::<_, String>(0)
        })?;
        for row in rows {
            attachments.push(row?);
        }
        drop(stmt);
        tx.execute(
            "DELETE FROM characters WHERE id = ?1",
            rusqlite::params![id.to_string()],
        )
        .context("delete character")?;
        tx.commit()?;
        drop(conn);

        // Files are best-effort cleanup only after the cascading DB commit.
        for path in attachments {
            crate::services::chat_service::remove_attachment_file(&path);
        }
        if let Some(rel) = avatar {
            remove_avatar_file(&rel);
        }
        Ok(())
    }

    /// Validate, stage, and activate a new avatar before updating the DB. The
    /// old avatar remains valid until the DB commit succeeds. A unique filename
    /// also prevents replacement from overwriting the currently served file.
    pub fn replace_avatar(&self, id: Uuid, data: &str, content_type: &str) -> Result<Character> {
        use crate::models::message::MAX_AVATAR_BYTES;
        use crate::services::chat_service::decode_image;

        let decoded = decode_image(data, content_type, MAX_AVATAR_BYTES)?;
        let filename = format!("{id}-{}.{}", Uuid::new_v4(), decoded.extension);
        let final_path = config::avatars_dir()?.join(&filename);
        let staged_path = config::avatars_dir()?.join(format!(".{filename}.staging"));
        let write_result = (|| -> Result<()> {
            std::fs::write(&staged_path, &decoded.bytes).context("stage avatar file")?;
            std::fs::rename(&staged_path, &final_path).context("activate avatar file")?;
            Ok(())
        })();
        if let Err(error) = write_result {
            let _ = std::fs::remove_file(&staged_path);
            let _ = std::fs::remove_file(&final_path);
            return Err(error);
        }

        let rel_path = format!("avatars/{filename}");
        match self.set_avatar_path(id, Some(rel_path)) {
            Ok(character) => Ok(character),
            Err(error) => {
                let _ = std::fs::remove_file(&final_path);
                Err(error)
            }
        }
    }

    /// Update just the avatar path, removing any previous avatar file that
    /// differs from the new one.
    pub fn set_avatar_path(&self, id: Uuid, new_rel: Option<String>) -> Result<Character> {
        let conn = self.db.conn();
        let mut conn = conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;

        let existing: Character = {
            let mut stmt = tx.prepare(SELECT_ONE)?;
            let mut rows = stmt.query(rusqlite::params![id.to_string()])?;
            match rows.next()? {
                Some(row) => row_to_character(row)?,
                None => return Err(anyhow!("Character not found")),
            }
        };

        let now = Utc::now();
        tx.execute(
            "UPDATE characters SET avatar_path = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![id.to_string(), new_rel, now.to_rfc3339()],
        )
        .context("update avatar path")?;
        tx.commit()?;

        // Remove the old file if it changed.
        if let Some(old) = existing.avatar_path {
            if Some(&old) != new_rel.as_ref() {
                remove_avatar_file(&old);
            }
        }

        Ok(Character {
            avatar_path: new_rel,
            updated_at: now,
            ..existing
        })
    }
}

const SELECT_ONE: &str =
    "SELECT id, name, role, personality, system_prompt, greeting, avatar_path, created_at, updated_at
     FROM characters WHERE id = ?1";

/// Trim and drop empty strings to keep NULLs clean in the DB.
fn clean(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// For update fields where `None` on the outer Option means "no change" and the
/// inner value should be cleaned.
fn opt_clean(s: String) -> Option<String> {
    let t = s.trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

/// Delete an avatar file only when its DB path stays under `avatars/`.
fn remove_avatar_file(rel: &str) {
    use std::path::{Component, Path};
    let path = Path::new(rel);
    let mut components = path.components();
    let safe = matches!(components.next(), Some(Component::Normal(dir)) if dir == "avatars")
        && components.all(|component| matches!(component, Component::Normal(_)));
    if !safe {
        tracing::warn!("refusing to remove unsafe avatar path {rel}");
        return;
    }
    if let (Ok(dir), Some(relative)) = (config::avatars_dir(), rel.strip_prefix("avatars/")) {
        let path = dir.join(relative);
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!("failed to remove avatar file {}: {e}", path.display());
            }
        }
    }
}

fn row_to_character(row: &rusqlite::Row) -> rusqlite::Result<Character> {
    let id_str: String = row.get("id")?;
    let created_str: String = row.get("created_at")?;
    let updated_str: String = row.get("updated_at")?;

    Ok(Character {
        id: id_str.parse().map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                "invalid uuid".into(),
            )
        })?,
        name: row.get("name")?,
        role: row.get("role")?,
        personality: row.get("personality")?,
        system_prompt: row.get("system_prompt")?,
        greeting: row.get("greeting")?,
        avatar_path: row.get("avatar_path")?,
        created_at: parse_dt(&created_str)?,
        updated_at: parse_dt(&updated_str)?,
    })
}

fn parse_dt(s: &str) -> rusqlite::Result<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
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
    use crate::models::chat::NewChatSession;
    use crate::models::message::{MessageRole, NewMessage};
    use crate::services::chat_service::ChatService;
    use base64::Engine;

    fn new_character() -> NewCharacter {
        NewCharacter {
            name: "File cleanup".to_string(),
            role: None,
            personality: None,
            system_prompt: None,
            greeting: None,
        }
    }

    fn encoded(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn character_delete_cascades_avatar_and_descendant_attachment_files() {
        let _test_config = crate::db::TestConfigDir::new();
        let db = Db::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let characters = CharacterService::new(db.clone());
        let character = characters.create(&new_character()).unwrap();
        let character = characters
            .replace_avatar(character.id, &encoded(b"avatar"), "image/png")
            .unwrap();
        let avatar = config::config_dir()
            .unwrap()
            .join(character.avatar_path.as_deref().unwrap());
        let session = ChatService::new(db.clone())
            .create_session(
                &NewChatSession {
                    character_id: character.id,
                    title: None,
                },
                None,
            )
            .unwrap();
        let message = ChatService::new(db.clone())
            .add_message(&NewMessage {
                session_id: session.id,
                role: MessageRole::User,
                content: "attachment".to_string(),
                image_attachments: vec![crate::models::message::ImageUpload {
                    data: encoded(b"attachment"),
                    content_type: "image/png".to_string(),
                    original_name: None,
                }],
                model_used: None,
                provider_id: None,
            })
            .unwrap();
        let attachment = config::config_dir()
            .unwrap()
            .join(&message.attachments[0].file_path);
        assert!(avatar.exists());
        assert!(attachment.exists());

        characters.delete(character.id).unwrap();
        assert!(!avatar.exists());
        assert!(!attachment.exists());
        assert!(characters.get(character.id).unwrap().is_none());
    }

    #[test]
    fn failed_avatar_db_replacement_preserves_old_and_cleans_new_file() {
        let _test_config = crate::db::TestConfigDir::new();
        let db = Db::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        let characters = CharacterService::new(db.clone());
        let character = characters.create(&new_character()).unwrap();
        let character = characters
            .replace_avatar(character.id, &encoded(b"old avatar"), "image/png")
            .unwrap();
        let old_path = config::config_dir()
            .unwrap()
            .join(character.avatar_path.as_deref().unwrap());
        {
            let conn = db.conn();
            let conn = conn.lock().unwrap();
            conn.execute_batch(
                "CREATE TRIGGER fail_avatar_update BEFORE UPDATE OF avatar_path ON characters
                 BEGIN SELECT RAISE(ABORT, 'injected avatar update failure'); END;",
            )
            .unwrap();
        }

        assert!(characters
            .replace_avatar(character.id, &encoded(b"new avatar"), "image/webp")
            .is_err());
        assert!(old_path.exists());
        assert_eq!(
            std::fs::read_dir(config::avatars_dir().unwrap())
                .unwrap()
                .count(),
            1
        );
        let reloaded = characters.get(character.id).unwrap().unwrap();
        assert_eq!(reloaded.avatar_path, character.avatar_path);
    }
}
