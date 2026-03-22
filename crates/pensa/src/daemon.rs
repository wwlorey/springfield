use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::sync::Notify;

use crate::db::Db;
use crate::error::{ErrorResponse, PensaError};
use crate::types::{CreateIssueParams, IssueType, ListFilters, Priority, Status, UpdateFields};

struct DaemonState {
    db: Mutex<Db>,
    project_dir: PathBuf,
    shutdown: Notify,
}

type AppState = Arc<DaemonState>;

struct AppError(PensaError);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            PensaError::NotFound(_) => StatusCode::NOT_FOUND,
            PensaError::AlreadyClaimed { .. }
            | PensaError::CycleDetected
            | PensaError::InvalidStatusTransition { .. }
            | PensaError::DeleteRequiresForce(_) => StatusCode::CONFLICT,
            PensaError::SpecNotFound(_) => StatusCode::UNPROCESSABLE_ENTITY,
            PensaError::FormaUnavailable => StatusCode::SERVICE_UNAVAILABLE,
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

fn forma_port(project_dir: &std::path::Path) -> u16 {
    use sha2::{Digest, Sha256};
    let canonical = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf());
    let input = format!("forma:{}", canonical.to_string_lossy());
    let hash: [u8; 32] = Sha256::digest(input.as_bytes()).into();
    let raw = u16::from_be_bytes([hash[8], hash[9]]);
    10000 + (raw % 50000)
}

fn discover_forma_port(project_dir: &std::path::Path) -> u16 {
    let port_file = project_dir.join(".forma/daemon.port");
    if let Ok(contents) = std::fs::read_to_string(&port_file)
        && let Ok(port) = contents.trim().parse::<u16>()
    {
        return port;
    }
    forma_port(project_dir)
}

async fn validate_spec_against_forma(
    project_dir: &std::path::Path,
    stem: &str,
) -> Result<(), PensaError> {
    let port = discover_forma_port(project_dir);
    let url = format!("http://localhost:{port}/specs/{stem}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| PensaError::Internal(format!("http client error: {e}")))?;
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => Ok(()),
        Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => {
            Err(PensaError::SpecNotFound(stem.to_string()))
        }
        Ok(resp) => Err(PensaError::Internal(format!(
            "forma returned unexpected status: {}",
            resp.status()
        ))),
        Err(_) => Err(PensaError::FormaUnavailable),
    }
}

pub async fn start(port: u16, project_dir: PathBuf) {
    start_with_data_dir(port, project_dir, None).await;
}

pub async fn start_with_data_dir(port: u16, project_dir: PathBuf, data_dir: Option<PathBuf>) {
    let db = match data_dir {
        Some(dd) => {
            let pensa_dir = project_dir.join(".pensa");
            Db::open_with_data_dir(pensa_dir, dd).expect("failed to open database")
        }
        None => Db::open(&project_dir).expect("failed to open database"),
    };
    let state: AppState = Arc::new(DaemonState {
        db: Mutex::new(db),
        project_dir: project_dir.clone(),
        shutdown: Notify::new(),
    });

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
        .route(
            "/issues/{id}/src-refs",
            get(list_src_refs).post(add_src_ref),
        )
        .route("/src-refs/{id}", axum::routing::delete(remove_src_ref))
        .route(
            "/issues/{id}/doc-refs",
            get(list_doc_refs).post(add_doc_ref),
        )
        .route("/doc-refs/{id}", axum::routing::delete(remove_doc_ref))
        .route("/deps", post(add_dep).delete(remove_dep))
        .route("/deps/cycles", get(detect_cycles))
        .route("/export", post(export_jsonl))
        .route("/import", post(import_jsonl))
        .route("/doctor", post(doctor))
        .route("/status", get(project_status))
        .route("/shutdown", post(shutdown_endpoint))
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("failed to bind");

    let pensa_dir = project_dir.join(".pensa");
    let port_file = pensa_dir.join("daemon.port");
    let project_file = pensa_dir.join("daemon.project");
    let _ = std::fs::create_dir_all(&pensa_dir);
    if let Err(e) = std::fs::write(&port_file, port.to_string()) {
        tracing::warn!("failed to write port file: {e}");
    }
    let canonical = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.clone());
    if let Err(e) = std::fs::write(&project_file, canonical.to_string_lossy().as_bytes()) {
        tracing::warn!("failed to write project file: {e}");
    }

    tracing::info!("pensa daemon listening on port {port}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(state))
        .await
        .expect("server error");

    let _ = std::fs::remove_file(&port_file);
    let _ = std::fs::remove_file(&project_file);
}

async fn shutdown_endpoint(State(state): State<AppState>) -> StatusCode {
    state.shutdown.notify_one();
    StatusCode::OK
}

async fn shutdown_signal(state: AppState) {
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

    let project_dir_gone = async {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            if !state.project_dir.exists() {
                tracing::info!("project directory gone, shutting down");
                break;
            }
        }
    };

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
        () = state.shutdown.notified() => {},
        () = project_dir_gone => {},
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
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateIssueBody>,
) -> Result<impl IntoResponse, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    if let Some(ref spec) = body.spec {
        validate_spec_against_forma(&state.project_dir, spec).await?;
    }

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

    let db = state.db.lock().unwrap();
    let issue = db.create_issue(&params)?;
    Ok((StatusCode::CREATED, Json(issue)))
}

