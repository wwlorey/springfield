use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::types::{Event, FormaError, Ref, RequiredSection, Section, Spec, SpecDetail};

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

    fn generate_id() -> String {
        let uuid = uuid::Uuid::now_v7();
        let bytes = uuid.as_bytes();
        let hex: String = bytes[12..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!("fm-{hex}")
    }

    fn now_iso() -> String {
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    fn log_event(
        &self,
        spec_stem: &str,
        event_type: &str,
        actor: Option<&str>,
        detail: Option<&str>,
    ) -> Result<(), FormaError> {
        let now = Self::now_iso();
        self.conn
            .execute(
                "INSERT INTO events (spec_stem, event_type, actor, detail, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![spec_stem, event_type, actor, detail, now],
            )
            .map_err(|e| FormaError::Internal(format!("failed to log event: {e}")))?;
        Ok(())
    }

    pub fn create_spec(
        &self,
        stem: &str,
        crate_path: &str,
        purpose: &str,
        actor: Option<&str>,
    ) -> Result<Spec, FormaError> {
        let now = Self::now_iso();

        self.conn
            .execute(
                "INSERT INTO specs (stem, crate_path, purpose, status, created_at, updated_at) VALUES (?1, ?2, ?3, 'draft', ?4, ?5)",
                rusqlite::params![stem, crate_path, purpose, now, now],
            )
            .map_err(|e| {
                if e.to_string().contains("UNIQUE constraint failed") {
                    FormaError::AlreadyExists(format!("spec already exists: {stem}"))
                } else {
                    FormaError::Internal(format!("failed to create spec: {e}"))
                }
            })?;

        for rs in RequiredSection::ALL {
            let id = Self::generate_id();
            self.conn
                .execute(
                    "INSERT INTO sections (id, spec_stem, name, slug, kind, body, position, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'required', '', ?5, ?6, ?7)",
                    rusqlite::params![id, stem, rs.name(), rs.slug(), rs.position(), now, now],
                )
                .map_err(|e| FormaError::Internal(format!("failed to create required section: {e}")))?;
        }

        self.log_event(
            stem,
            "created",
            actor,
            Some(&format!(
                "created spec with crate_path={crate_path}, purpose={purpose}"
            )),
        )?;

        self.conn
            .query_row("SELECT * FROM specs WHERE stem = ?1", [stem], spec_from_row)
            .map_err(|e| FormaError::Internal(format!("failed to read created spec: {e}")))
    }

    pub fn get_spec(&self, stem: &str) -> Result<SpecDetail, FormaError> {
        let spec = self
            .conn
            .query_row("SELECT * FROM specs WHERE stem = ?1", [stem], spec_from_row)
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        let mut stmt = self
            .conn
            .prepare("SELECT * FROM sections WHERE spec_stem = ?1 ORDER BY position")
            .map_err(|e| FormaError::Internal(format!("failed to prepare sections query: {e}")))?;
        let sections: Vec<Section> = stmt
            .query_map([stem], section_from_row)
            .map_err(|e| FormaError::Internal(format!("failed to query sections: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| FormaError::Internal(format!("failed to read sections: {e}")))?;

        let mut stmt = self
            .conn
            .prepare("SELECT s.* FROM specs s INNER JOIN refs r ON s.stem = r.to_stem WHERE r.from_stem = ?1")
            .map_err(|e| FormaError::Internal(format!("failed to prepare refs query: {e}")))?;
        let refs: Vec<Spec> = stmt
            .query_map([stem], spec_from_row)
            .map_err(|e| FormaError::Internal(format!("failed to query refs: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| FormaError::Internal(format!("failed to read refs: {e}")))?;

        Ok(SpecDetail {
            spec,
            sections,
            refs,
        })
    }

    pub fn list_specs(&self, status: Option<&str>) -> Result<Vec<Spec>, FormaError> {
        let specs = if let Some(status) = status {
            let mut stmt = self
                .conn
                .prepare("SELECT * FROM specs WHERE status = ?1 ORDER BY stem")
                .map_err(|e| FormaError::Internal(format!("failed to prepare list query: {e}")))?;
            stmt.query_map([status], spec_from_row)
                .map_err(|e| FormaError::Internal(format!("failed to list specs: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| FormaError::Internal(format!("failed to read specs: {e}")))?
        } else {
            let mut stmt = self
                .conn
                .prepare("SELECT * FROM specs ORDER BY stem")
                .map_err(|e| FormaError::Internal(format!("failed to prepare list query: {e}")))?;
            stmt.query_map([], spec_from_row)
                .map_err(|e| FormaError::Internal(format!("failed to list specs: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| FormaError::Internal(format!("failed to read specs: {e}")))?
        };
        Ok(specs)
    }

    pub fn update_spec(
        &self,
        stem: &str,
        status: Option<&str>,
        crate_path: Option<&str>,
        purpose: Option<&str>,
        actor: Option<&str>,
    ) -> Result<Spec, FormaError> {
        // Verify spec exists
        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        if status.is_none() && crate_path.is_none() && purpose.is_none() {
            return Err(FormaError::ValidationFailed(
                "at least one field must be provided for update".to_string(),
            ));
        }

        let now = Self::now_iso();
        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut detail_parts = Vec::new();
        let mut param_idx = 2u32;

        // Build dynamic SQL — we use a fixed set of possible params
        // so we build the query string and collect params for execution
        struct ParamCollector {
            values: Vec<String>,
        }

        let mut params = ParamCollector {
            values: vec![now.clone()],
        };

        if let Some(s) = status {
            updates.push(format!("status = ?{param_idx}"));
            params.values.push(s.to_string());
            detail_parts.push(format!("status={s}"));
            param_idx += 1;
        }
        if let Some(c) = crate_path {
            updates.push(format!("crate_path = ?{param_idx}"));
            params.values.push(c.to_string());
            detail_parts.push(format!("crate_path={c}"));
            param_idx += 1;
        }
        if let Some(p) = purpose {
            updates.push(format!("purpose = ?{param_idx}"));
            params.values.push(p.to_string());
            detail_parts.push(format!("purpose={p}"));
            param_idx += 1;
        }

        let sql = format!(
            "UPDATE specs SET {} WHERE stem = ?{param_idx}",
            updates.join(", ")
        );
        params.values.push(stem.to_string());

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params
            .values
            .iter()
            .map(|v| v as &dyn rusqlite::types::ToSql)
            .collect();

        self.conn
            .execute(&sql, param_refs.as_slice())
            .map_err(|e| FormaError::Internal(format!("failed to update spec: {e}")))?;

        let detail = format!("updated {}", detail_parts.join(", "));
        self.log_event(stem, "updated", actor, Some(&detail))?;

        self.conn
            .query_row("SELECT * FROM specs WHERE stem = ?1", [stem], spec_from_row)
            .map_err(|e| FormaError::Internal(format!("failed to read updated spec: {e}")))
    }

    pub fn delete_spec(
        &self,
        stem: &str,
        force: bool,
        actor: Option<&str>,
    ) -> Result<(), FormaError> {
        // Verify spec exists
        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        if !force {
            let has_content: bool = self
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sections WHERE spec_stem = ?1 AND body != '')",
                    [stem],
                    |row| row.get(0),
                )
                .map_err(|e| FormaError::Internal(format!("failed to check sections: {e}")))?;

            if has_content {
                return Err(FormaError::ValidationFailed(format!(
                    "spec '{stem}' has sections with content; use --force to delete"
                )));
            }
        }

        self.log_event(
            stem,
            "deleted",
            actor,
            Some(&format!("deleted spec {stem}")),
        )?;

        self.conn
            .execute("DELETE FROM events WHERE spec_stem = ?1", [stem])
            .map_err(|e| FormaError::Internal(format!("failed to delete events: {e}")))?;
        self.conn
            .execute(
                "DELETE FROM refs WHERE from_stem = ?1 OR to_stem = ?1",
                [stem],
            )
            .map_err(|e| FormaError::Internal(format!("failed to delete refs: {e}")))?;
        self.conn
            .execute("DELETE FROM sections WHERE spec_stem = ?1", [stem])
            .map_err(|e| FormaError::Internal(format!("failed to delete sections: {e}")))?;
        self.conn
            .execute("DELETE FROM specs WHERE stem = ?1", [stem])
            .map_err(|e| FormaError::Internal(format!("failed to delete spec: {e}")))?;

        Ok(())
    }

    pub fn search_specs(&self, query: &str) -> Result<Vec<Spec>, FormaError> {
        let pattern = format!("%{query}%");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT s.* FROM specs s
                 LEFT JOIN sections sec ON s.stem = sec.spec_stem
                 WHERE s.stem LIKE ?1 COLLATE NOCASE
                    OR s.purpose LIKE ?1 COLLATE NOCASE
                    OR sec.body LIKE ?1 COLLATE NOCASE
                 ORDER BY s.stem",
            )
            .map_err(|e| FormaError::Internal(format!("failed to prepare search query: {e}")))?;
        let specs = stmt
            .query_map([&pattern], spec_from_row)
            .map_err(|e| FormaError::Internal(format!("failed to search specs: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| FormaError::Internal(format!("failed to read search results: {e}")))?;
        Ok(specs)
    }

    pub fn count_specs(&self, by_status: bool) -> Result<CountResult, FormaError> {
        let total: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM specs", [], |row| row.get(0))
            .map_err(|e| FormaError::Internal(format!("failed to count specs: {e}")))?;

        if !by_status {
            return Ok(CountResult {
                total,
                groups: None,
            });
        }

        let mut stmt = self
            .conn
            .prepare("SELECT status, COUNT(*) FROM specs GROUP BY status ORDER BY status")
            .map_err(|e| FormaError::Internal(format!("failed to prepare count query: {e}")))?;
        let groups: Vec<StatusCount> = stmt
            .query_map([], |row| {
                Ok(StatusCount {
                    status: row.get(0)?,
                    count: row.get(1)?,
                })
            })
            .map_err(|e| FormaError::Internal(format!("failed to count by status: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| FormaError::Internal(format!("failed to read count results: {e}")))?;

        Ok(CountResult {
            total,
            groups: Some(groups),
        })
    }

    pub fn project_status(&self) -> Result<HashMap<String, i64>, FormaError> {
        let mut stmt = self
            .conn
            .prepare("SELECT status, COUNT(*) FROM specs GROUP BY status ORDER BY status")
            .map_err(|e| FormaError::Internal(format!("failed to prepare status query: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| FormaError::Internal(format!("failed to query status: {e}")))?;

        let mut map = HashMap::new();
        for row in rows {
            let (status, count) =
                row.map_err(|e| FormaError::Internal(format!("failed to read status: {e}")))?;
            map.insert(status, count);
        }
        Ok(map)
    }

    pub fn spec_history(&self, stem: &str) -> Result<Vec<Event>, FormaError> {
        // Verify spec exists
        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        let mut stmt = self
            .conn
            .prepare("SELECT * FROM events WHERE spec_stem = ?1 ORDER BY id DESC")
            .map_err(|e| FormaError::Internal(format!("failed to prepare history query: {e}")))?;
        let events = stmt
            .query_map([stem], event_from_row)
            .map_err(|e| FormaError::Internal(format!("failed to query history: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| FormaError::Internal(format!("failed to read history: {e}")))?;
        Ok(events)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountResult {
    pub total: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<StatusCount>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
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

    #[test]
    fn create_spec_returns_draft_with_required_sections() {
        let (db, _p, _d) = test_db();
        let spec = db
            .create_spec("auth", "crates/auth/", "Authentication", Some("tester"))
            .unwrap();
        assert_eq!(spec.stem, "auth");
        assert_eq!(spec.crate_path, "crates/auth/");
        assert_eq!(spec.purpose, "Authentication");
        assert_eq!(spec.status, Status::Draft);

        let section_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sections WHERE spec_stem = 'auth'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(section_count, 5);

        let required_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sections WHERE spec_stem = 'auth' AND kind = 'required'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(required_count, 5);
    }

    #[test]
    fn create_spec_scaffolds_correct_slugs() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();

        let mut stmt = db
            .conn
            .prepare("SELECT slug FROM sections WHERE spec_stem = 'auth' ORDER BY position")
            .unwrap();
        let slugs: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            slugs,
            vec![
                "overview",
                "architecture",
                "dependencies",
                "error-handling",
                "testing"
            ]
        );
    }

    #[test]
    fn create_spec_duplicate_fails() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        let err = db
            .create_spec("auth", "crates/auth/", "Auth again", None)
            .unwrap_err();
        assert_eq!(err.code(), "already_exists");
    }

    #[test]
    fn create_spec_logs_event() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", Some("alice"))
            .unwrap();
        let events = db.spec_history("auth").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "created");
        assert_eq!(events[0].actor, Some("alice".to_string()));
    }

    #[test]
    fn get_spec_returns_detail() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Authentication", None)
            .unwrap();
        let detail = db.get_spec("auth").unwrap();
        assert_eq!(detail.spec.stem, "auth");
        assert_eq!(detail.sections.len(), 5);
        assert!(detail.refs.is_empty());
    }

    #[test]
    fn get_spec_not_found() {
        let (db, _p, _d) = test_db();
        let err = db.get_spec("nonexistent").unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn get_spec_includes_refs() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        db.create_spec("ralph", "crates/ralph/", "Runner", None)
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO refs (from_stem, to_stem) VALUES ('auth', 'ralph')",
                [],
            )
            .unwrap();

        let detail = db.get_spec("auth").unwrap();
        assert_eq!(detail.refs.len(), 1);
        assert_eq!(detail.refs[0].stem, "ralph");
    }

    #[test]
    fn list_specs_returns_all() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        db.create_spec("ralph", "crates/ralph/", "Runner", None)
            .unwrap();

        let specs = db.list_specs(None).unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].stem, "auth");
        assert_eq!(specs[1].stem, "ralph");
    }

    #[test]
    fn list_specs_filters_by_status() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        db.create_spec("ralph", "crates/ralph/", "Runner", None)
            .unwrap();
        db.update_spec("ralph", Some("stable"), None, None, None)
            .unwrap();

        let drafts = db.list_specs(Some("draft")).unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].stem, "auth");

        let stables = db.list_specs(Some("stable")).unwrap();
        assert_eq!(stables.len(), 1);
        assert_eq!(stables[0].stem, "ralph");
    }

    #[test]
    fn update_spec_changes_fields() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();

        let updated = db
            .update_spec(
                "auth",
                Some("stable"),
                Some("crates/auth-v2/"),
                Some("Authentication v2"),
                Some("bob"),
            )
            .unwrap();
        assert_eq!(updated.status, Status::Stable);
        assert_eq!(updated.crate_path, "crates/auth-v2/");
        assert_eq!(updated.purpose, "Authentication v2");
    }

    #[test]
    fn update_spec_partial() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();

        let updated = db
            .update_spec("auth", Some("proven"), None, None, None)
            .unwrap();
        assert_eq!(updated.status, Status::Proven);
        assert_eq!(updated.crate_path, "crates/auth/");
        assert_eq!(updated.purpose, "Auth");
    }

    #[test]
    fn update_spec_not_found() {
        let (db, _p, _d) = test_db();
        let err = db
            .update_spec("nope", Some("stable"), None, None, None)
            .unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn update_spec_no_fields_fails() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        let err = db.update_spec("auth", None, None, None, None).unwrap_err();
        assert_eq!(err.code(), "validation_failed");
    }

    #[test]
    fn update_spec_logs_event() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        db.update_spec("auth", Some("stable"), None, None, Some("bob"))
            .unwrap();

        let events = db.spec_history("auth").unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "updated");
        assert_eq!(events[0].actor, Some("bob".to_string()));
    }

    #[test]
    fn delete_spec_empty_bodies_no_force() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        db.delete_spec("auth", false, None).unwrap();

        let specs = db.list_specs(None).unwrap();
        assert!(specs.is_empty());
    }

    #[test]
    fn delete_spec_with_content_requires_force() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        db.conn
            .execute(
                "UPDATE sections SET body = 'some content' WHERE spec_stem = 'auth' AND slug = 'overview'",
                [],
            )
            .unwrap();

        let err = db.delete_spec("auth", false, None).unwrap_err();
        assert_eq!(err.code(), "validation_failed");

        db.delete_spec("auth", true, None).unwrap();
        let specs = db.list_specs(None).unwrap();
        assert!(specs.is_empty());
    }

    #[test]
    fn delete_spec_not_found() {
        let (db, _p, _d) = test_db();
        let err = db.delete_spec("nope", false, None).unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn delete_spec_removes_sections_refs_events() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        db.create_spec("ralph", "crates/ralph/", "Runner", None)
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO refs (from_stem, to_stem) VALUES ('auth', 'ralph')",
                [],
            )
            .unwrap();

        db.delete_spec("auth", false, None).unwrap();

        let section_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sections WHERE spec_stem = 'auth'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(section_count, 0);

        let ref_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM refs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(ref_count, 0);

        let event_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE spec_stem = 'auth'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(event_count, 0);
    }

    #[test]
    fn search_specs_matches_stem() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Login system", None)
            .unwrap();
        db.create_spec("ralph", "crates/ralph/", "Runner", None)
            .unwrap();

        let results = db.search_specs("auth").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].stem, "auth");
    }

    #[test]
    fn search_specs_matches_purpose() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Login system", None)
            .unwrap();
        db.create_spec("ralph", "crates/ralph/", "Runner", None)
            .unwrap();

        let results = db.search_specs("Login").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].stem, "auth");
    }

    #[test]
    fn search_specs_matches_section_body() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        db.conn
            .execute(
                "UPDATE sections SET body = 'Uses JWT tokens for session management' WHERE spec_stem = 'auth' AND slug = 'overview'",
                [],
            )
            .unwrap();

        let results = db.search_specs("JWT").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].stem, "auth");
    }

    #[test]
    fn search_specs_case_insensitive() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Authentication", None)
            .unwrap();

        let results = db.search_specs("AUTHENTICATION").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_specs_no_duplicates() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "auth system", None)
            .unwrap();
        db.conn
            .execute(
                "UPDATE sections SET body = 'auth overview' WHERE spec_stem = 'auth' AND slug = 'overview'",
                [],
            )
            .unwrap();

        let results = db.search_specs("auth").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn count_specs_total_only() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        db.create_spec("ralph", "crates/ralph/", "Runner", None)
            .unwrap();

        let result = db.count_specs(false).unwrap();
        assert_eq!(result.total, 2);
        assert!(result.groups.is_none());
    }

    #[test]
    fn count_specs_by_status() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        db.create_spec("ralph", "crates/ralph/", "Runner", None)
            .unwrap();
        db.update_spec("ralph", Some("stable"), None, None, None)
            .unwrap();

        let result = db.count_specs(true).unwrap();
        assert_eq!(result.total, 2);
        let groups = result.groups.unwrap();
        assert_eq!(groups.len(), 2);

        let draft = groups.iter().find(|g| g.status == "draft").unwrap();
        assert_eq!(draft.count, 1);
        let stable = groups.iter().find(|g| g.status == "stable").unwrap();
        assert_eq!(stable.count, 1);
    }

    #[test]
    fn project_status_returns_status_map() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        db.create_spec("ralph", "crates/ralph/", "Runner", None)
            .unwrap();
        db.update_spec("ralph", Some("proven"), None, None, None)
            .unwrap();

        let status = db.project_status().unwrap();
        assert_eq!(status.get("draft"), Some(&1));
        assert_eq!(status.get("proven"), Some(&1));
    }

    #[test]
    fn spec_history_ordered_newest_first() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", "crates/auth/", "Auth", None)
            .unwrap();
        db.update_spec("auth", Some("stable"), None, None, None)
            .unwrap();
        db.update_spec("auth", Some("proven"), None, None, None)
            .unwrap();

        let events = db.spec_history("auth").unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, "updated");
        assert_eq!(events[2].event_type, "created");
    }

    #[test]
    fn spec_history_not_found() {
        let (db, _p, _d) = test_db();
        let err = db.spec_history("nope").unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn generate_id_format() {
        let id = Db::generate_id();
        assert!(id.starts_with("fm-"));
        assert_eq!(id.len(), 11); // "fm-" + 8 hex chars
        assert!(id[3..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn full_crud_lifecycle() {
        let (db, _p, _d) = test_db();

        let spec = db
            .create_spec("auth", "crates/auth/", "Authentication", Some("alice"))
            .unwrap();
        assert_eq!(spec.status, Status::Draft);

        let detail = db.get_spec("auth").unwrap();
        assert_eq!(detail.sections.len(), 5);

        let updated = db
            .update_spec("auth", Some("stable"), None, None, Some("bob"))
            .unwrap();
        assert_eq!(updated.status, Status::Stable);

        let all = db.list_specs(None).unwrap();
        assert_eq!(all.len(), 1);

        let count = db.count_specs(false).unwrap();
        assert_eq!(count.total, 1);

        let events = db.spec_history("auth").unwrap();
        assert_eq!(events.len(), 2);

        db.delete_spec("auth", false, Some("alice")).unwrap();
        let all = db.list_specs(None).unwrap();
        assert!(all.is_empty());
    }
}
