use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::db::Db;
use crate::error::{ErrorResponse, PensaError};
use crate::types::{CreateIssueParams, IssueType, ListFilters, Priority, Status, UpdateFields};

type AppState = Arc<Mutex<Db>>;

struct AppError(PensaError);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            PensaError::NotFound(_) => StatusCode::NOT_FOUND,
            PensaError::AlreadyClaimed { .. }
            | PensaError::CycleDetected
            | PensaError::InvalidStatusTransition { .. }
            | PensaError::DeleteRequiresForce(_) => StatusCode::CONFLICT,
            PensaError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = ErrorResponse::from(&self.0);
        (status, Json(body)).into_response()
    }
}

impl From<PensaError> for AppError {
    fn from(err: PensaError) -> Self {
        AppError(err)
    }
}

fn actor_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-pensa-actor")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

pub async fn start(port: u16, project_dir: PathBuf) {
    let db = Db::open(&project_dir).expect("failed to open database");
    let state: AppState = Arc::new(Mutex::new(db));

    let app = Router::new()
        .route("/issues", get(list_issues).post(create_issue))
        .route("/issues/ready", get(ready_issues))
        .route("/issues/blocked", get(blocked_issues))
        .route("/issues/search", get(search_issues))
        .route("/issues/count", get(count_issues))
        .route(
            "/issues/{id}",
            get(get_issue).patch(update_issue).delete(delete_issue),
        )
        .route("/issues/{id}/close", post(close_issue))
        .route("/issues/{id}/reopen", post(reopen_issue))
        .route("/issues/{id}/release", post(release_issue))
        .route("/issues/{id}/history", get(issue_history))
        .route("/issues/{id}/deps", get(list_deps))
        .route("/issues/{id}/deps/tree", get(dep_tree))
        .route(
            "/issues/{id}/comments",
            get(list_comments).post(add_comment),
        )
        .route("/deps", post(add_dep).delete(remove_dep))
        .route("/deps/cycles", get(detect_cycles))
        .route("/export", post(export_jsonl))
        .route("/import", post(import_jsonl))
        .route("/doctor", post(doctor))
        .route("/status", get(project_status))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("failed to bind");

    tracing::info!("pensa daemon listening on port {port}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install ctrl+c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("shutdown signal received");
}

// --- Issue endpoints ---

#[derive(Deserialize)]
struct CreateIssueBody {
    title: String,
    issue_type: IssueType,
    #[serde(default = "default_priority")]
    priority: Priority,
    description: Option<String>,
    spec: Option<String>,
    fixes: Option<String>,
    assignee: Option<String>,
    #[serde(default)]
    deps: Vec<String>,
    actor: Option<String>,
}

fn default_priority() -> Priority {
    Priority::P2
}

async fn create_issue(
    State(db): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateIssueBody>,
) -> Result<impl IntoResponse, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    let params = CreateIssueParams {
        title: body.title,
        issue_type: body.issue_type,
        priority: body.priority,
        description: body.description,
        spec: body.spec,
        fixes: body.fixes,
        assignee: body.assignee,
        deps: body.deps,
        actor,
    };

    let db = db.lock().unwrap();
    let issue = db.create_issue(&params)?;
    Ok((StatusCode::CREATED, Json(issue)))
}

async fn get_issue(
    State(db): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db.lock().unwrap();
    let detail = db.get_issue(&id)?;
    Ok(Json(serde_json::to_value(detail).unwrap()))
}

#[derive(Deserialize)]
struct UpdateIssueBody {
    title: Option<String>,
    description: Option<String>,
    priority: Option<Priority>,
    status: Option<Status>,
    assignee: Option<String>,
    spec: Option<String>,
    fixes: Option<String>,
    #[serde(default)]
    claim: bool,
    #[serde(default)]
    unclaim: bool,
    actor: Option<String>,
}

async fn update_issue(
    State(db): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateIssueBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    let db = db.lock().unwrap();

    if body.claim {
        let issue = db.claim_issue(&id, &actor)?;
        return Ok(Json(serde_json::to_value(issue).unwrap()));
    }

    if body.unclaim {
        let issue = db.release_issue(&id, &actor)?;
        return Ok(Json(serde_json::to_value(issue).unwrap()));
    }

    let fields = UpdateFields {
        title: body.title,
        description: body.description,
        priority: body.priority,
        status: body.status,
        assignee: body.assignee,
        spec: body.spec,
        fixes: body.fixes,
    };

    let issue = db.update_issue(&id, &fields, &actor)?;
    Ok(Json(serde_json::to_value(issue).unwrap()))
}

