use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::Connection;

use crate::error::PensaError;

pub struct Db {
    pub conn: Connection,
    pub pensa_dir: PathBuf,
}

impl Db {
    pub fn open(project_dir: &Path) -> Result<Db, PensaError> {
        let pensa_dir = project_dir.join(".pensa");
        fs::create_dir_all(&pensa_dir)
            .map_err(|e| PensaError::Internal(format!("failed to create .pensa dir: {e}")))?;

        let db_path = pensa_dir.join("db.sqlite");
        let conn = Connection::open(&db_path)
            .map_err(|e| PensaError::Internal(format!("failed to open database: {e}")))?;

        conn.pragma_update(None, "busy_timeout", 5000)
            .map_err(|e| PensaError::Internal(format!("failed to set busy_timeout: {e}")))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| PensaError::Internal(format!("failed to enable foreign_keys: {e}")))?;

        Self::run_migrations(&conn)?;

        // TODO: Phase 8 — auto-import from JSONL if tables are empty but JSONL files exist

        Ok(Db { conn, pensa_dir })
    }

    fn run_migrations(conn: &Connection) -> Result<(), PensaError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS issues (
                id          TEXT PRIMARY KEY,
                title       TEXT NOT NULL,
                description TEXT,
                issue_type  TEXT NOT NULL CHECK (issue_type IN ('bug', 'task', 'test', 'chore')),
                status      TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'in_progress', 'closed')),
                priority    TEXT NOT NULL DEFAULT 'p2' CHECK (priority IN ('p0', 'p1', 'p2', 'p3')),
                spec        TEXT,
                fixes       TEXT REFERENCES issues(id),
                assignee    TEXT,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL,
                closed_at   TEXT,
                close_reason TEXT
            );

            CREATE TABLE IF NOT EXISTS deps (
                issue_id      TEXT NOT NULL REFERENCES issues(id),
                depends_on_id TEXT NOT NULL REFERENCES issues(id),
                PRIMARY KEY (issue_id, depends_on_id),
                CHECK (issue_id != depends_on_id)
            );

            CREATE TABLE IF NOT EXISTS comments (
                id         TEXT PRIMARY KEY,
                issue_id   TEXT NOT NULL REFERENCES issues(id),
                actor      TEXT NOT NULL,
                text       TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS events (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                issue_id   TEXT NOT NULL REFERENCES issues(id),
                event_type TEXT NOT NULL,
                actor      TEXT,
                detail     TEXT,
                created_at TEXT NOT NULL
            );",
        )
        .map_err(|e| PensaError::Internal(format!("migration failed: {e}")))?;

        Ok(())
    }
}

pub fn now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_temp_db() -> (Db, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = Db::open(dir.path()).unwrap();
        (db, dir)
    }

    #[test]
    fn open_creates_tables() {
        let (db, _dir) = open_temp_db();

        let tables: Vec<String> = db
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert!(tables.contains(&"issues".to_string()));
        assert!(tables.contains(&"deps".to_string()));
        assert!(tables.contains(&"comments".to_string()));
        assert!(tables.contains(&"events".to_string()));
    }

    #[test]
    fn open_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let _db1 = Db::open(dir.path()).unwrap();
        let _db2 = Db::open(dir.path()).unwrap();
    }

    #[test]
    fn foreign_keys_enforced() {
        let (db, _dir) = open_temp_db();

        let result = db.conn.execute(
            "INSERT INTO deps (issue_id, depends_on_id) VALUES ('nonexistent-a', 'nonexistent-b')",
            [],
        );

        assert!(
            result.is_err(),
            "should reject dep referencing nonexistent issues"
        );
    }
}
