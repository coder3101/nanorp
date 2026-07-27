//! Character CRUD, including avatar file cleanup.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
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
        // Grab the avatar path first so we can clean up the file afterwards.
        let avatar = self.get(id)?.and_then(|c| c.avatar_path);

        {
            let conn = self.db.conn();
            let conn = conn.lock().expect("db mutex poisoned");
            conn.execute(
                "DELETE FROM characters WHERE id = ?1",
                rusqlite::params![id.to_string()],
            )
            .context("delete character")?;
        }

        // Best-effort file cleanup (DB delete already succeeded).
        if let Some(rel) = avatar {
            remove_avatar_file(&rel);
        }
        Ok(())
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

/// Delete an avatar file given its config-relative path (e.g. "avatars/{id}.png").
fn remove_avatar_file(rel: &str) {
    if let Ok(dir) = config::config_dir() {
        let path = dir.join(rel);
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
