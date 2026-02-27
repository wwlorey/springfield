use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use rusqlite::types::Value;

use crate::error::PensaError;
use crate::id::generate_id;
use crate::types::{Comment, CreateIssueParams, Issue, IssueDetail, Status, UpdateFields};

pub struct Db {
    pub conn: Connection,
    pub pensa_dir: PathBuf,
}

fn parse_dt(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

pub(crate) fn issue_from_row(row: &rusqlite::Row) -> Result<Issue, rusqlite::Error> {
    let issue_type_str: String = row.get("issue_type")?;
    let status_str: String = row.get("status")?;
    let priority_str: String = row.get("priority")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;
    let closed_at_str: Option<String> = row.get("closed_at")?;

    Ok(Issue {
        id: row.get("id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        issue_type: issue_type_str.parse().unwrap(),
        status: status_str.parse().unwrap(),
        priority: priority_str.parse().unwrap(),
        spec: row.get("spec")?,
        fixes: row.get("fixes")?,
        assignee: row.get("assignee")?,
        created_at: parse_dt(&created_at_str),
        updated_at: parse_dt(&updated_at_str),
        closed_at: closed_at_str.map(|s| parse_dt(&s)),
        close_reason: row.get("close_reason")?,
    })
}

pub(crate) fn comment_from_row(row: &rusqlite::Row) -> Result<Comment, rusqlite::Error> {
    let created_at_str: String = row.get("created_at")?;
    Ok(Comment {
        id: row.get("id")?,
        issue_id: row.get("issue_id")?,
        actor: row.get("actor")?,
        text: row.get("text")?,
        created_at: parse_dt(&created_at_str),
    })
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

    pub fn create_issue(&self, params: &CreateIssueParams) -> Result<Issue, PensaError> {
        let id = generate_id();
        let ts = now();

        self.conn
            .execute(
                "INSERT INTO issues (id, title, description, issue_type, status, priority, spec, fixes, assignee, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    id,
                    params.title,
                    params.description,
                    params.issue_type.as_str(),
                    "open",
                    params.priority.as_str(),
                    params.spec,
                    params.fixes,
                    params.assignee,
                    ts,
                    ts,
                ],
            )
            .map_err(|e| PensaError::Internal(format!("failed to create issue: {e}")))?;

        self.conn
            .execute(
                "INSERT INTO events (issue_id, event_type, actor, created_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, "created", params.actor, ts],
            )
            .map_err(|e| PensaError::Internal(format!("failed to log create event: {e}")))?;

        for dep_id in &params.deps {
            self.conn
                .execute(
                    "INSERT INTO deps (issue_id, depends_on_id) VALUES (?1, ?2)",
                    rusqlite::params![id, dep_id],
                )
                .map_err(|e| PensaError::Internal(format!("failed to add dep: {e}")))?;
        }

        self.get_issue_only(&id)
    }

    pub(crate) fn get_issue_only(&self, id: &str) -> Result<Issue, PensaError> {
        self.conn
            .query_row(
                "SELECT * FROM issues WHERE id = ?1",
                rusqlite::params![id],
                issue_from_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => PensaError::NotFound(id.to_string()),
                other => PensaError::Internal(format!("failed to get issue: {other}")),
            })
    }

    pub fn get_issue(&self, id: &str) -> Result<IssueDetail, PensaError> {
        let issue = self.get_issue_only(id)?;

        let mut dep_stmt = self
            .conn
            .prepare(
                "SELECT i.* FROM issues i
                 JOIN deps d ON d.depends_on_id = i.id
                 WHERE d.issue_id = ?1",
            )
            .map_err(|e| PensaError::Internal(format!("failed to prepare deps query: {e}")))?;
        let deps = dep_stmt
            .query_map(rusqlite::params![id], issue_from_row)
            .map_err(|e| PensaError::Internal(format!("failed to query deps: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| PensaError::Internal(format!("failed to read deps: {e}")))?;

        let mut comment_stmt = self
            .conn
            .prepare("SELECT * FROM comments WHERE issue_id = ?1 ORDER BY created_at")
            .map_err(|e| PensaError::Internal(format!("failed to prepare comments query: {e}")))?;
        let comments = comment_stmt
            .query_map(rusqlite::params![id], comment_from_row)
            .map_err(|e| PensaError::Internal(format!("failed to query comments: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| PensaError::Internal(format!("failed to read comments: {e}")))?;

        Ok(IssueDetail {
            issue,
            deps,
            comments,
        })
    }

    pub fn claim_issue(&self, id: &str, actor: &str) -> Result<Issue, PensaError> {
        let rows = self
            .conn
            .execute(
                "UPDATE issues SET status = 'in_progress', assignee = ?1, updated_at = ?2 WHERE id = ?3 AND status = 'open'",
                rusqlite::params![actor, now(), id],
            )
            .map_err(|e| PensaError::Internal(format!("failed to claim issue: {e}")))?;

        if rows == 0 {
            let issue = self.get_issue_only(id)?;
            return Err(PensaError::AlreadyClaimed {
                id: id.to_string(),
                holder: issue.assignee.unwrap_or_default(),
            });
        }

        let ts = now();
        self.conn
            .execute(
                "INSERT INTO events (issue_id, event_type, actor, created_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, "claimed", actor, ts],
            )
            .map_err(|e| PensaError::Internal(format!("failed to log claim event: {e}")))?;

        self.get_issue_only(id)
    }

    pub fn release_issue(&self, id: &str, actor: &str) -> Result<Issue, PensaError> {
        self.get_issue_only(id)?;

        let ts = now();
        self.conn
            .execute(
                "UPDATE issues SET status = 'open', assignee = NULL, updated_at = ?1 WHERE id = ?2",
                rusqlite::params![ts, id],
            )
            .map_err(|e| PensaError::Internal(format!("failed to release issue: {e}")))?;

        self.conn
            .execute(
                "INSERT INTO events (issue_id, event_type, actor, created_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, "released", actor, ts],
            )
            .map_err(|e| PensaError::Internal(format!("failed to log release event: {e}")))?;

        self.get_issue_only(id)
    }

    pub fn close_issue(
        &self,
        id: &str,
        reason: Option<&str>,
        force: bool,
        actor: &str,
    ) -> Result<Issue, PensaError> {
        let issue = self.get_issue_only(id)?;

        if !force && issue.status == Status::Closed {
            return Err(PensaError::InvalidStatusTransition {
                from: "closed".to_string(),
                to: "closed".to_string(),
            });
        }

        let ts = now();
        self.conn
            .execute(
                "UPDATE issues SET status = 'closed', closed_at = ?1, close_reason = ?2, updated_at = ?1 WHERE id = ?3",
                rusqlite::params![ts, reason, id],
            )
            .map_err(|e| PensaError::Internal(format!("failed to close issue: {e}")))?;

        self.conn
            .execute(
                "INSERT INTO events (issue_id, event_type, actor, detail, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id, "closed", actor, reason, ts],
            )
            .map_err(|e| PensaError::Internal(format!("failed to log close event: {e}")))?;

        if let Some(fixes_id) = &issue.fixes {
            let fixes_reason = format!("fixed by {id}");
            self.conn
                .execute(
                    "UPDATE issues SET status = 'closed', closed_at = ?1, close_reason = ?2, updated_at = ?1 WHERE id = ?3",
                    rusqlite::params![ts, fixes_reason, fixes_id],
                )
                .map_err(|e| PensaError::Internal(format!("failed to auto-close linked bug: {e}")))?;

            self.conn
                .execute(
                    "INSERT INTO events (issue_id, event_type, actor, detail, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![fixes_id, "closed", actor, fixes_reason, ts],
                )
                .map_err(|e| PensaError::Internal(format!("failed to log auto-close event: {e}")))?;
        }

        self.get_issue_only(id)
    }

    pub fn reopen_issue(
        &self,
        id: &str,
        reason: Option<&str>,
        actor: &str,
    ) -> Result<Issue, PensaError> {
        self.get_issue_only(id)?;

        let ts = now();
        self.conn
            .execute(
                "UPDATE issues SET status = 'open', closed_at = NULL, close_reason = NULL, updated_at = ?1 WHERE id = ?2",
                rusqlite::params![ts, id],
            )
            .map_err(|e| PensaError::Internal(format!("failed to reopen issue: {e}")))?;

        self.conn
            .execute(
                "INSERT INTO events (issue_id, event_type, actor, detail, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id, "reopened", actor, reason, ts],
            )
            .map_err(|e| PensaError::Internal(format!("failed to log reopen event: {e}")))?;

        self.get_issue_only(id)
    }

    pub fn delete_issue(&self, id: &str, force: bool) -> Result<(), PensaError> {
        self.get_issue_only(id)?;

        if !force {
            let dependents: i64 = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM deps WHERE depends_on_id = ?1",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .map_err(|e| PensaError::Internal(format!("failed to check dependents: {e}")))?;

            let comments: i64 = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM comments WHERE issue_id = ?1",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .map_err(|e| PensaError::Internal(format!("failed to check comments: {e}")))?;

            if dependents > 0 || comments > 0 {
                return Err(PensaError::DeleteRequiresForce(format!(
                    "issue has {dependents} dependents and {comments} comments"
                )));
            }
        }

        self.conn
            .execute(
                "DELETE FROM deps WHERE issue_id = ?1 OR depends_on_id = ?1",
                rusqlite::params![id],
            )
            .map_err(|e| PensaError::Internal(format!("failed to delete deps: {e}")))?;
        self.conn
            .execute(
                "DELETE FROM comments WHERE issue_id = ?1",
                rusqlite::params![id],
            )
            .map_err(|e| PensaError::Internal(format!("failed to delete comments: {e}")))?;
        self.conn
            .execute(
                "DELETE FROM events WHERE issue_id = ?1",
                rusqlite::params![id],
            )
            .map_err(|e| PensaError::Internal(format!("failed to delete events: {e}")))?;
        self.conn
            .execute("DELETE FROM issues WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| PensaError::Internal(format!("failed to delete issue: {e}")))?;

        Ok(())
    }

    pub fn update_issue(
        &self,
        id: &str,
        fields: &UpdateFields,
        actor: &str,
    ) -> Result<Issue, PensaError> {
        self.get_issue_only(id)?;

        let mut set_clauses = Vec::new();
        let mut values: Vec<Value> = Vec::new();
        let mut changed = serde_json::Map::new();

        if let Some(title) = &fields.title {
            set_clauses.push("title = ?");
            values.push(Value::Text(title.clone()));
            changed.insert("title".into(), serde_json::Value::String(title.clone()));
        }
        if let Some(description) = &fields.description {
            set_clauses.push("description = ?");
            values.push(Value::Text(description.clone()));
            changed.insert(
                "description".into(),
                serde_json::Value::String(description.clone()),
            );
        }
        if let Some(priority) = &fields.priority {
            set_clauses.push("priority = ?");
            values.push(Value::Text(priority.as_str().to_string()));
            changed.insert(
                "priority".into(),
                serde_json::Value::String(priority.as_str().to_string()),
            );
        }
        if let Some(status) = &fields.status {
            set_clauses.push("status = ?");
            values.push(Value::Text(status.as_str().to_string()));
            changed.insert(
                "status".into(),
                serde_json::Value::String(status.as_str().to_string()),
            );
        }
        if let Some(assignee) = &fields.assignee {
            set_clauses.push("assignee = ?");
            if assignee.is_empty() {
                values.push(Value::Null);
            } else {
                values.push(Value::Text(assignee.clone()));
            }
            changed.insert(
                "assignee".into(),
                serde_json::Value::String(assignee.clone()),
            );
        }
        if let Some(spec) = &fields.spec {
            set_clauses.push("spec = ?");
            values.push(Value::Text(spec.clone()));
            changed.insert("spec".into(), serde_json::Value::String(spec.clone()));
        }
        if let Some(fixes) = &fields.fixes {
            set_clauses.push("fixes = ?");
            values.push(Value::Text(fixes.clone()));
            changed.insert("fixes".into(), serde_json::Value::String(fixes.clone()));
        }

        let ts = now();
        set_clauses.push("updated_at = ?");
        values.push(Value::Text(ts.clone()));

        values.push(Value::Text(id.to_string()));

        let sql = format!("UPDATE issues SET {} WHERE id = ?", set_clauses.join(", "));

        self.conn
            .execute(&sql, rusqlite::params_from_iter(values))
            .map_err(|e| PensaError::Internal(format!("failed to update issue: {e}")))?;

        let detail = serde_json::Value::Object(changed).to_string();
        self.conn
            .execute(
                "INSERT INTO events (issue_id, event_type, actor, detail, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id, "updated", actor, detail, ts],
            )
            .map_err(|e| PensaError::Internal(format!("failed to log update event: {e}")))?;

        self.get_issue_only(id)
    }
}

pub fn now() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CreateIssueParams, IssueType, Priority, Status};
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

    #[test]
    fn create_and_get() {
        let (db, _dir) = open_temp_db();

        let issue = db
            .create_issue(&CreateIssueParams {
                title: "login crash".into(),
                issue_type: IssueType::Bug,
                priority: Priority::P0,
                description: Some("crashes on empty password".into()),
                spec: None,
                fixes: None,
                assignee: Some("alice".into()),
                deps: vec![],
                actor: "test-agent".into(),
            })
            .unwrap();

        assert!(issue.id.starts_with("pn-"));
        assert_eq!(issue.title, "login crash");
        assert_eq!(issue.issue_type, IssueType::Bug);
        assert_eq!(issue.priority, Priority::P0);
        assert_eq!(issue.status, Status::Open);
        assert_eq!(
            issue.description.as_deref(),
            Some("crashes on empty password")
        );
        assert_eq!(issue.assignee.as_deref(), Some("alice"));
        assert!(issue.spec.is_none());
        assert!(issue.fixes.is_none());
        assert!(issue.closed_at.is_none());
        assert!(issue.close_reason.is_none());

        let detail = db.get_issue(&issue.id).unwrap();
        assert_eq!(detail.issue.id, issue.id);
        assert_eq!(detail.issue.title, "login crash");
        assert!(detail.deps.is_empty());
        assert!(detail.comments.is_empty());
    }

    #[test]
    fn get_nonexistent() {
        let (db, _dir) = open_temp_db();
        let result = db.get_issue("pn-00000000");
        assert!(matches!(result, Err(PensaError::NotFound(_))));
    }

    #[test]
    fn update_fields() {
        let (db, _dir) = open_temp_db();

        let issue = db
            .create_issue(&CreateIssueParams {
                title: "original title".into(),
                issue_type: IssueType::Task,
                priority: Priority::P2,
                description: Some("original desc".into()),
                spec: None,
                fixes: None,
                assignee: None,
                deps: vec![],
                actor: "test-agent".into(),
            })
            .unwrap();

        let updated = db
            .update_issue(
                &issue.id,
                &UpdateFields {
                    title: Some("new title".to_string()),
                    priority: Some(Priority::P1),
                    ..Default::default()
                },
                "test-agent",
            )
            .unwrap();

        assert_eq!(updated.title, "new title");
        assert_eq!(updated.priority, Priority::P1);
        assert_eq!(updated.description.as_deref(), Some("original desc"));
        assert_eq!(updated.issue_type, IssueType::Task);
        assert!(updated.updated_at >= issue.updated_at);
    }

    #[test]
    fn update_logs_event() {
        let (db, _dir) = open_temp_db();

        let issue = db
            .create_issue(&CreateIssueParams {
                title: "test issue".into(),
                issue_type: IssueType::Task,
                priority: Priority::P2,
                description: None,
                spec: None,
                fixes: None,
                assignee: None,
                deps: vec![],
                actor: "test-agent".into(),
            })
            .unwrap();

        db.update_issue(
            &issue.id,
            &UpdateFields {
                title: Some("updated title".to_string()),
                ..Default::default()
            },
            "test-agent",
        )
        .unwrap();

        let mut stmt = db
            .conn
            .prepare(
                "SELECT event_type, detail FROM events WHERE issue_id = ?1 ORDER BY created_at",
            )
            .unwrap();
        let events: Vec<(String, Option<String>)> = stmt
            .query_map(rusqlite::params![issue.id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, "created");
        assert_eq!(events[1].0, "updated");
        assert!(events[1].1.as_ref().unwrap().contains("updated title"));
    }

    fn create_task(db: &Db, title: &str) -> Issue {
        db.create_issue(&CreateIssueParams {
            title: title.into(),
            issue_type: IssueType::Task,
            priority: Priority::P2,
            description: None,
            spec: None,
            fixes: None,
            assignee: None,
            deps: vec![],
            actor: "test-agent".into(),
        })
        .unwrap()
    }

    #[test]
    fn claim_sets_in_progress() {
        let (db, _dir) = open_temp_db();
        let issue = create_task(&db, "implement auth");

        let claimed = db.claim_issue(&issue.id, "agent-1").unwrap();

        assert_eq!(claimed.status, Status::InProgress);
        assert_eq!(claimed.assignee.as_deref(), Some("agent-1"));
    }

    #[test]
    fn double_claim_fails() {
        let (db, _dir) = open_temp_db();
        let issue = create_task(&db, "implement auth");

        db.claim_issue(&issue.id, "agent-1").unwrap();
        let result = db.claim_issue(&issue.id, "agent-2");

        assert!(matches!(result, Err(PensaError::AlreadyClaimed { .. })));
        if let Err(PensaError::AlreadyClaimed { holder, .. }) = result {
            assert_eq!(holder, "agent-1");
        }
    }

    #[test]
    fn release_clears() {
        let (db, _dir) = open_temp_db();
        let issue = create_task(&db, "implement auth");

        db.claim_issue(&issue.id, "agent-1").unwrap();
        let released = db.release_issue(&issue.id, "agent-1").unwrap();

        assert_eq!(released.status, Status::Open);
        assert!(released.assignee.is_none());
    }

    #[test]
    fn close_reopen_cycle() {
        let (db, _dir) = open_temp_db();
        let issue = create_task(&db, "implement auth");

        let closed = db
            .close_issue(&issue.id, Some("done"), false, "agent-1")
            .unwrap();
        assert_eq!(closed.status, Status::Closed);
        assert_eq!(closed.close_reason.as_deref(), Some("done"));
        assert!(closed.closed_at.is_some());

        let reopened = db
            .reopen_issue(&issue.id, Some("not done"), "agent-1")
            .unwrap();
        assert_eq!(reopened.status, Status::Open);
        assert!(reopened.closed_at.is_none());
        assert!(reopened.close_reason.is_none());

        let closed_again = db.close_issue(&issue.id, None, false, "agent-1").unwrap();
        assert_eq!(closed_again.status, Status::Closed);
    }

    #[test]
    fn fixes_auto_close() {
        let (db, _dir) = open_temp_db();

        let bug = db
            .create_issue(&CreateIssueParams {
                title: "login crash".into(),
                issue_type: IssueType::Bug,
                priority: Priority::P0,
                description: None,
                spec: None,
                fixes: None,
                assignee: None,
                deps: vec![],
                actor: "test-agent".into(),
            })
            .unwrap();

        let task = db
            .create_issue(&CreateIssueParams {
                title: "fix login".into(),
                issue_type: IssueType::Task,
                priority: Priority::P1,
                description: None,
                spec: None,
                fixes: Some(bug.id.clone()),
                assignee: None,
                deps: vec![],
                actor: "test-agent".into(),
            })
            .unwrap();

        db.close_issue(&task.id, Some("implemented"), false, "agent-1")
            .unwrap();

        let bug_after = db.get_issue_only(&bug.id).unwrap();
        assert_eq!(bug_after.status, Status::Closed);
        assert!(
            bug_after
                .close_reason
                .as_ref()
                .unwrap()
                .contains(&format!("fixed by {}", task.id))
        );
    }

    #[test]
    fn delete_requires_force() {
        let (db, _dir) = open_temp_db();
        let issue = create_task(&db, "implement auth");

        db.conn
            .execute(
                "INSERT INTO comments (id, issue_id, actor, text, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params!["pn-comment01", issue.id, "agent", "note", now()],
            )
            .unwrap();

        let result = db.delete_issue(&issue.id, false);
        assert!(matches!(result, Err(PensaError::DeleteRequiresForce(_))));
    }

    #[test]
    fn force_delete_cascades() {
        let (db, _dir) = open_temp_db();
        let issue_a = create_task(&db, "task A");
        let issue_b = create_task(&db, "task B");

        db.conn
            .execute(
                "INSERT INTO deps (issue_id, depends_on_id) VALUES (?1, ?2)",
                rusqlite::params![issue_b.id, issue_a.id],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO comments (id, issue_id, actor, text, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params!["pn-comment01", issue_a.id, "agent", "note", now()],
            )
            .unwrap();

        db.delete_issue(&issue_a.id, true).unwrap();

        assert!(matches!(
            db.get_issue_only(&issue_a.id),
            Err(PensaError::NotFound(_))
        ));

        let dep_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM deps WHERE issue_id = ?1 OR depends_on_id = ?1",
                rusqlite::params![issue_a.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dep_count, 0);

        let comment_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM comments WHERE issue_id = ?1",
                rusqlite::params![issue_a.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(comment_count, 0);

        let event_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE issue_id = ?1",
                rusqlite::params![issue_a.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(event_count, 0);
    }
}
