use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::types::{Event, FormaError, Ref, Section, Spec};

pub struct Db {
    pub conn: Connection,
    pub forma_dir: PathBuf,
    pub data_dir: PathBuf,
}

fn project_hash(project_dir: &Path) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let canonical = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf());
    let input = format!("forma:{}", canonical.to_string_lossy());
    Sha256::digest(input.as_bytes()).into()
}

pub fn data_dir(project_dir: &Path) -> PathBuf {
    let hash = project_hash(project_dir);
    let hex: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home).join(".local/share/forma").join(hex)
}

pub fn project_port(project_dir: &Path) -> u16 {
    let hash = project_hash(project_dir);
    let raw = u16::from_be_bytes([hash[8], hash[9]]);
    10000 + (raw % 50000)
}

pub fn parse_dt(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

pub fn spec_from_row(row: &rusqlite::Row) -> Result<Spec, rusqlite::Error> {
    let status_str: String = row.get("status")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;
    Ok(Spec {
        stem: row.get("stem")?,
        crate_path: row.get("crate_path")?,
        purpose: row.get("purpose")?,
        status: status_str.parse().unwrap(),
        created_at: parse_dt(&created_at_str),
        updated_at: parse_dt(&updated_at_str),
    })
}

pub fn section_from_row(row: &rusqlite::Row) -> Result<Section, rusqlite::Error> {
    let kind_str: String = row.get("kind")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;
    Ok(Section {
        id: Some(row.get("id")?),
        spec_stem: Some(row.get("spec_stem")?),
        name: row.get("name")?,
        slug: row.get("slug")?,
        kind: kind_str.parse().unwrap(),
        body: row.get("body")?,
        position: row.get("position")?,
        created_at: Some(parse_dt(&created_at_str)),
        updated_at: Some(parse_dt(&updated_at_str)),
    })
}

pub fn event_from_row(row: &rusqlite::Row) -> Result<Event, rusqlite::Error> {
    let created_at_str: String = row.get("created_at")?;
    Ok(Event {
        id: row.get("id")?,
        spec_stem: row.get("spec_stem")?,
        event_type: row.get("event_type")?,
        actor: row.get("actor")?,
        detail: row.get("detail")?,
        created_at: parse_dt(&created_at_str),
    })
}

impl Db {
    pub fn open(project_dir: &Path) -> Result<Db, FormaError> {
        let forma_dir = project_dir.join(".forma");
        let dd = data_dir(project_dir);
        Self::open_with_data_dir(forma_dir, dd)
    }

    pub fn open_with_data_dir(forma_dir: PathBuf, data_dir: PathBuf) -> Result<Db, FormaError> {
        fs::create_dir_all(&forma_dir)
            .map_err(|e| FormaError::Internal(format!("failed to create .forma dir: {e}")))?;
        fs::create_dir_all(&data_dir)
            .map_err(|e| FormaError::Internal(format!("failed to create data dir: {e}")))?;

        let db_path = data_dir.join("db.sqlite");
        let conn = Connection::open(&db_path)
            .map_err(|e| FormaError::Internal(format!("failed to open database: {e}")))?;

        conn.pragma_update(None, "busy_timeout", 5000)
            .map_err(|e| FormaError::Internal(format!("failed to set busy_timeout: {e}")))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| FormaError::Internal(format!("failed to enable foreign_keys: {e}")))?;

        Self::run_migrations(&conn)?;

        let db = Db {
            conn,
            forma_dir: forma_dir.clone(),
            data_dir,
        };

        let spec_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM specs", [], |row| row.get(0))
            .map_err(|e| FormaError::Internal(format!("failed to count specs: {e}")))?;

        if spec_count == 0 && forma_dir.join("specs.jsonl").exists() {
            db.import_jsonl()?;
        }

        Ok(db)
    }

    fn run_migrations(conn: &Connection) -> Result<(), FormaError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS specs (
                stem       TEXT PRIMARY KEY,
                crate_path TEXT NOT NULL,
                purpose    TEXT NOT NULL,
                status     TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'stable', 'proven')),
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sections (
                id         TEXT PRIMARY KEY,
                spec_stem  TEXT NOT NULL REFERENCES specs(stem),
                name       TEXT NOT NULL,
                slug       TEXT NOT NULL,
                kind       TEXT NOT NULL CHECK (kind IN ('required', 'custom')),
                body       TEXT NOT NULL DEFAULT '',
                position   INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(spec_stem, slug)
            );

            CREATE TABLE IF NOT EXISTS refs (
                from_stem TEXT NOT NULL REFERENCES specs(stem),
                to_stem   TEXT NOT NULL REFERENCES specs(stem),
                PRIMARY KEY (from_stem, to_stem),
                CHECK (from_stem != to_stem)
            );

            CREATE TABLE IF NOT EXISTS events (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                spec_stem  TEXT NOT NULL REFERENCES specs(stem),
                event_type TEXT NOT NULL,
                actor      TEXT,
                detail     TEXT,
                created_at TEXT NOT NULL
            );",
        )
        .map_err(|e| FormaError::Internal(format!("migration failed: {e}")))?;

        Ok(())
    }

    pub fn import_jsonl(&self) -> Result<ImportResult, FormaError> {
        let specs_path = self.forma_dir.join("specs.jsonl");
        let sections_path = self.forma_dir.join("sections.jsonl");
        let refs_path = self.forma_dir.join("refs.jsonl");

        self.conn
            .execute_batch(
                "DELETE FROM events;
                 DELETE FROM refs;
                 DELETE FROM sections;
                 DELETE FROM specs;",
            )
            .map_err(|e| FormaError::Internal(format!("failed to clear tables for import: {e}")))?;

        let mut spec_count = 0;
        if specs_path.exists() {
            let content = fs::read_to_string(&specs_path)
                .map_err(|e| FormaError::Internal(format!("failed to read specs.jsonl: {e}")))?;
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let spec: Spec = serde_json::from_str(line)
                    .map_err(|e| FormaError::Internal(format!("failed to parse spec: {e}")))?;
                self.conn
                    .execute(
                        "INSERT INTO specs (stem, crate_path, purpose, status, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        rusqlite::params![
                            spec.stem,
                            spec.crate_path,
                            spec.purpose,
                            spec.status.as_str(),
                            spec.created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                            spec.updated_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                        ],
                    )
                    .map_err(|e| FormaError::Internal(format!("failed to import spec: {e}")))?;
                spec_count += 1;
            }
        }

        let mut section_count = 0;
        if sections_path.exists() {
            let content = fs::read_to_string(&sections_path)
                .map_err(|e| FormaError::Internal(format!("failed to read sections.jsonl: {e}")))?;
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let section: Section = serde_json::from_str(line)
                    .map_err(|e| FormaError::Internal(format!("failed to parse section: {e}")))?;
                self.conn
                    .execute(
                        "INSERT INTO sections (id, spec_stem, name, slug, kind, body, position, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        rusqlite::params![
                            section.id.unwrap_or_default(),
                            section.spec_stem.unwrap_or_default(),
                            section.name,
                            section.slug,
                            section.kind.as_str(),
                            section.body,
                            section.position,
                            section.created_at.map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default(),
                            section.updated_at.map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default(),
                        ],
                    )
                    .map_err(|e| FormaError::Internal(format!("failed to import section: {e}")))?;
                section_count += 1;
            }
        }

        let mut ref_count = 0;
        if refs_path.exists() {
            let content = fs::read_to_string(&refs_path)
                .map_err(|e| FormaError::Internal(format!("failed to read refs.jsonl: {e}")))?;
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let r: Ref = serde_json::from_str(line)
                    .map_err(|e| FormaError::Internal(format!("failed to parse ref: {e}")))?;
                self.conn
                    .execute(
                        "INSERT INTO refs (from_stem, to_stem) VALUES (?1, ?2)",
                        rusqlite::params![r.from_stem, r.to_stem],
                    )
                    .map_err(|e| FormaError::Internal(format!("failed to import ref: {e}")))?;
                ref_count += 1;
            }
        }

        Ok(ImportResult {
            specs: spec_count,
            sections: section_count,
            refs: ref_count,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub specs: usize,
    pub sections: usize,
    pub refs: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SectionKind, Status};
    use tempfile::TempDir;

    fn test_db() -> (Db, TempDir, TempDir) {
        let project_dir = TempDir::new().unwrap();
        let data_dir = TempDir::new().unwrap();
        let forma_dir = project_dir.path().join(".forma");
        let db = Db::open_with_data_dir(forma_dir, data_dir.path().to_path_buf()).unwrap();
        (db, project_dir, data_dir)
    }

    #[test]
    fn open_creates_tables() {
        let (db, _p, _d) = test_db();
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM specs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM sections", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM refs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn pragmas_are_set() {
        let (db, _p, _d) = test_db();
        let busy: i64 = db
            .conn
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .unwrap();
        assert_eq!(busy, 5000);

        let fk: i64 = db
            .conn
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn insert_and_read_spec() {
        let (db, _p, _d) = test_db();
        let now = "2026-03-14T14:30:00Z";
        db.conn
            .execute(
                "INSERT INTO specs (stem, crate_path, purpose, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["auth", "crates/auth/", "Authentication", "draft", now, now],
            )
            .unwrap();

        let spec = db
            .conn
            .query_row(
                "SELECT * FROM specs WHERE stem = ?1",
                ["auth"],
                spec_from_row,
            )
            .unwrap();
        assert_eq!(spec.stem, "auth");
        assert_eq!(spec.crate_path, "crates/auth/");
        assert_eq!(spec.purpose, "Authentication");
        assert_eq!(spec.status, Status::Draft);
    }

    #[test]
    fn insert_and_read_section() {
        let (db, _p, _d) = test_db();
        let now = "2026-03-14T14:30:00Z";
        db.conn
            .execute(
                "INSERT INTO specs (stem, crate_path, purpose, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["auth", "crates/auth/", "Auth", "draft", now, now],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO sections (id, spec_stem, name, slug, kind, body, position, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    "fm-a1b2c3d4",
                    "auth",
                    "Overview",
                    "overview",
                    "required",
                    "Auth overview",
                    0,
                    now,
                    now
                ],
            )
            .unwrap();

        let section = db
            .conn
            .query_row(
                "SELECT * FROM sections WHERE id = ?1",
                ["fm-a1b2c3d4"],
                section_from_row,
            )
            .unwrap();
        assert_eq!(section.name, "Overview");
        assert_eq!(section.slug, "overview");
        assert_eq!(section.kind, SectionKind::Required);
        assert_eq!(section.body, "Auth overview");
        assert_eq!(section.position, 0);
    }

    #[test]
    fn section_unique_slug_per_spec() {
        let (db, _p, _d) = test_db();
        let now = "2026-03-14T14:30:00Z";
        db.conn
            .execute(
                "INSERT INTO specs (stem, crate_path, purpose, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["auth", "crates/auth/", "Auth", "draft", now, now],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO sections (id, spec_stem, name, slug, kind, body, position, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params!["fm-00000001", "auth", "Overview", "overview", "required", "", 0, now, now],
            )
            .unwrap();

        let result = db.conn.execute(
            "INSERT INTO sections (id, spec_stem, name, slug, kind, body, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params!["fm-00000002", "auth", "Overview 2", "overview", "custom", "", 1, now, now],
        );
        assert!(result.is_err());
    }

    #[test]
    fn refs_prevent_self_reference() {
        let (db, _p, _d) = test_db();
        let now = "2026-03-14T14:30:00Z";
        db.conn
            .execute(
                "INSERT INTO specs (stem, crate_path, purpose, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["auth", "crates/auth/", "Auth", "draft", now, now],
            )
            .unwrap();

        let result = db.conn.execute(
            "INSERT INTO refs (from_stem, to_stem) VALUES (?1, ?2)",
            rusqlite::params!["auth", "auth"],
        );
        assert!(result.is_err());
    }

    #[test]
    fn refs_foreign_key_enforcement() {
        let (db, _p, _d) = test_db();
        let result = db.conn.execute(
            "INSERT INTO refs (from_stem, to_stem) VALUES (?1, ?2)",
            rusqlite::params!["nonexistent", "also-nonexistent"],
        );
        assert!(result.is_err());
    }

    #[test]
    fn events_table_works() {
        let (db, _p, _d) = test_db();
        let now = "2026-03-14T14:30:00Z";
        db.conn
            .execute(
                "INSERT INTO specs (stem, crate_path, purpose, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["auth", "crates/auth/", "Auth", "draft", now, now],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO events (spec_stem, event_type, actor, detail, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params!["auth", "created", "test-actor", "created spec", now],
            )
            .unwrap();

        let event = db
            .conn
            .query_row(
                "SELECT * FROM events WHERE spec_stem = ?1",
                ["auth"],
                event_from_row,
            )
            .unwrap();
        assert_eq!(event.spec_stem, "auth");
        assert_eq!(event.event_type, "created");
        assert_eq!(event.actor, Some("test-actor".to_string()));
    }

    #[test]
    fn status_check_constraint() {
        let (db, _p, _d) = test_db();
        let now = "2026-03-14T14:30:00Z";
        let result = db.conn.execute(
            "INSERT INTO specs (stem, crate_path, purpose, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params!["auth", "crates/auth/", "Auth", "invalid_status", now, now],
        );
        assert!(result.is_err());
    }

    #[test]
    fn section_kind_check_constraint() {
        let (db, _p, _d) = test_db();
        let now = "2026-03-14T14:30:00Z";
        db.conn
            .execute(
                "INSERT INTO specs (stem, crate_path, purpose, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["auth", "crates/auth/", "Auth", "draft", now, now],
            )
            .unwrap();
        let result = db.conn.execute(
            "INSERT INTO sections (id, spec_stem, name, slug, kind, body, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params!["fm-00000001", "auth", "Test", "test", "invalid_kind", "", 0, now, now],
        );
        assert!(result.is_err());
    }

    #[test]
    fn port_derivation_uses_forma_prefix() {
        let dir = TempDir::new().unwrap();
        let port = project_port(dir.path());
        assert!(port >= 10000);
        assert!(port < 60000);
    }

    #[test]
    fn data_dir_uses_forma_prefix() {
        let dir = TempDir::new().unwrap();
        let dd = data_dir(dir.path());
        assert!(dd.to_string_lossy().contains("/forma/"));
        assert!(!dd.to_string_lossy().contains("/pensa/"));
    }

    #[test]
    fn auto_import_on_open() {
        let project_dir = TempDir::new().unwrap();
        let data_tmp = TempDir::new().unwrap();
        let forma_dir = project_dir.path().join(".forma");
        fs::create_dir_all(&forma_dir).unwrap();

        fs::write(
            forma_dir.join("specs.jsonl"),
            r#"{"stem":"auth","crate_path":"crates/auth/","purpose":"Authentication","status":"draft","created_at":"2026-03-14T14:30:00Z","updated_at":"2026-03-14T14:30:00Z"}"#,
        )
        .unwrap();
        fs::write(
            forma_dir.join("sections.jsonl"),
            r#"{"id":"fm-a1b2c3d4","spec_stem":"auth","name":"Overview","slug":"overview","kind":"required","body":"Auth overview","position":0,"created_at":"2026-03-14T14:30:00Z","updated_at":"2026-03-14T14:30:00Z"}"#,
        )
        .unwrap();
        fs::write(forma_dir.join("refs.jsonl"), "").unwrap();

        let db = Db::open_with_data_dir(forma_dir, data_tmp.path().to_path_buf()).unwrap();

        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM specs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM sections", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn import_result_counts() {
        let project_dir = TempDir::new().unwrap();
        let data_tmp = TempDir::new().unwrap();
        let forma_dir = project_dir.path().join(".forma");
        fs::create_dir_all(&forma_dir).unwrap();

        let specs = r#"{"stem":"auth","crate_path":"crates/auth/","purpose":"Auth","status":"draft","created_at":"2026-03-14T14:30:00Z","updated_at":"2026-03-14T14:30:00Z"}
{"stem":"ralph","crate_path":"crates/ralph/","purpose":"Runner","status":"stable","created_at":"2026-03-14T14:30:00Z","updated_at":"2026-03-14T14:30:00Z"}"#;
        fs::write(forma_dir.join("specs.jsonl"), specs).unwrap();
        fs::write(forma_dir.join("sections.jsonl"), "").unwrap();
        fs::write(
            forma_dir.join("refs.jsonl"),
            r#"{"from_stem":"auth","to_stem":"ralph"}"#,
        )
        .unwrap();

        let db = Db::open_with_data_dir(forma_dir, data_tmp.path().to_path_buf()).unwrap();

        let spec_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM specs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(spec_count, 2);

        let ref_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM refs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(ref_count, 1);
    }

    #[test]
    fn idempotent_open() {
        let project_dir = TempDir::new().unwrap();
        let data_tmp = TempDir::new().unwrap();
        let forma_dir = project_dir.path().join(".forma");

        let db = Db::open_with_data_dir(forma_dir.clone(), data_tmp.path().to_path_buf()).unwrap();
        let now = "2026-03-14T14:30:00Z";
        db.conn
            .execute(
                "INSERT INTO specs (stem, crate_path, purpose, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["auth", "crates/auth/", "Auth", "draft", now, now],
            )
            .unwrap();
        drop(db);

        let db2 = Db::open_with_data_dir(forma_dir, data_tmp.path().to_path_buf()).unwrap();
        let count: i64 = db2
            .conn
            .query_row("SELECT COUNT(*) FROM specs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
