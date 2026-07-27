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
];

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
