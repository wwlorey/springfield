use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::types::{
    Event, FormaError, Ref, RefTreeNode, RequiredSection, Section, SectionKind, Spec, SpecDetail,
    slugify,
};

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

pub fn pensa_port(project_dir: &Path) -> u16 {
    use sha2::{Digest, Sha256};
    let canonical = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf());
    let hash: [u8; 32] = Sha256::digest(canonical.to_string_lossy().as_bytes()).into();
    let raw = u16::from_be_bytes([hash[8], hash[9]]);
    10000 + (raw % 50000)
}

pub fn pensa_url(project_dir: &Path) -> String {
    let pensa_port_file = project_dir.join(".pensa/daemon.port");
    if let Ok(contents) = std::fs::read_to_string(&pensa_port_file)
        && let Ok(port) = contents.trim().parse::<u16>()
    {
        return format!("http://localhost:{port}");
    }
    let port = pensa_port(project_dir);
    format!("http://localhost:{port}")
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
        src: row.get("src")?,
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
                src        TEXT,
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

        let has_crate_path: bool = conn
            .prepare("PRAGMA table_info(specs)")
            .and_then(|mut stmt| {
                let cols: Vec<String> = stmt
                    .query_map([], |row| row.get::<_, String>(1))
                    .unwrap()
                    .filter_map(|r| r.ok())
                    .collect();
                Ok(cols.contains(&"crate_path".to_string()))
            })
            .unwrap_or(false);

        if has_crate_path {
            conn.execute_batch(
                "ALTER TABLE specs RENAME COLUMN crate_path TO src;",
            )
            .map_err(|e| FormaError::Internal(format!("migration (crate_path->src) failed: {e}")))?;
        }

        Ok(())
    }

    fn generate_id() -> String {
        let uuid = uuid::Uuid::now_v7();
        let bytes = uuid.as_bytes();
        let hex: String = bytes[12..16].iter().map(|b| format!("{b:02x}")).collect();
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
        src: Option<&str>,
        purpose: &str,
        actor: Option<&str>,
    ) -> Result<Spec, FormaError> {
        let now = Self::now_iso();

        self.conn
            .execute(
                "INSERT INTO specs (stem, src, purpose, status, created_at, updated_at) VALUES (?1, ?2, ?3, 'draft', ?4, ?5)",
                rusqlite::params![stem, src, purpose, now, now],
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
                "created spec with src={}, purpose={purpose}",
                src.unwrap_or("(none)")
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
        src: Option<&str>,
        purpose: Option<&str>,
        actor: Option<&str>,
    ) -> Result<Spec, FormaError> {
        // Verify spec exists
        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        if status.is_none() && src.is_none() && purpose.is_none() {
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
        if let Some(s) = src {
            updates.push(format!("src = ?{param_idx}"));
            params.values.push(s.to_string());
            detail_parts.push(format!("src={s}"));
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

    pub fn add_section(
        &self,
        stem: &str,
        name: &str,
        body: &str,
        after_slug: Option<&str>,
        actor: Option<&str>,
    ) -> Result<Section, FormaError> {
        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        let slug = slugify(name);

        let exists: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sections WHERE spec_stem = ?1 AND slug = ?2)",
                rusqlite::params![stem, slug],
                |row| row.get(0),
            )
            .map_err(|e| FormaError::Internal(format!("failed to check slug uniqueness: {e}")))?;
        if exists {
            return Err(FormaError::AlreadyExists(format!(
                "section with slug '{slug}' already exists in spec '{stem}'"
            )));
        }

        let position = if let Some(after) = after_slug {
            let after_pos: i64 = self
                .conn
                .query_row(
                    "SELECT position FROM sections WHERE spec_stem = ?1 AND slug = ?2",
                    rusqlite::params![stem, after],
                    |row| row.get(0),
                )
                .map_err(|_| {
                    FormaError::NotFound(format!("section '{after}' not found in spec '{stem}'"))
                })?;

            self.conn
                .execute(
                    "UPDATE sections SET position = position + 1 WHERE spec_stem = ?1 AND position > ?2",
                    rusqlite::params![stem, after_pos],
                )
                .map_err(|e| FormaError::Internal(format!("failed to shift positions: {e}")))?;

            after_pos + 1
        } else {
            let max_pos: Option<i64> = self
                .conn
                .query_row(
                    "SELECT MAX(position) FROM sections WHERE spec_stem = ?1",
                    [stem],
                    |row| row.get(0),
                )
                .map_err(|e| FormaError::Internal(format!("failed to get max position: {e}")))?;
            max_pos.map_or(0, |p| p + 1)
        };

        let id = Self::generate_id();
        let now = Self::now_iso();
        self.conn
            .execute(
                "INSERT INTO sections (id, spec_stem, name, slug, kind, body, position, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'custom', ?5, ?6, ?7, ?8)",
                rusqlite::params![id, stem, name, slug, body, position, now, now],
            )
            .map_err(|e| FormaError::Internal(format!("failed to add section: {e}")))?;

        self.log_event(
            stem,
            "section_added",
            actor,
            Some(&format!("added section '{slug}'")),
        )?;

        self.conn
            .query_row(
                "SELECT * FROM sections WHERE id = ?1",
                [&id],
                section_from_row,
            )
            .map_err(|e| FormaError::Internal(format!("failed to read added section: {e}")))
    }

    pub fn set_section(
        &self,
        stem: &str,
        slug: &str,
        body: &str,
        actor: Option<&str>,
    ) -> Result<Section, FormaError> {
        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        let now = Self::now_iso();
        let rows = self
            .conn
            .execute(
                "UPDATE sections SET body = ?1, updated_at = ?2 WHERE spec_stem = ?3 AND slug = ?4",
                rusqlite::params![body, now, stem, slug],
            )
            .map_err(|e| FormaError::Internal(format!("failed to set section body: {e}")))?;

        if rows == 0 {
            return Err(FormaError::NotFound(format!(
                "section '{slug}' not found in spec '{stem}'"
            )));
        }

        self.log_event(
            stem,
            "section_updated",
            actor,
            Some(&format!("updated section '{slug}'")),
        )?;

        self.conn
            .query_row(
                "SELECT * FROM sections WHERE spec_stem = ?1 AND slug = ?2",
                rusqlite::params![stem, slug],
                section_from_row,
            )
            .map_err(|e| FormaError::Internal(format!("failed to read updated section: {e}")))
    }

    pub fn get_section(&self, stem: &str, slug: &str) -> Result<Section, FormaError> {
        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        self.conn
            .query_row(
                "SELECT * FROM sections WHERE spec_stem = ?1 AND slug = ?2",
                rusqlite::params![stem, slug],
                section_from_row,
            )
            .map_err(|_| {
                FormaError::NotFound(format!("section '{slug}' not found in spec '{stem}'"))
            })
    }

    pub fn list_sections(&self, stem: &str) -> Result<Vec<Section>, FormaError> {
        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        let mut stmt = self
            .conn
            .prepare("SELECT * FROM sections WHERE spec_stem = ?1 ORDER BY position")
            .map_err(|e| FormaError::Internal(format!("failed to prepare sections query: {e}")))?;
        let sections = stmt
            .query_map([stem], section_from_row)
            .map_err(|e| FormaError::Internal(format!("failed to list sections: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| FormaError::Internal(format!("failed to read sections: {e}")))?;
        Ok(sections)
    }

    pub fn remove_section(
        &self,
        stem: &str,
        slug: &str,
        actor: Option<&str>,
    ) -> Result<(), FormaError> {
        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        let (kind_str, position): (String, i64) = self
            .conn
            .query_row(
                "SELECT kind, position FROM sections WHERE spec_stem = ?1 AND slug = ?2",
                rusqlite::params![stem, slug],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| {
                FormaError::NotFound(format!("section '{slug}' not found in spec '{stem}'"))
            })?;

        let kind: SectionKind = kind_str.parse().unwrap();
        if kind == SectionKind::Required {
            return Err(FormaError::RequiredSection(slug.to_string()));
        }

        self.conn
            .execute(
                "DELETE FROM sections WHERE spec_stem = ?1 AND slug = ?2",
                rusqlite::params![stem, slug],
            )
            .map_err(|e| FormaError::Internal(format!("failed to remove section: {e}")))?;

        self.conn
            .execute(
                "UPDATE sections SET position = position - 1 WHERE spec_stem = ?1 AND position > ?2",
                rusqlite::params![stem, position],
            )
            .map_err(|e| FormaError::Internal(format!("failed to renumber positions: {e}")))?;

        self.log_event(
            stem,
            "section_removed",
            actor,
            Some(&format!("removed section '{slug}'")),
        )?;

        Ok(())
    }

    pub fn move_section(
        &self,
        stem: &str,
        slug: &str,
        after_slug: &str,
        actor: Option<&str>,
    ) -> Result<Section, FormaError> {
        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        let old_pos: i64 = self
            .conn
            .query_row(
                "SELECT position FROM sections WHERE spec_stem = ?1 AND slug = ?2",
                rusqlite::params![stem, slug],
                |row| row.get(0),
            )
            .map_err(|_| {
                FormaError::NotFound(format!("section '{slug}' not found in spec '{stem}'"))
            })?;

        let after_pos: i64 = self
            .conn
            .query_row(
                "SELECT position FROM sections WHERE spec_stem = ?1 AND slug = ?2",
                rusqlite::params![stem, after_slug],
                |row| row.get(0),
            )
            .map_err(|_| {
                FormaError::NotFound(format!("section '{after_slug}' not found in spec '{stem}'"))
            })?;

        // Use a temporary position to avoid unique constraint issues during renumbering
        self.conn
            .execute(
                "UPDATE sections SET position = -1 WHERE spec_stem = ?1 AND slug = ?2",
                rusqlite::params![stem, slug],
            )
            .map_err(|e| FormaError::Internal(format!("failed to move section: {e}")))?;

        if old_pos < after_pos {
            // Moving down: shift items between old_pos+1..=after_pos up by -1
            self.conn
                .execute(
                    "UPDATE sections SET position = position - 1 WHERE spec_stem = ?1 AND position > ?2 AND position <= ?3",
                    rusqlite::params![stem, old_pos, after_pos],
                )
                .map_err(|e| FormaError::Internal(format!("failed to shift positions: {e}")))?;
        } else {
            // Moving up: shift items between after_pos+1..old_pos-1 down by +1
            self.conn
                .execute(
                    "UPDATE sections SET position = position + 1 WHERE spec_stem = ?1 AND position > ?2 AND position < ?3",
                    rusqlite::params![stem, after_pos, old_pos],
                )
                .map_err(|e| FormaError::Internal(format!("failed to shift positions: {e}")))?;
        }

        let new_pos = if old_pos < after_pos {
            after_pos
        } else {
            after_pos + 1
        };

        let now = Self::now_iso();
        self.conn
            .execute(
                "UPDATE sections SET position = ?1, updated_at = ?2 WHERE spec_stem = ?3 AND slug = ?4",
                rusqlite::params![new_pos, now, stem, slug],
            )
            .map_err(|e| FormaError::Internal(format!("failed to set new position: {e}")))?;

        self.log_event(
            stem,
            "section_moved",
            actor,
            Some(&format!("moved section '{slug}' after '{after_slug}'")),
        )?;

        self.conn
            .query_row(
                "SELECT * FROM sections WHERE spec_stem = ?1 AND slug = ?2",
                rusqlite::params![stem, slug],
                section_from_row,
            )
            .map_err(|e| FormaError::Internal(format!("failed to read moved section: {e}")))
    }

    pub fn add_ref(
        &self,
        from_stem: &str,
        to_stem: &str,
        actor: Option<&str>,
    ) -> Result<(), FormaError> {
        self.conn
            .query_row(
                "SELECT stem FROM specs WHERE stem = ?1",
                [from_stem],
                |row| row.get::<_, String>(0),
            )
            .map_err(|_| FormaError::NotFound(format!("spec not found: {from_stem}")))?;

        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [to_stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {to_stem}")))?;

        let exists: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM refs WHERE from_stem = ?1 AND to_stem = ?2)",
                rusqlite::params![from_stem, to_stem],
                |row| row.get(0),
            )
            .map_err(|e| FormaError::Internal(format!("failed to check ref: {e}")))?;
        if exists {
            return Err(FormaError::AlreadyExists(format!(
                "ref from '{from_stem}' to '{to_stem}' already exists"
            )));
        }

        if self.would_create_cycle(from_stem, to_stem)? {
            return Err(FormaError::CycleDetected);
        }

        self.conn
            .execute(
                "INSERT INTO refs (from_stem, to_stem) VALUES (?1, ?2)",
                rusqlite::params![from_stem, to_stem],
            )
            .map_err(|e| FormaError::Internal(format!("failed to add ref: {e}")))?;

        self.log_event(
            from_stem,
            "ref_added",
            actor,
            Some(&format!("added ref to {to_stem}")),
        )?;

        Ok(())
    }

    pub fn remove_ref(
        &self,
        from_stem: &str,
        to_stem: &str,
        actor: Option<&str>,
    ) -> Result<(), FormaError> {
        let changed = self
            .conn
            .execute(
                "DELETE FROM refs WHERE from_stem = ?1 AND to_stem = ?2",
                rusqlite::params![from_stem, to_stem],
            )
            .map_err(|e| FormaError::Internal(format!("failed to remove ref: {e}")))?;

        if changed == 0 {
            return Err(FormaError::NotFound(format!(
                "ref from '{from_stem}' to '{to_stem}' not found"
            )));
        }

        self.log_event(
            from_stem,
            "ref_removed",
            actor,
            Some(&format!("removed ref to {to_stem}")),
        )?;

        Ok(())
    }

    pub fn list_refs(&self, stem: &str) -> Result<Vec<Spec>, FormaError> {
        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT s.* FROM specs s
                 INNER JOIN refs r ON s.stem = r.to_stem
                 WHERE r.from_stem = ?1
                 ORDER BY s.stem",
            )
            .map_err(|e| FormaError::Internal(format!("failed to prepare ref list: {e}")))?;

        let specs = stmt
            .query_map([stem], spec_from_row)
            .map_err(|e| FormaError::Internal(format!("failed to list refs: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| FormaError::Internal(format!("failed to collect refs: {e}")))?;

        Ok(specs)
    }

    pub fn ref_tree(&self, stem: &str, direction: &str) -> Result<Vec<RefTreeNode>, FormaError> {
        self.conn
            .query_row("SELECT stem FROM specs WHERE stem = ?1", [stem], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|_| FormaError::NotFound(format!("spec not found: {stem}")))?;

        let sql = if direction == "up" {
            "WITH RECURSIVE tree(stem, depth) AS (
                 SELECT ?1, 0
                 UNION
                 SELECT r.from_stem, tree.depth + 1
                 FROM refs r
                 INNER JOIN tree ON r.to_stem = tree.stem
             )
             SELECT s.stem, s.purpose, s.status, t.depth
             FROM tree t
             INNER JOIN specs s ON s.stem = t.stem
             ORDER BY t.depth, s.stem"
        } else {
            "WITH RECURSIVE tree(stem, depth) AS (
                 SELECT ?1, 0
                 UNION
                 SELECT r.to_stem, tree.depth + 1
                 FROM refs r
                 INNER JOIN tree ON r.from_stem = tree.stem
             )
             SELECT s.stem, s.purpose, s.status, t.depth
             FROM tree t
             INNER JOIN specs s ON s.stem = t.stem
             ORDER BY t.depth, s.stem"
        };

        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| FormaError::Internal(format!("failed to prepare ref tree: {e}")))?;

        let nodes = stmt
            .query_map([stem], |row| {
                Ok(RefTreeNode {
                    stem: row.get(0)?,
                    purpose: row.get(1)?,
                    status: row.get(2)?,
                    depth: row.get(3)?,
                })
            })
            .map_err(|e| FormaError::Internal(format!("failed to query ref tree: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| FormaError::Internal(format!("failed to collect ref tree: {e}")))?;

        Ok(nodes)
    }

    pub fn detect_cycles(&self) -> Result<Vec<Vec<String>>, FormaError> {
        let mut all_refs: HashMap<String, Vec<String>> = HashMap::new();
        let mut stmt = self
            .conn
            .prepare("SELECT from_stem, to_stem FROM refs")
            .map_err(|e| FormaError::Internal(format!("failed to prepare refs query: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| FormaError::Internal(format!("failed to query refs: {e}")))?;

        let mut all_stems: std::collections::HashSet<String> = std::collections::HashSet::new();
        for row in rows {
            let (from, to) = row.map_err(|e| FormaError::Internal(format!("row error: {e}")))?;
            all_stems.insert(from.clone());
            all_stems.insert(to.clone());
            all_refs.entry(from).or_default().push(to);
        }

        let mut cycles: Vec<Vec<String>> = Vec::new();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();

        for start in &all_stems {
            if visited.contains(start) {
                continue;
            }
            let mut stack: Vec<(String, Vec<String>)> = vec![(start.clone(), vec![start.clone()])];
            let mut in_path: std::collections::HashSet<String> = std::collections::HashSet::new();
            in_path.insert(start.clone());

            while let Some((node, path)) = stack.pop() {
                if let Some(neighbors) = all_refs.get(&node) {
                    for next in neighbors {
                        if next == start && path.len() > 1 {
                            let mut cycle = path.clone();
                            cycle.push(next.clone());
                            cycles.push(cycle);
                        } else if !in_path.contains(next) {
                            in_path.insert(next.clone());
                            let mut new_path = path.clone();
                            new_path.push(next.clone());
                            stack.push((next.clone(), new_path));
                        }
                    }
                }
                visited.insert(node);
            }
        }

        Ok(cycles)
    }

    fn would_create_cycle(&self, from_stem: &str, to_stem: &str) -> Result<bool, FormaError> {
        use std::collections::VecDeque;
        let mut queue = VecDeque::new();
        let mut seen = std::collections::HashSet::new();
        queue.push_back(to_stem.to_string());
        seen.insert(to_stem.to_string());

        while let Some(current) = queue.pop_front() {
            if current == from_stem {
                return Ok(true);
            }
            let mut stmt = self
                .conn
                .prepare("SELECT to_stem FROM refs WHERE from_stem = ?1")
                .map_err(|e| FormaError::Internal(format!("cycle check failed: {e}")))?;
            let neighbors = stmt
                .query_map([&current], |row| row.get::<_, String>(0))
                .map_err(|e| FormaError::Internal(format!("cycle check failed: {e}")))?;
            for n in neighbors {
                let n = n.map_err(|e| FormaError::Internal(format!("cycle check failed: {e}")))?;
                if seen.insert(n.clone()) {
                    queue.push_back(n);
                }
            }
        }

        Ok(false)
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
                        "INSERT INTO specs (stem, src, purpose, status, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        rusqlite::params![
                            spec.stem,
                            spec.src,
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

    pub fn export_jsonl(&self) -> Result<ExportResult, FormaError> {
        let specs = self.list_specs(None)?;

        let all_sections: Vec<Section> = {
            let mut stmt = self
                .conn
                .prepare("SELECT * FROM sections ORDER BY spec_stem, position")
                .map_err(|e| {
                    FormaError::Internal(format!("failed to prepare sections export: {e}"))
                })?;
            stmt.query_map([], section_from_row)
                .map_err(|e| FormaError::Internal(format!("failed to export sections: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| FormaError::Internal(format!("failed to read sections: {e}")))?
        };

        let all_refs: Vec<Ref> = {
            let mut stmt = self
                .conn
                .prepare("SELECT from_stem, to_stem FROM refs ORDER BY from_stem, to_stem")
                .map_err(|e| FormaError::Internal(format!("failed to prepare refs export: {e}")))?;
            stmt.query_map([], |row| {
                Ok(Ref {
                    from_stem: row.get(0)?,
                    to_stem: row.get(1)?,
                })
            })
            .map_err(|e| FormaError::Internal(format!("failed to export refs: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| FormaError::Internal(format!("failed to read refs: {e}")))?
        };

        // Write JSONL files
        let mut specs_jsonl = String::new();
        for spec in &specs {
            specs_jsonl.push_str(
                &serde_json::to_string(spec)
                    .map_err(|e| FormaError::Internal(format!("failed to serialize spec: {e}")))?,
            );
            specs_jsonl.push('\n');
        }
        fs::write(self.forma_dir.join("specs.jsonl"), &specs_jsonl)
            .map_err(|e| FormaError::Internal(format!("failed to write specs.jsonl: {e}")))?;

        let mut sections_jsonl = String::new();
        for section in &all_sections {
            sections_jsonl.push_str(
                &serde_json::to_string(section).map_err(|e| {
                    FormaError::Internal(format!("failed to serialize section: {e}"))
                })?,
            );
            sections_jsonl.push('\n');
        }
        fs::write(self.forma_dir.join("sections.jsonl"), &sections_jsonl)
            .map_err(|e| FormaError::Internal(format!("failed to write sections.jsonl: {e}")))?;

        let mut refs_jsonl = String::new();
        for r in &all_refs {
            refs_jsonl.push_str(
                &serde_json::to_string(r)
                    .map_err(|e| FormaError::Internal(format!("failed to serialize ref: {e}")))?,
            );
            refs_jsonl.push('\n');
        }
        fs::write(self.forma_dir.join("refs.jsonl"), &refs_jsonl)
            .map_err(|e| FormaError::Internal(format!("failed to write refs.jsonl: {e}")))?;

        // Generate markdown specs
        let specs_md_dir = self.forma_dir.join("specs");
        fs::create_dir_all(&specs_md_dir)
            .map_err(|e| FormaError::Internal(format!("failed to create specs dir: {e}")))?;

        let refs_by_stem: HashMap<String, Vec<&Ref>> = {
            let mut map: HashMap<String, Vec<&Ref>> = HashMap::new();
            for r in &all_refs {
                map.entry(r.from_stem.clone()).or_default().push(r);
            }
            map
        };

        let spec_map: HashMap<&str, &Spec> = specs.iter().map(|s| (s.stem.as_str(), s)).collect();

        let sections_by_stem: HashMap<String, Vec<&Section>> = {
            let mut map: HashMap<String, Vec<&Section>> = HashMap::new();
            for sec in &all_sections {
                if let Some(stem) = &sec.spec_stem {
                    map.entry(stem.clone()).or_default().push(sec);
                }
            }
            map
        };

        for spec in &specs {
            let mut md = String::new();
            md.push_str(&format!("# {} Specification\n\n", spec.stem));
            md.push_str(&format!("{}\n\n", spec.purpose));
            md.push_str("| Field | Value |\n");
            md.push_str("|-------|-------|\n");
            if let Some(src) = &spec.src {
                md.push_str(&format!("| Src | `{src}` |\n"));
            }
            md.push_str(&format!("| Status | {} |\n", spec.status));

            if let Some(sections) = sections_by_stem.get(&spec.stem) {
                for sec in sections {
                    md.push_str(&format!("\n## {}\n\n", sec.name));
                    md.push_str(&sec.body);
                    if !sec.body.is_empty() && !sec.body.ends_with('\n') {
                        md.push('\n');
                    }
                }
            }

            if let Some(refs) = refs_by_stem.get(&spec.stem) {
                md.push_str("\n## Related Specifications\n\n");
                for r in refs {
                    let purpose = spec_map
                        .get(r.to_stem.as_str())
                        .map(|s| s.purpose.as_str())
                        .unwrap_or("");
                    md.push_str(&format!(
                        "- [{}]({}.md) — {}\n",
                        r.to_stem, r.to_stem, purpose
                    ));
                }
            }

            fs::write(specs_md_dir.join(format!("{}.md", spec.stem)), &md)
                .map_err(|e| FormaError::Internal(format!("failed to write spec markdown: {e}")))?;
        }

        // Generate README.md
        let mut readme = String::new();
        readme.push_str("# Specifications\n\n");
        readme.push_str("| Spec | Src | Status | Purpose |\n");
        readme.push_str("|------|-----|--------|--------|\n");
        for spec in &specs {
            let src_col = spec
                .src
                .as_deref()
                .map(|s| format!("`{s}`"))
                .unwrap_or_default();
            readme.push_str(&format!(
                "| [{}](specs/{}.md) | {} | {} | {} |\n",
                spec.stem, spec.stem, src_col, spec.status, spec.purpose
            ));
        }
        fs::write(self.forma_dir.join("README.md"), &readme)
            .map_err(|e| FormaError::Internal(format!("failed to write README.md: {e}")))?;

        Ok(ExportResult {
            specs: specs.len(),
            sections: all_sections.len(),
            refs: all_refs.len(),
        })
    }

    pub fn check(&self, pensa_url: Option<&str>) -> Result<CheckReport, FormaError> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        let specs = self.list_specs(None)?;
        let required_slugs: Vec<&str> = RequiredSection::ALL.iter().map(|rs| rs.slug()).collect();

        for spec in &specs {
            // Required sections present
            let mut stmt = self
                .conn
                .prepare("SELECT slug FROM sections WHERE spec_stem = ?1 AND kind = 'required'")
                .map_err(|e| FormaError::Internal(format!("check query failed: {e}")))?;
            let slugs: Vec<String> = stmt
                .query_map([&spec.stem], |row| row.get::<_, String>(0))
                .map_err(|e| FormaError::Internal(format!("check query failed: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| FormaError::Internal(format!("check query failed: {e}")))?;

            for &rs in &required_slugs {
                if !slugs.iter().any(|s| s == rs) {
                    errors.push(CheckFinding {
                        check: "required_sections_present".to_string(),
                        message: format!("spec '{}' missing required section '{}'", spec.stem, rs),
                    });
                }
            }

            // Required sections non-empty
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT slug, body FROM sections WHERE spec_stem = ?1 AND kind = 'required'",
                )
                .map_err(|e| FormaError::Internal(format!("check query failed: {e}")))?;
            let rows: Vec<(String, String)> = stmt
                .query_map([&spec.stem], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|e| FormaError::Internal(format!("check query failed: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| FormaError::Internal(format!("check query failed: {e}")))?;

            for (slug, body) in &rows {
                if body.trim().is_empty() {
                    warnings.push(CheckFinding {
                        check: "required_sections_nonempty".to_string(),
                        message: format!(
                            "spec '{}' has empty required section '{}'",
                            spec.stem, slug
                        ),
                    });
                }
            }

            if let Some(src) = &spec.src {
                let project_dir = self.forma_dir.parent().unwrap_or(Path::new("."));
                let src_path = project_dir.join(src);
                if !src_path.exists() {
                    errors.push(CheckFinding {
                        check: "src_paths_exist".to_string(),
                        message: format!(
                            "spec '{}' src '{}' does not exist on disk",
                            spec.stem, src
                        ),
                    });
                }
            }

            // No duplicate slugs within a spec
            let dup_count: i64 = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM (SELECT slug FROM sections WHERE spec_stem = ?1 GROUP BY slug HAVING COUNT(*) > 1)",
                    [&spec.stem],
                    |row| row.get(0),
                )
                .map_err(|e| FormaError::Internal(format!("check query failed: {e}")))?;
            if dup_count > 0 {
                errors.push(CheckFinding {
                    check: "no_duplicate_slugs".to_string(),
                    message: format!("spec '{}' has duplicate section slugs", spec.stem),
                });
            }
        }

        // Ref targets exist
        let mut stmt = self
            .conn
            .prepare("SELECT from_stem, to_stem FROM refs")
            .map_err(|e| FormaError::Internal(format!("check query failed: {e}")))?;
        let refs: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| FormaError::Internal(format!("check query failed: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| FormaError::Internal(format!("check query failed: {e}")))?;

        let spec_stems: std::collections::HashSet<&str> =
            specs.iter().map(|s| s.stem.as_str()).collect();

        for (from, to) in &refs {
            if !spec_stems.contains(to.as_str()) {
                errors.push(CheckFinding {
                    check: "ref_targets_exist".to_string(),
                    message: format!("ref from '{}' to '{}' targets non-existent spec", from, to),
                });
            }
        }

        // No ref cycles
        let cycles = self.detect_cycles()?;
        for cycle in &cycles {
            errors.push(CheckFinding {
                check: "no_ref_cycles".to_string(),
                message: format!("ref cycle detected: {}", cycle.join(" -> ")),
            });
        }

        // Pensa integration: validate spec references
        if let Some(url) = pensa_url {
            match reqwest::blocking::Client::new()
                .get(format!("{url}/issues"))
                .query(&[("status", "open"), ("status", "in_progress")])
                .timeout(std::time::Duration::from_secs(3))
                .send()
            {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(issues) = resp.json::<Vec<serde_json::Value>>() {
                        for issue in &issues {
                            if let Some(spec_val) = issue.get("spec")
                                && !spec_val.is_null()
                                && let Some(spec_stem) = spec_val.as_str()
                                && !spec_stems.contains(spec_stem)
                            {
                                let id = issue.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                                warnings.push(CheckFinding {
                                    check: "pensa_spec_refs_valid".to_string(),
                                    message: format!(
                                        "pensa issue '{}' references non-existent spec '{}'",
                                        id, spec_stem
                                    ),
                                });
                            }
                        }
                    }
                }
                _ => {
                    warnings.push(CheckFinding {
                        check: "pensa_spec_refs_valid".to_string(),
                        message: "pensa daemon unreachable, skipping spec reference validation"
                            .to_string(),
                    });
                }
            }
        }

        let ok = errors.is_empty();
        Ok(CheckReport {
            ok,
            errors,
            warnings,
        })
    }

    pub fn doctor(&self, fix: bool) -> Result<DoctorReport, FormaError> {
        let mut findings = Vec::new();
        let mut fixes_applied = Vec::new();

        // 1. JSONL/SQLite count drift
        let db_spec_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM specs", [], |row| row.get(0))
            .map_err(|e| FormaError::Internal(format!("doctor query failed: {e}")))?;
        let db_section_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sections", [], |row| row.get(0))
            .map_err(|e| FormaError::Internal(format!("doctor query failed: {e}")))?;
        let db_ref_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM refs", [], |row| row.get(0))
            .map_err(|e| FormaError::Internal(format!("doctor query failed: {e}")))?;

        let jsonl_spec_count = Self::count_jsonl_lines(&self.forma_dir.join("specs.jsonl"));
        let jsonl_section_count = Self::count_jsonl_lines(&self.forma_dir.join("sections.jsonl"));
        let jsonl_ref_count = Self::count_jsonl_lines(&self.forma_dir.join("refs.jsonl"));

        if db_spec_count != jsonl_spec_count {
            findings.push(DoctorFinding {
                check: "sync_drift".to_string(),
                message: format!(
                    "specs count mismatch: SQLite has {db_spec_count}, JSONL has {jsonl_spec_count}"
                ),
            });
        }
        if db_section_count != jsonl_section_count {
            findings.push(DoctorFinding {
                check: "sync_drift".to_string(),
                message: format!(
                    "sections count mismatch: SQLite has {db_section_count}, JSONL has {jsonl_section_count}"
                ),
            });
        }
        if db_ref_count != jsonl_ref_count {
            findings.push(DoctorFinding {
                check: "sync_drift".to_string(),
                message: format!(
                    "refs count mismatch: SQLite has {db_ref_count}, JSONL has {jsonl_ref_count}"
                ),
            });
        }

        // 2. Orphaned refs
        let orphaned_refs: Vec<(String, String)> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT r.from_stem, r.to_stem FROM refs r
                     WHERE r.from_stem NOT IN (SELECT stem FROM specs)
                        OR r.to_stem NOT IN (SELECT stem FROM specs)",
                )
                .map_err(|e| FormaError::Internal(format!("doctor query failed: {e}")))?;
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|e| FormaError::Internal(format!("doctor query failed: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| FormaError::Internal(format!("doctor query failed: {e}")))?
        };

        for (from, to) in &orphaned_refs {
            findings.push(DoctorFinding {
                check: "orphaned_ref".to_string(),
                message: format!("orphaned ref: {from} -> {to}"),
            });
        }

        if fix && !orphaned_refs.is_empty() {
            self.conn
                .execute(
                    "DELETE FROM refs WHERE from_stem NOT IN (SELECT stem FROM specs)
                        OR to_stem NOT IN (SELECT stem FROM specs)",
                    [],
                )
                .map_err(|e| FormaError::Internal(format!("doctor fix failed: {e}")))?;
            fixes_applied.push(format!("removed {} orphaned ref(s)", orphaned_refs.len()));
        }

        // 3. Orphaned sections
        let orphaned_sections: Vec<(String, String)> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT s.id, s.spec_stem FROM sections s
                     WHERE s.spec_stem NOT IN (SELECT stem FROM specs)",
                )
                .map_err(|e| FormaError::Internal(format!("doctor query failed: {e}")))?;
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|e| FormaError::Internal(format!("doctor query failed: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| FormaError::Internal(format!("doctor query failed: {e}")))?
        };

        for (id, spec_stem) in &orphaned_sections {
            findings.push(DoctorFinding {
                check: "orphaned_section".to_string(),
                message: format!(
                    "orphaned section '{id}' references non-existent spec '{spec_stem}'"
                ),
            });
        }

        if fix && !orphaned_sections.is_empty() {
            self.conn
                .execute(
                    "DELETE FROM sections WHERE spec_stem NOT IN (SELECT stem FROM specs)",
                    [],
                )
                .map_err(|e| FormaError::Internal(format!("doctor fix failed: {e}")))?;
            fixes_applied.push(format!(
                "removed {} orphaned section(s)",
                orphaned_sections.len()
            ));
        }

        Ok(DoctorReport {
            findings,
            fixes_applied,
        })
    }

    fn count_jsonl_lines(path: &Path) -> i64 {
        match fs::read_to_string(path) {
            Ok(content) => content
                .lines()
                .filter(|line| !line.trim().is_empty())
                .count() as i64,
            Err(_) => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckFinding {
    pub check: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckReport {
    pub ok: bool,
    pub errors: Vec<CheckFinding>,
    pub warnings: Vec<CheckFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResult {
    pub specs: usize,
    pub sections: usize,
    pub refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub specs: usize,
    pub sections: usize,
    pub refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorFinding {
    pub check: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    pub findings: Vec<DoctorFinding>,
    pub fixes_applied: Vec<String>,
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
                "INSERT INTO specs (stem, src, purpose, status, created_at, updated_at)
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
        assert_eq!(spec.src.as_deref(), Some("crates/auth/"));
        assert_eq!(spec.purpose, "Authentication");
        assert_eq!(spec.status, Status::Draft);
    }

    #[test]
    fn insert_and_read_section() {
        let (db, _p, _d) = test_db();
        let now = "2026-03-14T14:30:00Z";
        db.conn
            .execute(
                "INSERT INTO specs (stem, src, purpose, status, created_at, updated_at)
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
                "INSERT INTO specs (stem, src, purpose, status, created_at, updated_at)
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
                "INSERT INTO specs (stem, src, purpose, status, created_at, updated_at)
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
                "INSERT INTO specs (stem, src, purpose, status, created_at, updated_at)
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
            "INSERT INTO specs (stem, src, purpose, status, created_at, updated_at)
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
                "INSERT INTO specs (stem, src, purpose, status, created_at, updated_at)
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
            r#"{"stem":"auth","src":"crates/auth/","purpose":"Authentication","status":"draft","created_at":"2026-03-14T14:30:00Z","updated_at":"2026-03-14T14:30:00Z"}"#,
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

        let specs = r#"{"stem":"auth","src":"crates/auth/","purpose":"Auth","status":"draft","created_at":"2026-03-14T14:30:00Z","updated_at":"2026-03-14T14:30:00Z"}
{"stem":"ralph","src":"crates/ralph/","purpose":"Runner","status":"stable","created_at":"2026-03-14T14:30:00Z","updated_at":"2026-03-14T14:30:00Z"}"#;
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
                "INSERT INTO specs (stem, src, purpose, status, created_at, updated_at)
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
            .create_spec("auth", Some("crates/auth/"), "Authentication", Some("tester"))
            .unwrap();
        assert_eq!(spec.stem, "auth");
        assert_eq!(spec.src.as_deref(), Some("crates/auth/"));
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
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
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
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        let err = db
            .create_spec("auth", Some("crates/auth/"), "Auth again", None)
            .unwrap_err();
        assert_eq!(err.code(), "already_exists");
    }

    #[test]
    fn create_spec_logs_event() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", Some("alice"))
            .unwrap();
        let events = db.spec_history("auth").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "created");
        assert_eq!(events[0].actor, Some("alice".to_string()));
    }

    #[test]
    fn get_spec_returns_detail() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Authentication", None)
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
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Runner", None)
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
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Runner", None)
            .unwrap();

        let specs = db.list_specs(None).unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].stem, "auth");
        assert_eq!(specs[1].stem, "ralph");
    }

    #[test]
    fn list_specs_filters_by_status() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Runner", None)
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
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
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
        assert_eq!(updated.src.as_deref(), Some("crates/auth-v2/"));
        assert_eq!(updated.purpose, "Authentication v2");
    }

    #[test]
    fn update_spec_partial() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        let updated = db
            .update_spec("auth", Some("proven"), None, None, None)
            .unwrap();
        assert_eq!(updated.status, Status::Proven);
        assert_eq!(updated.src.as_deref(), Some("crates/auth/"));
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
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        let err = db.update_spec("auth", None, None, None, None).unwrap_err();
        assert_eq!(err.code(), "validation_failed");
    }

    #[test]
    fn update_spec_logs_event() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
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
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.delete_spec("auth", false, None).unwrap();

        let specs = db.list_specs(None).unwrap();
        assert!(specs.is_empty());
    }

    #[test]
    fn delete_spec_with_content_requires_force() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
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
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Runner", None)
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
        db.create_spec("auth", Some("crates/auth/"), "Login system", None)
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Runner", None)
            .unwrap();

        let results = db.search_specs("auth").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].stem, "auth");
    }

    #[test]
    fn search_specs_matches_purpose() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Login system", None)
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Runner", None)
            .unwrap();

        let results = db.search_specs("Login").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].stem, "auth");
    }

    #[test]
    fn search_specs_matches_section_body() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
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
        db.create_spec("auth", Some("crates/auth/"), "Authentication", None)
            .unwrap();

        let results = db.search_specs("AUTHENTICATION").unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_specs_no_duplicates() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "auth system", None)
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
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Runner", None)
            .unwrap();

        let result = db.count_specs(false).unwrap();
        assert_eq!(result.total, 2);
        assert!(result.groups.is_none());
    }

    #[test]
    fn count_specs_by_status() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Runner", None)
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
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Runner", None)
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
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
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
    fn add_section_appends_at_end() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        let section = db
            .add_section("auth", "Custom Section", "body text", None, Some("alice"))
            .unwrap();
        assert_eq!(section.slug, "custom-section");
        assert_eq!(section.kind, SectionKind::Custom);
        assert_eq!(section.body, "body text");
        assert_eq!(section.position, 5); // after 5 required sections (0-4)
    }

    #[test]
    fn add_section_after_specific_slug() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        let section = db
            .add_section("auth", "Security Notes", "notes", Some("overview"), None)
            .unwrap();
        assert_eq!(section.position, 1); // after overview (pos 0)

        let sections = db.list_sections("auth").unwrap();
        assert_eq!(sections[0].slug, "overview");
        assert_eq!(sections[1].slug, "security-notes");
        assert_eq!(sections[2].slug, "architecture");
        assert_eq!(sections[2].position, 2); // shifted from 1
    }

    #[test]
    fn add_section_duplicate_slug_fails() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.add_section("auth", "Extra", "body", None, None).unwrap();

        let err = db
            .add_section("auth", "Extra", "other body", None, None)
            .unwrap_err();
        assert_eq!(err.code(), "already_exists");
    }

    #[test]
    fn add_section_spec_not_found() {
        let (db, _p, _d) = test_db();
        let err = db
            .add_section("nope", "Section", "body", None, None)
            .unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn add_section_after_nonexistent_slug_fails() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        let err = db
            .add_section("auth", "New", "body", Some("nonexistent"), None)
            .unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn add_section_logs_event() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.add_section("auth", "Extra", "body", None, Some("bob"))
            .unwrap();

        let events = db.spec_history("auth").unwrap();
        let added = events
            .iter()
            .find(|e| e.event_type == "section_added")
            .unwrap();
        assert_eq!(added.actor, Some("bob".to_string()));
        assert!(added.detail.as_ref().unwrap().contains("extra"));
    }

    #[test]
    fn set_section_replaces_body() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        let section = db
            .set_section("auth", "overview", "new body", Some("alice"))
            .unwrap();
        assert_eq!(section.body, "new body");
        assert_eq!(section.slug, "overview");
    }

    #[test]
    fn set_section_not_found() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        let err = db
            .set_section("auth", "nonexistent", "body", None)
            .unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn set_section_spec_not_found() {
        let (db, _p, _d) = test_db();
        let err = db
            .set_section("nope", "overview", "body", None)
            .unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn set_section_logs_event() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.set_section("auth", "overview", "content", Some("charlie"))
            .unwrap();

        let events = db.spec_history("auth").unwrap();
        let updated = events
            .iter()
            .find(|e| e.event_type == "section_updated")
            .unwrap();
        assert_eq!(updated.actor, Some("charlie".to_string()));
    }

    #[test]
    fn get_section_returns_section() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.set_section("auth", "overview", "hello", None).unwrap();

        let section = db.get_section("auth", "overview").unwrap();
        assert_eq!(section.slug, "overview");
        assert_eq!(section.body, "hello");
        assert_eq!(section.kind, SectionKind::Required);
    }

    #[test]
    fn get_section_not_found() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        let err = db.get_section("auth", "nonexistent").unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn get_section_spec_not_found() {
        let (db, _p, _d) = test_db();
        let err = db.get_section("nope", "overview").unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn list_sections_ordered_by_position() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.add_section("auth", "Extra One", "", None, None).unwrap();
        db.add_section("auth", "Extra Two", "", None, None).unwrap();

        let sections = db.list_sections("auth").unwrap();
        assert_eq!(sections.len(), 7); // 5 required + 2 custom
        for (i, section) in sections.iter().enumerate() {
            assert_eq!(section.position, i as i64);
        }
    }

    #[test]
    fn list_sections_spec_not_found() {
        let (db, _p, _d) = test_db();
        let err = db.list_sections("nope").unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn remove_section_custom() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.add_section("auth", "Extra", "body", None, None).unwrap();

        db.remove_section("auth", "extra", Some("alice")).unwrap();

        let sections = db.list_sections("auth").unwrap();
        assert_eq!(sections.len(), 5); // only required sections remain
    }

    #[test]
    fn remove_section_required_fails() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        let err = db.remove_section("auth", "overview", None).unwrap_err();
        assert_eq!(err.code(), "required_section");
    }

    #[test]
    fn remove_section_not_found() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        let err = db.remove_section("auth", "nonexistent", None).unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn remove_section_renumbers_positions() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.add_section("auth", "Extra One", "", None, None).unwrap();
        db.add_section("auth", "Extra Two", "", None, None).unwrap();

        db.remove_section("auth", "extra-one", None).unwrap();

        let sections = db.list_sections("auth").unwrap();
        assert_eq!(sections.len(), 6); // 5 required + 1 custom
        for (i, section) in sections.iter().enumerate() {
            assert_eq!(section.position, i as i64);
        }
        assert_eq!(sections[5].slug, "extra-two");
    }

    #[test]
    fn remove_section_logs_event() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.add_section("auth", "Extra", "", None, None).unwrap();
        db.remove_section("auth", "extra", Some("dave")).unwrap();

        let events = db.spec_history("auth").unwrap();
        let removed = events
            .iter()
            .find(|e| e.event_type == "section_removed")
            .unwrap();
        assert_eq!(removed.actor, Some("dave".to_string()));
    }

    #[test]
    fn move_section_down() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        // Move overview (pos 0) to after testing (pos 4)
        let section = db
            .move_section("auth", "overview", "testing", None)
            .unwrap();
        assert_eq!(section.position, 4);

        let sections = db.list_sections("auth").unwrap();
        assert_eq!(sections[0].slug, "architecture");
        assert_eq!(sections[0].position, 0);
        assert_eq!(sections[1].slug, "dependencies");
        assert_eq!(sections[2].slug, "error-handling");
        assert_eq!(sections[3].slug, "testing");
        assert_eq!(sections[4].slug, "overview");
    }

    #[test]
    fn move_section_up() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        // Move testing (pos 4) to after overview (pos 0)
        let section = db
            .move_section("auth", "testing", "overview", None)
            .unwrap();
        assert_eq!(section.position, 1);

        let sections = db.list_sections("auth").unwrap();
        assert_eq!(sections[0].slug, "overview");
        assert_eq!(sections[1].slug, "testing");
        assert_eq!(sections[2].slug, "architecture");
        assert_eq!(sections[3].slug, "dependencies");
        assert_eq!(sections[4].slug, "error-handling");
    }

    #[test]
    fn move_section_not_found() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        let err = db
            .move_section("auth", "nonexistent", "overview", None)
            .unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn move_section_after_not_found() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        let err = db
            .move_section("auth", "overview", "nonexistent", None)
            .unwrap_err();
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn move_section_logs_event() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();
        db.move_section("auth", "overview", "testing", Some("eve"))
            .unwrap();

        let events = db.spec_history("auth").unwrap();
        let moved = events
            .iter()
            .find(|e| e.event_type == "section_moved")
            .unwrap();
        assert_eq!(moved.actor, Some("eve".to_string()));
        assert!(moved.detail.as_ref().unwrap().contains("overview"));
        assert!(moved.detail.as_ref().unwrap().contains("testing"));
    }

    #[test]
    fn section_lifecycle() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Auth", None)
            .unwrap();

        // Add custom sections
        db.add_section("auth", "Security Notes", "initial notes", None, None)
            .unwrap();
        db.add_section(
            "auth",
            "API Design",
            "api design content",
            Some("overview"),
            None,
        )
        .unwrap();

        // Verify positions
        let sections = db.list_sections("auth").unwrap();
        assert_eq!(sections.len(), 7);
        assert_eq!(sections[1].slug, "api-design");
        assert_eq!(sections[6].slug, "security-notes");

        // Set body
        let updated = db
            .set_section("auth", "security-notes", "updated notes", None)
            .unwrap();
        assert_eq!(updated.body, "updated notes");

        // Get section
        let got = db.get_section("auth", "api-design").unwrap();
        assert_eq!(got.body, "api design content");

        // Move section
        db.move_section("auth", "security-notes", "api-design", None)
            .unwrap();
        let sections = db.list_sections("auth").unwrap();
        assert_eq!(sections[2].slug, "security-notes");

        // Remove section
        db.remove_section("auth", "api-design", None).unwrap();
        let sections = db.list_sections("auth").unwrap();
        assert_eq!(sections.len(), 6);
        assert!(!sections.iter().any(|s| s.slug == "api-design"));

        // Verify contiguous positions after removal
        for (i, section) in sections.iter().enumerate() {
            assert_eq!(section.position, i as i64);
        }
    }

    #[test]
    fn full_crud_lifecycle() {
        let (db, _p, _d) = test_db();

        let spec = db
            .create_spec("auth", Some("crates/auth/"), "Authentication", Some("alice"))
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

    fn create_two_specs(db: &Db) {
        db.create_spec("alpha", Some("crates/alpha/"), "Alpha spec", Some("test"))
            .unwrap();
        db.create_spec("beta", Some("crates/beta/"), "Beta spec", Some("test"))
            .unwrap();
    }

    #[test]
    fn add_ref_and_list() {
        let (db, _p, _d) = test_db();
        create_two_specs(&db);

        db.add_ref("alpha", "beta", Some("alice")).unwrap();

        let refs = db.list_refs("alpha").unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].stem, "beta");

        let refs = db.list_refs("beta").unwrap();
        assert!(refs.is_empty());
    }

    #[test]
    fn add_ref_logs_event() {
        let (db, _p, _d) = test_db();
        create_two_specs(&db);

        db.add_ref("alpha", "beta", Some("alice")).unwrap();

        let events = db.spec_history("alpha").unwrap();
        let ref_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == "ref_added")
            .collect();
        assert_eq!(ref_events.len(), 1);
        assert_eq!(ref_events[0].detail.as_deref(), Some("added ref to beta"));
    }

    #[test]
    fn add_ref_nonexistent_source() {
        let (db, _p, _d) = test_db();
        db.create_spec("beta", Some("crates/beta/"), "Beta spec", Some("test"))
            .unwrap();

        let err = db.add_ref("nope", "beta", None).unwrap_err();
        assert!(matches!(err, FormaError::NotFound(_)));
    }

    #[test]
    fn add_ref_nonexistent_target() {
        let (db, _p, _d) = test_db();
        db.create_spec("alpha", Some("crates/alpha/"), "Alpha spec", Some("test"))
            .unwrap();

        let err = db.add_ref("alpha", "nope", None).unwrap_err();
        assert!(matches!(err, FormaError::NotFound(_)));
    }

    #[test]
    fn add_ref_duplicate() {
        let (db, _p, _d) = test_db();
        create_two_specs(&db);

        db.add_ref("alpha", "beta", None).unwrap();
        let err = db.add_ref("alpha", "beta", None).unwrap_err();
        assert!(matches!(err, FormaError::AlreadyExists(_)));
    }

    #[test]
    fn add_ref_self_ref_rejected() {
        let (db, _p, _d) = test_db();
        db.create_spec("alpha", Some("crates/alpha/"), "Alpha spec", Some("test"))
            .unwrap();

        let err = db.add_ref("alpha", "alpha", None);
        assert!(err.is_err());
    }

    #[test]
    fn add_ref_cycle_detected() {
        let (db, _p, _d) = test_db();
        db.create_spec("a", Some("crates/a/"), "A", Some("test")).unwrap();
        db.create_spec("b", Some("crates/b/"), "B", Some("test")).unwrap();
        db.create_spec("c", Some("crates/c/"), "C", Some("test")).unwrap();

        db.add_ref("a", "b", None).unwrap();
        db.add_ref("b", "c", None).unwrap();
        let err = db.add_ref("c", "a", None).unwrap_err();
        assert!(matches!(err, FormaError::CycleDetected));
    }

    #[test]
    fn add_ref_no_false_cycle() {
        let (db, _p, _d) = test_db();
        db.create_spec("a", Some("crates/a/"), "A", Some("test")).unwrap();
        db.create_spec("b", Some("crates/b/"), "B", Some("test")).unwrap();
        db.create_spec("c", Some("crates/c/"), "C", Some("test")).unwrap();

        db.add_ref("a", "b", None).unwrap();
        db.add_ref("a", "c", None).unwrap();
        db.add_ref("b", "c", None).unwrap();
    }

    #[test]
    fn remove_ref_ok() {
        let (db, _p, _d) = test_db();
        create_two_specs(&db);

        db.add_ref("alpha", "beta", None).unwrap();
        db.remove_ref("alpha", "beta", Some("bob")).unwrap();

        let refs = db.list_refs("alpha").unwrap();
        assert!(refs.is_empty());
    }

    #[test]
    fn remove_ref_logs_event() {
        let (db, _p, _d) = test_db();
        create_two_specs(&db);

        db.add_ref("alpha", "beta", None).unwrap();
        db.remove_ref("alpha", "beta", Some("bob")).unwrap();

        let events = db.spec_history("alpha").unwrap();
        let ref_events: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == "ref_removed")
            .collect();
        assert_eq!(ref_events.len(), 1);
    }

    #[test]
    fn remove_ref_not_found() {
        let (db, _p, _d) = test_db();
        create_two_specs(&db);

        let err = db.remove_ref("alpha", "beta", None).unwrap_err();
        assert!(matches!(err, FormaError::NotFound(_)));
    }

    #[test]
    fn list_refs_spec_not_found() {
        let (db, _p, _d) = test_db();
        let err = db.list_refs("nope").unwrap_err();
        assert!(matches!(err, FormaError::NotFound(_)));
    }

    #[test]
    fn ref_tree_down() {
        let (db, _p, _d) = test_db();
        db.create_spec("a", Some("crates/a/"), "A spec", Some("test"))
            .unwrap();
        db.create_spec("b", Some("crates/b/"), "B spec", Some("test"))
            .unwrap();
        db.create_spec("c", Some("crates/c/"), "C spec", Some("test"))
            .unwrap();

        db.add_ref("a", "b", None).unwrap();
        db.add_ref("b", "c", None).unwrap();

        let tree = db.ref_tree("a", "down").unwrap();
        assert_eq!(tree.len(), 3);
        assert_eq!(tree[0].stem, "a");
        assert_eq!(tree[0].depth, 0);
        assert_eq!(tree[1].stem, "b");
        assert_eq!(tree[1].depth, 1);
        assert_eq!(tree[2].stem, "c");
        assert_eq!(tree[2].depth, 2);
    }

    #[test]
    fn ref_tree_up() {
        let (db, _p, _d) = test_db();
        db.create_spec("a", Some("crates/a/"), "A spec", Some("test"))
            .unwrap();
        db.create_spec("b", Some("crates/b/"), "B spec", Some("test"))
            .unwrap();
        db.create_spec("c", Some("crates/c/"), "C spec", Some("test"))
            .unwrap();

        db.add_ref("a", "b", None).unwrap();
        db.add_ref("b", "c", None).unwrap();

        let tree = db.ref_tree("c", "up").unwrap();
        assert_eq!(tree.len(), 3);
        assert_eq!(tree[0].stem, "c");
        assert_eq!(tree[0].depth, 0);
        assert_eq!(tree[1].stem, "b");
        assert_eq!(tree[1].depth, 1);
        assert_eq!(tree[2].stem, "a");
        assert_eq!(tree[2].depth, 2);
    }

    #[test]
    fn ref_tree_spec_not_found() {
        let (db, _p, _d) = test_db();
        let err = db.ref_tree("nope", "down").unwrap_err();
        assert!(matches!(err, FormaError::NotFound(_)));
    }

    #[test]
    fn detect_cycles_empty() {
        let (db, _p, _d) = test_db();
        let cycles = db.detect_cycles().unwrap();
        assert!(cycles.is_empty());
    }

    #[test]
    fn detect_cycles_no_cycles() {
        let (db, _p, _d) = test_db();
        db.create_spec("a", Some("crates/a/"), "A", Some("test")).unwrap();
        db.create_spec("b", Some("crates/b/"), "B", Some("test")).unwrap();
        db.add_ref("a", "b", None).unwrap();

        let cycles = db.detect_cycles().unwrap();
        assert!(cycles.is_empty());
    }

    #[test]
    fn detect_cycles_finds_cycle() {
        let (db, _p, _d) = test_db();
        db.create_spec("a", Some("crates/a/"), "A", Some("test")).unwrap();
        db.create_spec("b", Some("crates/b/"), "B", Some("test")).unwrap();
        db.create_spec("c", Some("crates/c/"), "C", Some("test")).unwrap();

        db.add_ref("a", "b", None).unwrap();
        db.add_ref("b", "c", None).unwrap();
        // Bypass add_ref cycle detection by inserting directly
        db.conn
            .execute(
                "INSERT INTO refs (from_stem, to_stem) VALUES ('c', 'a')",
                [],
            )
            .unwrap();

        let cycles = db.detect_cycles().unwrap();
        assert!(!cycles.is_empty());
    }

    #[test]
    fn ref_tree_single_node() {
        let (db, _p, _d) = test_db();
        db.create_spec("solo", Some("crates/solo/"), "Solo spec", Some("test"))
            .unwrap();

        let tree = db.ref_tree("solo", "down").unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].stem, "solo");
        assert_eq!(tree[0].depth, 0);
    }

    #[test]
    fn get_spec_detail_includes_refs() {
        let (db, _p, _d) = test_db();
        create_two_specs(&db);
        db.add_ref("alpha", "beta", None).unwrap();

        let detail = db.get_spec("alpha").unwrap();
        assert_eq!(detail.refs.len(), 1);
        assert_eq!(detail.refs[0].stem, "beta");
    }

    #[test]
    fn delete_spec_cascades_refs_cleanup() {
        let (db, _p, _d) = test_db();
        create_two_specs(&db);
        db.add_ref("alpha", "beta", None).unwrap();

        db.delete_spec("beta", false, Some("test")).unwrap();

        let refs = db.list_refs("alpha").unwrap();
        assert!(refs.is_empty());
    }

    #[test]
    fn export_writes_jsonl_files() {
        let (db, project_dir, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Authentication", Some("test"))
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Iterative runner", Some("test"))
            .unwrap();

        let result = db.export_jsonl().unwrap();
        assert_eq!(result.specs, 2);
        assert_eq!(result.sections, 10); // 5 required per spec
        assert_eq!(result.refs, 0);

        let forma_dir = project_dir.path().join(".forma");
        assert!(forma_dir.join("specs.jsonl").exists());
        assert!(forma_dir.join("sections.jsonl").exists());
        assert!(forma_dir.join("refs.jsonl").exists());

        let specs_content = fs::read_to_string(forma_dir.join("specs.jsonl")).unwrap();
        let lines: Vec<&str> = specs_content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"auth\""));
        assert!(lines[1].contains("\"ralph\""));
    }

    #[test]
    fn export_writes_spec_markdown() {
        let (db, project_dir, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Authentication", Some("test"))
            .unwrap();
        db.set_section("auth", "overview", "Auth overview body.", Some("test"))
            .unwrap();

        db.export_jsonl().unwrap();

        let forma_dir = project_dir.path().join(".forma");
        let md = fs::read_to_string(forma_dir.join("specs/auth.md")).unwrap();
        assert!(md.starts_with("# auth Specification"));
        assert!(md.contains("Authentication"));
        assert!(md.contains("| Crate | `crates/auth/` |"));
        assert!(md.contains("| Status | draft |"));
        assert!(md.contains("## Overview"));
        assert!(md.contains("Auth overview body."));
        assert!(!md.contains("## Related Specifications"));
    }

    #[test]
    fn export_markdown_includes_refs() {
        let (db, project_dir, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Authentication", Some("test"))
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Iterative runner", Some("test"))
            .unwrap();
        db.add_ref("auth", "ralph", Some("test")).unwrap();

        db.export_jsonl().unwrap();

        let forma_dir = project_dir.path().join(".forma");
        let md = fs::read_to_string(forma_dir.join("specs/auth.md")).unwrap();
        assert!(md.contains("## Related Specifications"));
        assert!(md.contains("[ralph](ralph.md) — Iterative runner"));
    }

    #[test]
    fn export_writes_readme() {
        let (db, project_dir, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Authentication", Some("test"))
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Iterative runner", Some("test"))
            .unwrap();

        db.export_jsonl().unwrap();

        let forma_dir = project_dir.path().join(".forma");
        let readme = fs::read_to_string(forma_dir.join("README.md")).unwrap();
        assert!(readme.starts_with("# Specifications"));
        assert!(readme.contains("| Spec | Code | Status | Purpose |"));
        assert!(readme.contains("[auth](specs/auth.md)"));
        assert!(readme.contains("[ralph](specs/ralph.md)"));
        assert!(readme.contains("`crates/auth/`"));
    }

    #[test]
    fn export_import_roundtrip() {
        let (db, _p, _d) = test_db();
        db.create_spec("auth", Some("crates/auth/"), "Authentication", Some("test"))
            .unwrap();
        db.create_spec("ralph", Some("crates/ralph/"), "Iterative runner", Some("test"))
            .unwrap();
        db.set_section("auth", "overview", "Auth overview.", Some("test"))
            .unwrap();
        db.add_ref("auth", "ralph", Some("test")).unwrap();

        let export_result = db.export_jsonl().unwrap();
        assert_eq!(export_result.specs, 2);
        assert_eq!(export_result.refs, 1);

        let import_result = db.import_jsonl().unwrap();
        assert_eq!(import_result.specs, 2);
        assert_eq!(import_result.sections, 10);
        assert_eq!(import_result.refs, 1);

        let detail = db.get_spec("auth").unwrap();
        assert_eq!(detail.spec.purpose, "Authentication");
        assert_eq!(detail.refs.len(), 1);
        assert_eq!(detail.refs[0].stem, "ralph");

        let overview = db.get_section("auth", "overview").unwrap();
        assert_eq!(overview.body, "Auth overview.");
    }

    #[test]
    fn export_empty_db() {
        let (db, project_dir, _d) = test_db();
        let result = db.export_jsonl().unwrap();
        assert_eq!(result.specs, 0);
        assert_eq!(result.sections, 0);
        assert_eq!(result.refs, 0);

        let forma_dir = project_dir.path().join(".forma");
        assert!(forma_dir.join("specs.jsonl").exists());
        assert!(forma_dir.join("README.md").exists());
    }

    #[test]
    fn check_clean_spec_with_content() {
        let (db, project_dir, _d) = test_db();
        let crate_dir = project_dir.path().join("crates/mylib");
        std::fs::create_dir_all(&crate_dir).unwrap();
        db.create_spec("mylib", Some("crates/mylib"), "Test spec", None)
            .unwrap();
        for slug in [
            "overview",
            "architecture",
            "dependencies",
            "error-handling",
            "testing",
        ] {
            db.set_section("mylib", slug, "Some content here", None)
                .unwrap();
        }

        let report = db.check(None).unwrap();
        assert!(report.ok);
        assert!(report.errors.is_empty());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn check_warns_empty_required_sections() {
        let (db, project_dir, _d) = test_db();
        let crate_dir = project_dir.path().join("crates/mylib");
        std::fs::create_dir_all(&crate_dir).unwrap();
        db.create_spec("mylib", Some("crates/mylib"), "Test spec", None)
            .unwrap();

        let report = db.check(None).unwrap();
        assert!(report.ok);
        assert!(report.errors.is_empty());
        assert_eq!(report.warnings.len(), 5);
        assert!(
            report
                .warnings
                .iter()
                .all(|w| w.check == "required_sections_nonempty")
        );
    }

    #[test]
    fn check_errors_missing_src_path() {
        let (db, _project_dir, _d) = test_db();
        db.create_spec("ghost", Some("crates/nonexistent"), "Does not exist", None)
            .unwrap();

        let report = db.check(None).unwrap();
        assert!(!report.ok);
        assert!(report.errors.iter().any(|e| e.check == "crate_paths_exist"));
    }

    #[test]
    fn check_detects_ref_cycles() {
        let (db, project_dir, _d) = test_db();
        for name in ["a", "b", "c"] {
            let crate_dir = project_dir.path().join(format!("crates/{name}"));
            std::fs::create_dir_all(&crate_dir).unwrap();
            db.create_spec(
                name,
                &format!("crates/{name}"),
                &format!("Spec {name}"),
                None,
            )
            .unwrap();
        }

        db.conn
            .execute(
                "INSERT INTO refs (from_stem, to_stem) VALUES ('a', 'b')",
                [],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO refs (from_stem, to_stem) VALUES ('b', 'c')",
                [],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO refs (from_stem, to_stem) VALUES ('c', 'a')",
                [],
            )
            .unwrap();

        let report = db.check(None).unwrap();
        assert!(!report.ok);
        assert!(report.errors.iter().any(|e| e.check == "no_ref_cycles"));
    }

    #[test]
    fn check_pensa_unreachable_warns() {
        let (db, project_dir, _d) = test_db();
        let crate_dir = project_dir.path().join("crates/mylib");
        std::fs::create_dir_all(&crate_dir).unwrap();
        db.create_spec("mylib", Some("crates/mylib"), "Test spec", None)
            .unwrap();

        let report = db.check(Some("http://localhost:1")).unwrap();
        assert!(report.ok);
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.check == "pensa_spec_refs_valid" && w.message.contains("unreachable"))
        );
    }

    #[test]
    fn doctor_healthy_db_no_findings() {
        let (db, project_dir, _d) = test_db();
        let crate_dir = project_dir.path().join("crates/mylib");
        std::fs::create_dir_all(&crate_dir).unwrap();
        db.create_spec("mylib", Some("crates/mylib"), "Test", None)
            .unwrap();
        db.export_jsonl().unwrap();

        let report = db.doctor(false).unwrap();
        assert!(report.findings.is_empty());
        assert!(report.fixes_applied.is_empty());
    }

    #[test]
    fn doctor_detects_sync_drift() {
        let (db, project_dir, _d) = test_db();
        let crate_dir = project_dir.path().join("crates/mylib");
        std::fs::create_dir_all(&crate_dir).unwrap();
        db.create_spec("mylib", Some("crates/mylib"), "Test", None)
            .unwrap();
        // Don't export — JSONL files don't exist, so count is 0 vs 1 in SQLite

        let report = db.doctor(false).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.check == "sync_drift" && f.message.contains("specs"))
        );
    }

    fn create_orphans(db: &Db) {
        // Temporarily disable FK checks to create orphaned data
        db.conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
        db.conn
            .execute(
                "INSERT INTO refs (from_stem, to_stem) VALUES ('alpha', 'ghost')",
                [],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO sections (id, spec_stem, name, slug, kind, body, position, created_at, updated_at)
                 VALUES ('fm-orphan01', 'ghost', 'Orphan', 'orphan', 'custom', '', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        db.conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
    }

    #[test]
    fn doctor_detects_orphaned_refs() {
        let (db, _p, _d) = test_db();
        db.create_spec("alpha", Some("crates/a"), "Alpha", None).unwrap();
        create_orphans(&db);

        let report = db.doctor(false).unwrap();
        assert!(report.findings.iter().any(|f| f.check == "orphaned_ref"));
        assert!(report.fixes_applied.is_empty());
    }

    #[test]
    fn doctor_fixes_orphaned_refs() {
        let (db, _p, _d) = test_db();
        db.create_spec("alpha", Some("crates/a"), "Alpha", None).unwrap();
        create_orphans(&db);

        let report = db.doctor(true).unwrap();
        assert!(report.findings.iter().any(|f| f.check == "orphaned_ref"));
        assert!(
            report
                .fixes_applied
                .iter()
                .any(|f| f.contains("orphaned ref"))
        );

        let ref_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM refs WHERE to_stem = 'ghost'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ref_count, 0);
    }

    #[test]
    fn doctor_detects_orphaned_sections() {
        let (db, _p, _d) = test_db();
        db.create_spec("alpha", Some("crates/a"), "Alpha", None).unwrap();
        create_orphans(&db);

        let report = db.doctor(false).unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.check == "orphaned_section")
        );
    }

    #[test]
    fn doctor_fixes_orphaned_sections() {
        let (db, _p, _d) = test_db();
        db.create_spec("alpha", Some("crates/a"), "Alpha", None).unwrap();
        create_orphans(&db);

        let report = db.doctor(true).unwrap();
        assert!(
            report
                .fixes_applied
                .iter()
                .any(|f| f.contains("orphaned section"))
        );

        let orphan_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sections WHERE spec_stem = 'ghost'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(orphan_count, 0);

        let alpha_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sections WHERE spec_stem = 'alpha'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(alpha_count > 0);
    }

    #[test]
    fn doctor_no_fix_does_not_remove() {
        let (db, _p, _d) = test_db();
        db.create_spec("alpha", Some("crates/a"), "Alpha", None).unwrap();
        create_orphans(&db);

        let report = db.doctor(false).unwrap();
        assert!(!report.findings.is_empty());
        assert!(report.fixes_applied.is_empty());

        let ref_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM refs WHERE to_stem = 'ghost'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ref_count, 1);
    }
}