async fn get_issue(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().unwrap();
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
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateIssueBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    if let Some(ref spec) = body.spec {
        validate_spec_against_forma(&state.project_dir, spec).await?;
    }

    let db = state.db.lock().unwrap();

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
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<StatusCode, AppError> {
    let db = state.db.lock().unwrap();
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
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<CloseBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    let db = state.db.lock().unwrap();
    let issue = db.close_issue(&id, body.reason.as_deref(), body.force, &actor)?;
    Ok(Json(serde_json::to_value(issue).unwrap()))
}

#[derive(Deserialize)]
struct ReopenBody {
    reason: Option<String>,
    actor: Option<String>,
}

async fn reopen_issue(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ReopenBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    let db = state.db.lock().unwrap();
    let issue = db.reopen_issue(&id, body.reason.as_deref(), &actor)?;
    Ok(Json(serde_json::to_value(issue).unwrap()))
}

async fn release_issue(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = actor_from_headers(&headers).unwrap_or_else(|| "unknown".to_string());

    let db = state.db.lock().unwrap();
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
    State(state): State<AppState>,
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

    let db = state.db.lock().unwrap();
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
    State(state): State<AppState>,
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

    let db = state.db.lock().unwrap();
    let issues = db.ready_issues(&filters)?;
    let values: Vec<serde_json::Value> = issues
        .into_iter()
        .map(|i| serde_json::to_value(i).unwrap())
        .collect();
    Ok(Json(values))
}

async fn blocked_issues(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = state.db.lock().unwrap();
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
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = state.db.lock().unwrap();
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
    State(state): State<AppState>,
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

    let db = state.db.lock().unwrap();
    let result = db.count_issues(&group_by)?;
    Ok(Json(result))
}

async fn project_status(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = state.db.lock().unwrap();
    let entries = db.project_status()?;
    let values: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect();
    Ok(Json(values))
}

async fn issue_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = state.db.lock().unwrap();
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
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AddDepBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    let db = state.db.lock().unwrap();
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
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RemoveDepQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = actor_from_headers(&headers).unwrap_or_else(|| "unknown".to_string());

    let db = state.db.lock().unwrap();
    db.remove_dep(&query.issue_id, &query.depends_on_id, &actor)?;
    Ok(Json(serde_json::json!({
        "status": "removed",
        "issue_id": query.issue_id,
        "depends_on_id": query.depends_on_id,
    })))
}

async fn list_deps(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = state.db.lock().unwrap();
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
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<DepTreeQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = state.db.lock().unwrap();
    let nodes = db.dep_tree(&id, &query.direction)?;
    let values: Vec<serde_json::Value> = nodes
        .into_iter()
        .map(|n| serde_json::to_value(n).unwrap())
        .collect();
    Ok(Json(values))
}

async fn detect_cycles(State(state): State<AppState>) -> Result<Json<Vec<Vec<String>>>, AppError> {
    let db = state.db.lock().unwrap();
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
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AddCommentBody>,
) -> Result<impl IntoResponse, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    let db = state.db.lock().unwrap();
    let comment = db.add_comment(&id, &actor, &body.text)?;
    Ok((StatusCode::CREATED, Json(comment)))
}

async fn list_comments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = state.db.lock().unwrap();
    let comments = db.list_comments(&id)?;
    let values: Vec<serde_json::Value> = comments
        .into_iter()
        .map(|c| serde_json::to_value(c).unwrap())
        .collect();
    Ok(Json(values))
}

// --- Src-ref endpoints ---

#[derive(Deserialize)]
struct AddRefBody {
    path: String,
    reason: Option<String>,
    actor: Option<String>,
}

async fn add_src_ref(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AddRefBody>,
) -> Result<impl IntoResponse, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    let db = state.db.lock().unwrap();
    let src_ref = db.add_src_ref(&id, &body.path, body.reason.as_deref(), &actor)?;
    Ok((StatusCode::CREATED, Json(src_ref)))
}

async fn list_src_refs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = state.db.lock().unwrap();
    let refs = db.list_src_refs(&id)?;
    let values: Vec<serde_json::Value> = refs
        .into_iter()
        .map(|r| serde_json::to_value(r).unwrap())
        .collect();
    Ok(Json(values))
}

async fn remove_src_ref(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let actor = actor_from_headers(&headers).unwrap_or_else(|| "unknown".to_string());

    let db = state.db.lock().unwrap();
    db.remove_src_ref(&id, &actor)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Doc-ref endpoints ---

async fn add_doc_ref(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AddRefBody>,
) -> Result<impl IntoResponse, AppError> {
    let actor = body
        .actor
        .or_else(|| actor_from_headers(&headers))
        .unwrap_or_else(|| "unknown".to_string());

    let db = state.db.lock().unwrap();
    let doc_ref = db.add_doc_ref(&id, &body.path, body.reason.as_deref(), &actor)?;
    Ok((StatusCode::CREATED, Json(doc_ref)))
}

async fn list_doc_refs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = state.db.lock().unwrap();
    let refs = db.list_doc_refs(&id)?;
    let values: Vec<serde_json::Value> = refs
        .into_iter()
        .map(|r| serde_json::to_value(r).unwrap())
        .collect();
    Ok(Json(values))
}

async fn remove_doc_ref(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let actor = actor_from_headers(&headers).unwrap_or_else(|| "unknown".to_string());

    let db = state.db.lock().unwrap();
    db.remove_doc_ref(&id, &actor)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Data endpoints ---

async fn export_jsonl(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().unwrap();
    let result = db.export_jsonl()?;
    Ok(Json(serde_json::to_value(result).unwrap()))
}

async fn import_jsonl(State(state): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().unwrap();
    let result = db.import_jsonl()?;
    Ok(Json(serde_json::to_value(result).unwrap()))
}

#[derive(Deserialize)]
struct DoctorQuery {
    #[serde(default)]
    fix: bool,
}

async fn doctor(
    State(state): State<AppState>,
    Query(query): Query<DoctorQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = state.db.lock().unwrap();
    let report = db.doctor(query.fix)?;
    Ok(Json(serde_json::to_value(report).unwrap()))
}
