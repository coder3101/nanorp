//! Settings persistence; missing keys fall back to `AppSettings::default()`.

use anyhow::{Context, Result};

use crate::db::Db;
use crate::models::settings::AppSettings;

pub const KEY_DEFAULT_SYSTEM_PROMPT: &str = "default_system_prompt";
pub const KEY_USER_NAME: &str = "user_name";
pub const KEY_DEFAULT_PROVIDER_ID: &str = "default_provider_id";
pub const KEY_DEFAULT_MODEL: &str = "default_model";
pub const KEY_THEME: &str = "theme";
pub const KEY_RENDER_THINKING: &str = "render_thinking";
pub const KEY_TEMPERATURE: &str = "temperature";
pub const KEY_TOP_P: &str = "top_p";
pub const KEY_MAX_TOKENS: &str = "max_tokens";

pub struct SettingsService {
    db: Db,
}

impl SettingsService {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Load all settings, merging persisted values over defaults.
    pub fn get_all(&self) -> Result<AppSettings> {
        let mut settings = AppSettings::default();

        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings")
            .context("prepare select settings")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .context("query settings")?;

        for row in rows {
            let (key, value) = row?;
            match key.as_str() {
                KEY_DEFAULT_SYSTEM_PROMPT => {
                    if let Ok(v) = serde_json::from_str::<String>(&value) {
                        settings.default_system_prompt = v;
                    }
                }
                KEY_USER_NAME => {
                    if let Ok(v) = serde_json::from_str::<String>(&value) {
                        settings.user_name = v;
                    }
                }
                KEY_DEFAULT_PROVIDER_ID => {
                    if let Ok(v) = serde_json::from_str::<Option<uuid::Uuid>>(&value) {
                        settings.default_provider_id = v;
                    }
                }
                KEY_DEFAULT_MODEL => {
                    if let Ok(v) = serde_json::from_str::<Option<String>>(&value) {
                        settings.default_model = v;
                    }
                }
                KEY_THEME => {
                    if let Ok(v) = serde_json::from_str::<String>(&value) {
                        settings.theme = v;
                    }
                }
                KEY_RENDER_THINKING => {
                    if let Ok(v) = serde_json::from_str::<bool>(&value) {
                        settings.render_thinking = v;
                    }
                }
                KEY_TEMPERATURE => {
                    if let Ok(v) = serde_json::from_str::<f32>(&value) {
                        settings.temperature = v;
                    }
                }
                KEY_TOP_P => {
                    if let Ok(v) = serde_json::from_str::<f32>(&value) {
                        settings.top_p = v;
                    }
                }
                KEY_MAX_TOKENS => {
                    if let Ok(v) = serde_json::from_str::<Option<u32>>(&value) {
                        settings.max_tokens = v;
                    }
                }
                _ => {} // ignore unknown keys (forward compatibility)
            }
        }

        Ok(settings)
    }

    /// Upsert a single JSON-encoded setting value.
    pub fn set(&self, key: &str, json_value: &str) -> Result<()> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, json_value],
        )
        .with_context(|| format!("upsert setting {key}"))?;
        Ok(())
    }

    /// Persist every field of `AppSettings` in a single transaction.
    pub fn set_all(&self, settings: &AppSettings) -> Result<()> {
        let conn = self.db.conn();
        let mut conn = conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction().context("begin settings transaction")?;

        let pairs = [
            (
                KEY_DEFAULT_SYSTEM_PROMPT,
                serde_json::to_string(&settings.default_system_prompt)?,
            ),
            (KEY_USER_NAME, serde_json::to_string(&settings.user_name)?),
            (
                KEY_DEFAULT_PROVIDER_ID,
                serde_json::to_string(&settings.default_provider_id)?,
            ),
            (
                KEY_DEFAULT_MODEL,
                serde_json::to_string(&settings.default_model)?,
            ),
            (KEY_THEME, serde_json::to_string(&settings.theme)?),
            (
                KEY_RENDER_THINKING,
                serde_json::to_string(&settings.render_thinking)?,
            ),
            (
                KEY_TEMPERATURE,
                serde_json::to_string(&settings.temperature)?,
            ),
            (KEY_TOP_P, serde_json::to_string(&settings.top_p)?),
            (KEY_MAX_TOKENS, serde_json::to_string(&settings.max_tokens)?),
        ];

        for (key, value) in pairs {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![key, value],
            )
            .with_context(|| format!("upsert setting {key}"))?;
        }

        tx.commit().context("commit settings transaction")?;
        Ok(())
    }
}
