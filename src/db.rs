//! SQLite connection and versioned schema migrations.
//!
//! A single `rusqlite::Connection` behind `Arc<Mutex<_>>`, cloned into Leptos
//! context. Callers on the async runtime should wrap DB access in
//! `tokio::task::spawn_blocking`.
//!
//! Migration scripts are SQL files in the `migrations/` directory at the repo
//! root, embedded into the binary at compile time.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::config;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Open (creating if needed) the SQLite database at the config path.
    pub fn open() -> Result<Self> {
        config::ensure_dirs().context("Failed to ensure config directories")?;
        let path = config::db_path()?;
        let conn = Connection::open(&path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;

        // Pragmas: WAL for concurrent reads, enforce foreign keys.
        conn.pragma_update(None, "journal_mode", "WAL")
            .context("Failed to set WAL journal mode")?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .context("Failed to enable foreign keys")?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open an in-memory database (used for tests).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Returns a clone of the shared connection handle.
    pub fn conn(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }

    /// Apply any pending migrations, tracked via SQLite's `PRAGMA user_version`.
    ///
    /// Each entry in `MIGRATIONS` is applied exactly once, in order, inside its
    /// own transaction; `user_version` is bumped atomically with each script.
    pub fn run_migrations(&self) -> Result<()> {
        let conn = self.conn.clone();
        let mut conn = conn.lock().expect("db mutex poisoned");

        let current: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .context("read user_version")?;

        for (idx, sql) in MIGRATIONS.iter().enumerate() {
            let version = (idx + 1) as i64;
            if version <= current {
                continue;
            }
            let tx = conn.transaction()?;
            tx.execute_batch(sql)
                .with_context(|| format!("apply migration {version}"))?;
            tx.pragma_update(None, "user_version", version)
                .with_context(|| format!("bump user_version to {version}"))?;
            tx.commit()
                .with_context(|| format!("commit migration {version}"))?;
            tracing::info!("applied database migration {version}");
        }

        Ok(())
    }

    /// The schema version this binary expects (the number of known migrations).
    pub fn latest_version() -> i64 {
        MIGRATIONS.len() as i64
    }

    /// The currently applied schema version.
    pub fn current_version(&self) -> Result<i64> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .context("read user_version")
    }
}

/// Migration scripts, in order; index 0 is schema version 1.
///
/// Scripts live as plain SQL files under `migrations/` and are embedded at
/// compile time. To change the schema, add a new numbered file there and
/// include it at the end of this list — never edit an existing script after
/// release (write ALTER TABLE / backfill statements in the new file instead).
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_initial_schema.sql"),
    include_str!("../migrations/002_session_list_indexes.sql"),
    include_str!("../migrations/003_message_sequence.sql"),
];

#[cfg(test)]
pub(crate) struct TestConfigDir {
    previous_xdg: Option<std::ffi::OsString>,
    root: std::path::PathBuf,
    _lock: std::sync::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl TestConfigDir {
    pub(crate) fn new() -> Self {
        static ENV_LOCK: Mutex<()> = Mutex::new(());
        let lock = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let previous_xdg = std::env::var_os("XDG_CONFIG_HOME");
        let root = std::path::Path::new("/tmp/opencode")
            .join(format!("nanorp-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_var("XDG_CONFIG_HOME", &root);
        config::ensure_dirs().unwrap();
        Self {
            previous_xdg,
            root,
            _lock: lock,
        }
    }
}

#[cfg(test)]
impl Drop for TestConfigDir {
    fn drop(&mut self) {
        match &self.previous_xdg {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_migrates_to_latest_version() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.current_version().unwrap(), 0);

        db.run_migrations().unwrap();
        assert_eq!(db.current_version().unwrap(), Db::latest_version());
    }

    #[test]
    fn migrations_are_idempotent() {
        let db = Db::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.run_migrations().unwrap();
        assert_eq!(db.current_version().unwrap(), Db::latest_version());
    }

    #[test]
    fn message_sequence_migration_preserves_existing_timestamp_order() {
        let db = Db::open_in_memory().unwrap();
        {
            let conn = db.conn();
            let conn = conn.lock().unwrap();
            for sql in MIGRATIONS.iter().take(2) {
                conn.execute_batch(sql).unwrap();
            }
            conn.pragma_update(None, "user_version", 2).unwrap();
            conn.execute(
                "INSERT INTO characters (id, name, created_at, updated_at) VALUES ('c', 'C', 'now', 'now')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO chat_sessions (id, character_id, created_at, updated_at) VALUES ('s', 'c', 'now', 'now')",
                [],
            )
            .unwrap();
            for (id, content) in [("b", "first"), ("a", "second")] {
                conn.execute(
                    "INSERT INTO messages (id, session_id, role, content, created_at)
                     VALUES (?1, 's', 'user', ?2, '2026-01-01T00:00:00Z')",
                    rusqlite::params![id, content],
                )
                .unwrap();
            }
        }

        db.run_migrations().unwrap();
        let conn = db.conn();
        let conn = conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT content, sequence FROM messages ORDER BY sequence")
            .unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![("first".to_string(), 1), ("second".to_string(), 2)]
        );
    }

    #[test]
    fn all_tables_exist_after_migration() {
        let db = Db::open_in_memory().unwrap();
        db.run_migrations().unwrap();

        let conn = db.conn();
        let conn = conn.lock().unwrap();
        for table in [
            "providers",
            "characters",
            "chat_sessions",
            "messages",
            "attachments",
            "settings",
        ] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "table {table} should exist");
        }
    }
}
