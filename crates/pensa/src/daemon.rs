use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::db::Db;
use crate::error::{ErrorResponse, PensaError};
use crate::types::{CreateIssueParams, IssueType, Priority, Status, UpdateFields};

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
        .route("/issues", post(create_issue))
        .route("/issues/{id}", get(get_issue))
        .route("/issues/{id}", patch(update_issue))
        .route("/issues/{id}", delete(delete_issue))
        .route("/issues/{id}/close", post(close_issue))
        .route("/issues/{id}/reopen", post(reopen_issue))
        .route("/issues/{id}/release", post(release_issue))
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
