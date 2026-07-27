//! Provider CRUD. API keys are encrypted before they reach the database and
//! decrypted on read, so callers only ever see plaintext.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::crypto;
use crate::db::Db;
use crate::models::provider::{NewProvider, Provider, ProviderType, UpdateProvider};

pub struct ProviderService {
    db: Db,
}

impl ProviderService {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub fn create(&self, new: &NewProvider) -> Result<Provider> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let provider = Provider {
            id,
            name: new.name.clone(),
            provider_type: new.provider_type.clone(),
            api_url: normalize_url(&new.api_url),
            api_key: new.api_key.clone().filter(|k| !k.is_empty()),
            is_default: new.is_default,
            created_at: now,
            updated_at: now,
        };

        // Encrypt the key before it touches the database.
        let stored_key = provider
            .api_key
            .as_deref()
            .map(crypto::encrypt)
            .transpose()
            .context("encrypt api key")?;

        let conn = self.db.conn();
        let mut conn = conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;

        if provider.is_default {
            tx.execute("UPDATE providers SET is_default = 0 WHERE is_default = 1", [])?;
        }

        tx.execute(
            "INSERT INTO providers
                (id, name, provider_type, api_url, api_key, is_default, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                provider.id.to_string(),
                provider.name,
                provider_type_str(&provider.provider_type),
                provider.api_url,
                stored_key,
                provider.is_default as i32,
                provider.created_at.to_rfc3339(),
                provider.updated_at.to_rfc3339(),
            ],
        )
        .context("insert provider")?;

        tx.commit()?;
        Ok(provider)
    }

    pub fn get(&self, id: Uuid) -> Result<Option<Provider>> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(SELECT_ALL_COLS_WHERE_ID)?;
        let mut rows = stmt.query(rusqlite::params![id.to_string()])?;
        match rows.next()? {
            Some(row) => {
                let mut provider = row_to_provider(row)?;
                decrypt_key(&mut provider);
                Ok(Some(provider))
            }
            None => Ok(None),
        }
    }

    pub fn list(&self) -> Result<Vec<Provider>> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, name, provider_type, api_url, api_key, is_default, created_at, updated_at
             FROM providers ORDER BY name COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([], row_to_provider)?;
        let mut providers = Vec::new();
        for r in rows {
            let mut provider = r?;
            decrypt_key(&mut provider);
            providers.push(provider);
        }
        Ok(providers)
    }

    pub fn update(&self, update: &UpdateProvider) -> Result<Provider> {
        let conn = self.db.conn();
        let mut conn = conn.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;

        // Load current record.
        let existing: Provider = {
            let mut stmt = tx.prepare(SELECT_ALL_COLS_WHERE_ID)?;
            let mut rows = stmt.query(rusqlite::params![update.id.to_string()])?;
            match rows.next()? {
                Some(row) => row_to_provider(row)?,
                None => return Err(anyhow!("Provider not found")),
            }
        };

        // `existing.api_key` is in stored (encrypted) form here — it comes
        // straight from row_to_provider without the decrypt step.
        let stored_key: Option<String> = match &update.api_key {
            Some(new_key) => new_key
                .clone()
                .filter(|k| !k.is_empty())
                .as_deref()
                .map(crypto::encrypt)
                .transpose()
                .context("encrypt api key")?,
            None => existing.api_key.clone(),
        };

        let mut merged = Provider {
            id: existing.id,
            name: update.name.clone().unwrap_or(existing.name),
            provider_type: update.provider_type.clone().unwrap_or(existing.provider_type),
            api_url: update
                .api_url
                .clone()
                .map(|u| normalize_url(&u))
                .unwrap_or(existing.api_url),
            api_key: stored_key.clone(),
            is_default: update.is_default.unwrap_or(existing.is_default),
            created_at: existing.created_at,
            updated_at: Utc::now(),
        };

        if update.is_default == Some(true) {
            tx.execute(
                "UPDATE providers SET is_default = 0 WHERE is_default = 1 AND id != ?1",
                rusqlite::params![merged.id.to_string()],
            )?;
        }

        tx.execute(
            "UPDATE providers SET
                name = ?2, provider_type = ?3, api_url = ?4, api_key = ?5,
                is_default = ?6, updated_at = ?7
             WHERE id = ?1",
            rusqlite::params![
                merged.id.to_string(),
                merged.name,
                provider_type_str(&merged.provider_type),
                merged.api_url,
                stored_key,
                merged.is_default as i32,
                merged.updated_at.to_rfc3339(),
            ],
        )
        .context("update provider")?;

        tx.commit()?;

        // Return the plaintext form to the caller, like every other read.
        decrypt_key(&mut merged);
        Ok(merged)
    }

    pub fn delete(&self, id: Uuid) -> Result<()> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        conn.execute(
            "DELETE FROM providers WHERE id = ?1",
            rusqlite::params![id.to_string()],
        )
        .context("delete provider")?;
        Ok(())
    }

    pub fn get_default(&self) -> Result<Option<Provider>> {
        let conn = self.db.conn();
        let conn = conn.lock().expect("db mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, name, provider_type, api_url, api_key, is_default, created_at, updated_at
             FROM providers WHERE is_default = 1 LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => {
                let mut provider = row_to_provider(row)?;
                decrypt_key(&mut provider);
                Ok(Some(provider))
            }
            None => Ok(None),
        }
    }

}

