use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::types::FormaError;

type AppState = Arc<Mutex<Db>>;

struct AppError(FormaError);

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    code: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            FormaError::NotFound(_) => StatusCode::NOT_FOUND,
            FormaError::AlreadyExists(_) | FormaError::CycleDetected => StatusCode::CONFLICT,
            FormaError::RequiredSection(_) | FormaError::ValidationFailed(_) => {
                StatusCode::BAD_REQUEST
            }
            FormaError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = ErrorResponse {
            error: self.0.to_string(),
            code: self.0.code().to_string(),
        };
        (status, Json(body)).into_response()
    }
}

impl From<FormaError> for AppError {
    fn from(err: FormaError) -> Self {
        AppError(err)
    }
}

fn actor_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forma-actor")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

pub async fn start(port: u16, project_dir: PathBuf) {
    start_with_data_dir(port, project_dir, None).await;
}

pub async fn start_with_data_dir(port: u16, project_dir: PathBuf, data_dir: Option<PathBuf>) {
    let db = match data_dir {
        Some(dd) => {
            let forma_dir = project_dir.join(".forma");
            Db::open_with_data_dir(forma_dir, dd).expect("failed to open database")
        }
        None => Db::open(&project_dir).expect("failed to open database"),
    };
    let state: AppState = Arc::new(Mutex::new(db));

    let app = Router::new()
        // Spec routes
        .route("/specs", get(list_specs).post(create_spec))
        .route("/specs/search", get(search_specs))
        .route("/specs/count", get(count_specs))
        .route(
            "/specs/{stem}",
            get(get_spec).patch(update_spec).delete(delete_spec),
        )
        .route("/specs/{stem}/history", get(spec_history))
        // Section routes
        .route(
            "/specs/{stem}/sections",
            get(list_sections).post(add_section),
        )
        .route(
            "/specs/{stem}/sections/{slug}",
            get(get_section).put(set_section).delete(remove_section),
        )
        .route("/specs/{stem}/sections/{slug}/move", patch(move_section))
        // Ref routes
        .route("/specs/{stem}/refs", get(list_refs).post(add_ref))
        .route("/specs/{stem}/refs/tree", get(ref_tree))
        .route("/specs/{stem}/refs/{target}", delete(remove_ref))
        .route("/refs/cycles", get(ref_cycles))
        // Data routes
        .route("/export", post(export_jsonl))
        .route("/import", post(import_jsonl))
        .route("/check", get(check))
        .route("/doctor", post(doctor))
        // Status
        .route("/status", get(project_status))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("failed to bind");

    let forma_dir = project_dir.join(".forma");
    let port_file = forma_dir.join("daemon.port");
    let project_file = forma_dir.join("daemon.project");
    let _ = std::fs::create_dir_all(&forma_dir);
    if let Err(e) = std::fs::write(&port_file, port.to_string()) {
        tracing::warn!("failed to write port file: {e}");
    }
    let canonical = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.clone());
    if let Err(e) = std::fs::write(&project_file, canonical.to_string_lossy().as_bytes()) {
        tracing::warn!("failed to write project file: {e}");
    }

    tracing::info!("forma daemon listening on port {port}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");

    let _ = std::fs::remove_file(&port_file);
    let _ = std::fs::remove_file(&project_file);
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

// --- Spec endpoints ---

#[derive(Deserialize)]
struct CreateSpecBody {
    stem: String,
    src: Option<String>,
    purpose: String,
}

async fn create_spec(
    State(db): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateSpecBody>,
) -> Result<impl IntoResponse, AppError> {
    let actor = actor_from_headers(&headers);
    let db = db.lock().unwrap();
    let spec = db.create_spec(
        &body.stem,
        body.src.as_deref(),
        &body.purpose,
        actor.as_deref(),
    )?;
    Ok((StatusCode::CREATED, Json(spec)))
}