#[derive(Deserialize)]
struct DeleteQuery {
    #[serde(default)]
    force: bool,
}

async fn delete_issue(
    State(db): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<StatusCode, AppError> {
    let db = db.lock().unwrap();
    db.delete_issue(&id, query.force)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct CloseBody {
    reason: Option<String>,
    #[serde(default)]
    force: bool,
    actor: Option<String>,
}

async fn close_issue(
    State(db): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CloseBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    let db = db.lock().unwrap();
    let issue = db.close_issue(&id, body.reason.as_deref(), body.force, &actor)?;
    Ok(Json(serde_json::to_value(issue).unwrap()))
}

#[derive(Deserialize)]
struct ReopenBody {
    reason: Option<String>,
    actor: Option<String>,
}

async fn reopen_issue(
    State(db): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ReopenBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    let db = db.lock().unwrap();
    let issue = db.reopen_issue(&id, body.reason.as_deref(), &actor)?;
    Ok(Json(serde_json::to_value(issue).unwrap()))
}

async fn release_issue(
    State(db): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = actor_from_headers(&headers).unwrap_or_else(|| "unknown".to_string());

    let db = db.lock().unwrap();
    let issue = db.release_issue(&id, &actor)?;
    Ok(Json(serde_json::to_value(issue).unwrap()))
}

// --- Query endpoints ---

#[derive(Deserialize)]
struct ListQuery {
    status: Option<Status>,
    priority: Option<Priority>,
    assignee: Option<String>,
    #[serde(rename = "type")]
    issue_type: Option<IssueType>,
    spec: Option<String>,
    sort: Option<String>,
    limit: Option<usize>,
}

async fn list_issues(
    State(db): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let filters = ListFilters {
        status: query.status,
        priority: query.priority,
        assignee: query.assignee,
        issue_type: query.issue_type,
        spec: query.spec,
        sort: query.sort,
        limit: query.limit,
    };

    let db = db.lock().unwrap();
    let issues = db.list_issues(&filters)?;
    let values: Vec<serde_json::Value> = issues
        .into_iter()
        .map(|i| serde_json::to_value(i).unwrap())
        .collect();
    Ok(Json(values))
}

#[derive(Deserialize)]
struct ReadyQuery {
    priority: Option<Priority>,
    assignee: Option<String>,
    #[serde(rename = "type")]
    issue_type: Option<IssueType>,
    spec: Option<String>,
    limit: Option<usize>,
}

async fn ready_issues(
    State(db): State<AppState>,
    Query(query): Query<ReadyQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let filters = ListFilters {
        priority: query.priority,
        assignee: query.assignee,
        issue_type: query.issue_type,
        spec: query.spec,
        limit: query.limit,
        ..Default::default()
    };

    let db = db.lock().unwrap();
    let issues = db.ready_issues(&filters)?;
    let values: Vec<serde_json::Value> = issues
        .into_iter()
        .map(|i| serde_json::to_value(i).unwrap())
        .collect();
    Ok(Json(values))
}

async fn blocked_issues(
    State(db): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let issues = db.blocked_issues()?;
    let values: Vec<serde_json::Value> = issues
        .into_iter()
        .map(|i| serde_json::to_value(i).unwrap())
        .collect();
    Ok(Json(values))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn search_issues(
    State(db): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let issues = db.search_issues(&query.q)?;
    let values: Vec<serde_json::Value> = issues
        .into_iter()
        .map(|i| serde_json::to_value(i).unwrap())
        .collect();
    Ok(Json(values))
}

#[derive(Deserialize)]
struct CountQuery {
    #[serde(default)]
    by_status: bool,
    #[serde(default)]
    by_priority: bool,
    #[serde(default)]
    by_issue_type: bool,
    #[serde(default)]
    by_assignee: bool,
}

async fn count_issues(
    State(db): State<AppState>,
    Query(query): Query<CountQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let mut group_by = Vec::new();
    if query.by_status {
        group_by.push("status");
    }
    if query.by_priority {
        group_by.push("priority");
    }
    if query.by_issue_type {
        group_by.push("issue_type");
    }
    if query.by_assignee {
        group_by.push("assignee");
    }

    let db = db.lock().unwrap();
    let result = db.count_issues(&group_by)?;
    Ok(Json(result))
}

async fn project_status(
    State(db): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let entries = db.project_status()?;
    let values: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect();
    Ok(Json(values))
}

async fn issue_history(
    State(db): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let events = db.issue_history(&id)?;
    let values: Vec<serde_json::Value> = events
        .into_iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect();
    Ok(Json(values))
}

// --- Dependency endpoints ---

#[derive(Deserialize)]
struct AddDepBody {
    issue_id: String,
    depends_on_id: String,
    actor: Option<String>,
}

async fn add_dep(
    State(db): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AddDepBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    let db = db.lock().unwrap();
    db.add_dep(&body.issue_id, &body.depends_on_id, &actor)?;
    Ok(Json(serde_json::json!({
        "status": "added",
        "issue_id": body.issue_id,
        "depends_on_id": body.depends_on_id,
    })))
}

#[derive(Deserialize)]
struct RemoveDepQuery {
    issue_id: String,
    depends_on_id: String,
}

async fn remove_dep(
    State(db): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RemoveDepQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = actor_from_headers(&headers).unwrap_or_else(|| "unknown".to_string());

    let db = db.lock().unwrap();
    db.remove_dep(&query.issue_id, &query.depends_on_id, &actor)?;
    Ok(Json(serde_json::json!({
        "status": "removed",
        "issue_id": query.issue_id,
        "depends_on_id": query.depends_on_id,
    })))
}

async fn list_deps(
    State(db): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let deps = db.list_deps(&id)?;
    let values: Vec<serde_json::Value> = deps
        .into_iter()
        .map(|i| serde_json::to_value(i).unwrap())
        .collect();
    Ok(Json(values))
}

#[derive(Deserialize)]
struct DepTreeQuery {
    #[serde(default = "default_direction")]
    direction: String,
}

fn default_direction() -> String {
    "down".to_string()
}

async fn dep_tree(
    State(db): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DepTreeQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let nodes = db.dep_tree(&id, &query.direction)?;
    let values: Vec<serde_json::Value> = nodes
        .into_iter()
        .map(|n| serde_json::to_value(n).unwrap())
        .collect();
    Ok(Json(values))
}

async fn detect_cycles(State(db): State<AppState>) -> Result<Json<Vec<Vec<String>>>, AppError> {
    let db = db.lock().unwrap();
    let cycles = db.detect_cycles()?;
    Ok(Json(cycles))
}

// --- Comment endpoints ---

#[derive(Deserialize)]
struct AddCommentBody {
    text: String,
    actor: Option<String>,
}

async fn add_comment(
    State(db): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AddCommentBody>,
) -> Result<impl IntoResponse, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    let db = db.lock().unwrap();
    let comment = db.add_comment(&id, &actor, &body.text)?;
    Ok((StatusCode::CREATED, Json(comment)))
}

async fn list_comments(
    State(db): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let comments = db.list_comments(&id)?;
    let values: Vec<serde_json::Value> = comments
        .into_iter()
        .map(|c| serde_json::to_value(c).unwrap())
        .collect();
    Ok(Json(values))
}

// --- Data endpoints ---

async fn export_jsonl(State(db): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    let db = db.lock().unwrap();
    let result = db.export_jsonl()?;
    Ok(Json(serde_json::to_value(result).unwrap()))
}

async fn import_jsonl(State(db): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    let db = db.lock().unwrap();
    let result = db.import_jsonl()?;
    Ok(Json(serde_json::to_value(result).unwrap()))
}

#[derive(Deserialize)]
struct DoctorQuery {
    #[serde(default)]
    fix: bool,
}

async fn doctor(
    State(db): State<AppState>,
    Query(query): Query<DoctorQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db.lock().unwrap();
    let report = db.doctor(query.fix)?;
    Ok(Json(serde_json::to_value(report).unwrap()))
}