/// Replace a provider's stored api_key with its decrypted form. A decryption
/// failure (e.g. deleted/replaced key file) is logged and surfaces as a
/// missing key rather than bricking every provider read.
fn decrypt_key(provider: &mut Provider) {
    if let Some(stored) = provider.api_key.take() {
        match crypto::decrypt(&stored) {
            Ok(plain) => provider.api_key = Some(plain),
            Err(e) => {
                tracing::warn!(
                    "could not decrypt api key for provider {} ({}): {e}",
                    provider.name,
                    provider.id
                );
            }
        }
    }
}

const SELECT_ALL_COLS_WHERE_ID: &str =
    "SELECT id, name, provider_type, api_url, api_key, is_default, created_at, updated_at
     FROM providers WHERE id = ?1";

fn provider_type_str(t: &ProviderType) -> &'static str {
    match t {
        ProviderType::Ollama => "ollama",
        ProviderType::OpenAiCompatible => "openai_compatible",
    }
}

/// Strip a single trailing slash so URLs are stored consistently.
fn normalize_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn row_to_provider(row: &rusqlite::Row) -> rusqlite::Result<Provider> {
    let provider_type_str: String = row.get("provider_type")?;
    let provider_type = match provider_type_str.as_str() {
        "ollama" => ProviderType::Ollama,
        "openai_compatible" => ProviderType::OpenAiCompatible,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("unknown provider type: {other}").into(),
            ))
        }
    };

    let id_str: String = row.get("id")?;
    let created_str: String = row.get("created_at")?;
    let updated_str: String = row.get("updated_at")?;

    Ok(Provider {
        id: id_str.parse().map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                "invalid uuid".into(),
            )
        })?,
        name: row.get("name")?,
        provider_type,
        api_url: row.get("api_url")?,
        api_key: row.get("api_key")?,
        is_default: row.get::<_, i32>("is_default")? == 1,
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

    /// Pin the process-wide crypto key so tests never read or create the real
    /// key file. First caller wins; that's fine since all tests want the same.
    fn use_test_key() {
        let _ = crate::crypto::set_test_key([9; 32]);
    }

    fn test_db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db
    }

    fn new_provider(name: &str, api_key: Option<&str>) -> NewProvider {
        NewProvider {
            name: name.to_string(),
            provider_type: ProviderType::OpenAiCompatible,
            api_url: "https://api.example.com/".to_string(),
            api_key: api_key.map(str::to_string),
            is_default: false,
        }
    }

    fn raw_stored_key(db: &Db, id: Uuid) -> Option<String> {
        let conn = db.conn();
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT api_key FROM providers WHERE id = ?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn api_key_is_encrypted_at_rest_and_decrypted_on_read() {
        use_test_key();
        let db = test_db();
        let svc = ProviderService::new(db.clone());

        let created = svc.create(&new_provider("OpenAI", Some("sk-secret-123"))).unwrap();
        assert_eq!(created.api_key.as_deref(), Some("sk-secret-123"));

        // At rest: encrypted, plaintext nowhere in the stored value.
        let stored = raw_stored_key(&db, created.id).unwrap();
        assert!(crate::crypto::is_encrypted(&stored));
        assert!(!stored.contains("sk-secret-123"));

        // Reads decrypt transparently.
        let fetched = svc.get(created.id).unwrap().unwrap();
        assert_eq!(fetched.api_key.as_deref(), Some("sk-secret-123"));
        let listed = svc.list().unwrap();
        assert_eq!(listed[0].api_key.as_deref(), Some("sk-secret-123"));
    }

    #[test]
    fn update_without_key_change_keeps_existing_key() {
        use_test_key();
        let db = test_db();
        let svc = ProviderService::new(db.clone());
        let created = svc.create(&new_provider("P", Some("sk-original"))).unwrap();

        let updated = svc
            .update(&UpdateProvider {
                id: created.id,
                name: Some("Renamed".to_string()),
                provider_type: None,
                api_url: None,
                api_key: None,
                is_default: None,
            })
            .unwrap();

        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.api_key.as_deref(), Some("sk-original"));
        // Still encrypted at rest, not double-encrypted.
        let stored = raw_stored_key(&db, created.id).unwrap();
        assert!(crate::crypto::is_encrypted(&stored));
        assert_eq!(crate::crypto::decrypt(&stored).unwrap(), "sk-original");
    }

    #[test]
    fn update_with_new_key_re_encrypts() {
        use_test_key();
        let db = test_db();
        let svc = ProviderService::new(db.clone());
        let created = svc.create(&new_provider("P", Some("sk-old"))).unwrap();

        let updated = svc
            .update(&UpdateProvider {
                id: created.id,
                name: None,
                provider_type: None,
                api_url: None,
                api_key: Some(Some("sk-new".to_string())),
                is_default: None,
            })
            .unwrap();

        assert_eq!(updated.api_key.as_deref(), Some("sk-new"));
        let stored = raw_stored_key(&db, created.id).unwrap();
        assert_eq!(crate::crypto::decrypt(&stored).unwrap(), "sk-new");
    }

    #[test]
    fn default_provider_is_exclusive() {
        use_test_key();
        let db = test_db();
        let svc = ProviderService::new(db);

        let mut first = new_provider("A", None);
        first.is_default = true;
        let a = svc.create(&first).unwrap();

        let mut second = new_provider("B", None);
        second.is_default = true;
        let b = svc.create(&second).unwrap();

        let default = svc.get_default().unwrap().unwrap();
        assert_eq!(default.id, b.id);
        assert!(!svc.get(a.id).unwrap().unwrap().is_default);
    }
}