async fn get_spec(
    State(db): State<AppState>,
    Path(stem): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db.lock().unwrap();
    let detail = db.get_spec(&stem)?;
    Ok(Json(serde_json::to_value(detail).unwrap()))
}

#[derive(Deserialize)]
struct ListSpecsQuery {
    status: Option<String>,
}

async fn list_specs(
    State(db): State<AppState>,
    Query(query): Query<ListSpecsQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let specs = db.list_specs(query.status.as_deref())?;
    let values: Vec<serde_json::Value> = specs
        .into_iter()
        .map(|s| serde_json::to_value(s).unwrap())
        .collect();
    Ok(Json(values))
}

#[derive(Deserialize)]
struct UpdateSpecBody {
    status: Option<String>,
    src: Option<String>,
    purpose: Option<String>,
}

async fn update_spec(
    State(db): State<AppState>,
    Path(stem): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateSpecBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = actor_from_headers(&headers);
    let db = db.lock().unwrap();
    let spec = db.update_spec(
        &stem,
        body.status.as_deref(),
        body.src.as_deref(),
        body.purpose.as_deref(),
        actor.as_deref(),
    )?;
    Ok(Json(serde_json::to_value(spec).unwrap()))
}

#[derive(Deserialize)]
struct DeleteQuery {
    #[serde(default)]
    force: bool,
}

async fn delete_spec(
    State(db): State<AppState>,
    Path(stem): Path<String>,
    headers: HeaderMap,
    Query(query): Query<DeleteQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = actor_from_headers(&headers);
    let db = db.lock().unwrap();
    db.delete_spec(&stem, query.force, actor.as_deref())?;
    Ok(Json(serde_json::json!({
        "status": "deleted",
        "stem": stem,
    })))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

async fn search_specs(
    State(db): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let specs = db.search_specs(&query.q)?;
    let values: Vec<serde_json::Value> = specs
        .into_iter()
        .map(|s| serde_json::to_value(s).unwrap())
        .collect();
    Ok(Json(values))
}

#[derive(Deserialize)]
struct CountQuery {
    #[serde(default)]
    by_status: bool,
}

async fn count_specs(
    State(db): State<AppState>,
    Query(query): Query<CountQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db.lock().unwrap();
    let result = db.count_specs(query.by_status)?;
    Ok(Json(serde_json::to_value(result).unwrap()))
}

async fn project_status(State(db): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    let db = db.lock().unwrap();
    let status = db.project_status()?;
    Ok(Json(serde_json::to_value(status).unwrap()))
}

async fn spec_history(
    State(db): State<AppState>,
    Path(stem): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let events = db.spec_history(&stem)?;
    let values: Vec<serde_json::Value> = events
        .into_iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect();
    Ok(Json(values))
}

// --- Section endpoints ---

#[derive(Deserialize)]
struct AddSectionBody {
    name: String,
    #[serde(default)]
    body: String,
    after: Option<String>,
}

async fn add_section(
    State(db): State<AppState>,
    Path(stem): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AddSectionBody>,
) -> Result<impl IntoResponse, AppError> {
    let actor = actor_from_headers(&headers);
    let db = db.lock().unwrap();
    let section = db.add_section(
        &stem,
        &body.name,
        &body.body,
        body.after.as_deref(),
        actor.as_deref(),
    )?;
    Ok((StatusCode::CREATED, Json(section)))
}

#[derive(Deserialize)]
struct SetSectionBody {
    body: String,
}

async fn set_section(
    State(db): State<AppState>,
    Path((stem, slug)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<SetSectionBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = actor_from_headers(&headers);
    let db = db.lock().unwrap();
    let section = db.set_section(&stem, &slug, &body.body, actor.as_deref())?;
    Ok(Json(serde_json::to_value(section).unwrap()))
}

async fn get_section(
    State(db): State<AppState>,
    Path((stem, slug)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let db = db.lock().unwrap();
    let section = db.get_section(&stem, &slug)?;
    Ok(Json(serde_json::to_value(section).unwrap()))
}

async fn list_sections(
    State(db): State<AppState>,
    Path(stem): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let sections = db.list_sections(&stem)?;
    let values: Vec<serde_json::Value> = sections
        .into_iter()
        .map(|s| serde_json::to_value(s).unwrap())
        .collect();
    Ok(Json(values))
}

async fn remove_section(
    State(db): State<AppState>,
    Path((stem, slug)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = actor_from_headers(&headers);
    let db = db.lock().unwrap();
    db.remove_section(&stem, &slug, actor.as_deref())?;
    Ok(Json(serde_json::json!({
        "status": "removed",
        "spec": stem,
        "slug": slug,
    })))
}

#[derive(Deserialize)]
struct MoveSectionBody {
    after: String,
}

async fn move_section(
    State(db): State<AppState>,
    Path((stem, slug)): Path<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<MoveSectionBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = actor_from_headers(&headers);
    let db = db.lock().unwrap();
    let section = db.move_section(&stem, &slug, &body.after, actor.as_deref())?;
    Ok(Json(serde_json::to_value(section).unwrap()))
}

// --- Ref endpoints ---

#[derive(Deserialize)]
struct AddRefBody {
    target: String,
}

async fn add_ref(
    State(db): State<AppState>,
    Path(stem): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AddRefBody>,
) -> Result<impl IntoResponse, AppError> {
    let actor = actor_from_headers(&headers);
    let db = db.lock().unwrap();
    db.add_ref(&stem, &body.target, actor.as_deref())?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "status": "added",
            "from": stem,
            "to": body.target,
        })),
    ))
}

async fn remove_ref(
    State(db): State<AppState>,
    Path((stem, target)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let actor = actor_from_headers(&headers);
    let db = db.lock().unwrap();
    db.remove_ref(&stem, &target, actor.as_deref())?;
    Ok(Json(serde_json::json!({
        "status": "removed",
        "from": stem,
        "to": target,
    })))
}

async fn list_refs(
    State(db): State<AppState>,
    Path(stem): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let specs = db.list_refs(&stem)?;
    let values: Vec<serde_json::Value> = specs
        .into_iter()
        .map(|s| serde_json::to_value(s).unwrap())
        .collect();
    Ok(Json(values))
}

#[derive(Deserialize)]
struct RefTreeQuery {
    #[serde(default = "default_direction")]
    direction: String,
}

fn default_direction() -> String {
    "down".to_string()
}

async fn ref_tree(
    State(db): State<AppState>,
    Path(stem): Path<String>,
    Query(query): Query<RefTreeQuery>,
) -> Result<Json<Vec<serde_json::Value>>, AppError> {
    let db = db.lock().unwrap();
    let nodes = db.ref_tree(&stem, &query.direction)?;
    let values: Vec<serde_json::Value> = nodes
        .into_iter()
        .map(|n| serde_json::to_value(n).unwrap())
        .collect();
    Ok(Json(values))
}

async fn ref_cycles(State(db): State<AppState>) -> Result<Json<Vec<Vec<String>>>, AppError> {
    let db = db.lock().unwrap();
    let cycles = db.detect_cycles()?;
    Ok(Json(cycles))
}

// --- Data endpoints ---

async fn export_jsonl(State(db): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    let db = db.lock().unwrap();
    let result = db.export_jsonl()?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "specs": result.specs,
        "sections": result.sections,
        "refs": result.refs,
    })))
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

async fn check(State(db): State<AppState>) -> Result<Json<serde_json::Value>, AppError> {
    let report = tokio::task::spawn_blocking(move || {
        let db = db.lock().unwrap();
        let project_dir = db
            .forma_dir
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let pensa = crate::db::pensa_url(project_dir);
        db.check(Some(&pensa))
    })
    .await
    .map_err(|e| AppError(FormaError::Internal(format!("check task failed: {e}"))))??;
    Ok(Json(serde_json::to_value(report).unwrap()))
}
