use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AppState {
    project_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({ "error": self.0.to_string() });
        (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Start the Axum web server on the given port, serving data from `project_dir`.
pub async fn serve(project_dir: PathBuf, port: u16) -> anyhow::Result<()> {
    let state = Arc::new(AppState {
        project_dir: project_dir.clone(),
    });

    let thumbnail_dir = project_dir.join("thumbnails");

    // Resolve static dir: check next to the binary first, then fall back to
    // the source tree location (for development).
    let static_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("web").join("dist")))
        .filter(|p| p.join("index.html").exists())
        .unwrap_or_else(|| {
            // Development fallback: ar-edit source tree
            let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../web/dist");
            if dev.join("index.html").exists() {
                dev
            } else {
                project_dir.join("web").join("dist")
            }
        });

    let app = Router::new()
        .route("/api/manifest", get(get_manifest))
        .route("/api/sources", get(get_sources))
        .route("/api/sources/{id}", get(get_source))
        .route("/api/edits", get(list_edits))
        .route("/api/edits/{name}", get(get_edit))
        .route("/api/transcripts/{id}", get(get_transcript))
        .route("/api/play", post(post_play))
        .route("/api/sources/{id}/video", get(get_source_video))
        .route("/api/sources/{id}/thumbnail", get(get_source_thumbnail))
        .route("/api/sources/{id}/rotation", get(get_source_rotation))
        .route("/api/sources/{id}/rotation", post(post_source_rotation))
        .nest_service("/api/thumbnails", ServeDir::new(&thumbnail_dir))
        .fallback_service(ServeDir::new(&static_dir))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    eprintln!("Serving project '{}' on http://localhost:{port}", project_dir.display());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn get_manifest(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let manifest = ar_edit_core::project::read_manifest(&state.project_dir)?;
    Ok(Json(manifest))
}

async fn get_sources(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let manifest = ar_edit_core::project::read_manifest(&state.project_dir)?;
    Ok(Json(manifest.sources))
}

async fn get_source(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let manifest = ar_edit_core::project::read_manifest(&state.project_dir)?;
    let source = manifest
        .sources
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| anyhow::anyhow!("source '{}' not found", id))?;
    Ok(Json(source))
}

async fn list_edits(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let edits_dir = state.project_dir.join("edits");
    let mut names = Vec::new();
    if edits_dir.is_dir() {
        for entry in std::fs::read_dir(&edits_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".edit.json") {
                names.push(name.trim_end_matches(".edit.json").to_string());
            }
        }
    }
    names.sort();
    Ok(Json(names))
}

async fn get_edit(
    State(state): State<Arc<AppState>>,
    AxumPath(name): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let path = state
        .project_dir
        .join("edits")
        .join(format!("{name}.edit.json"));
    let doc = ar_edit_core::models::EditDocument::load(&path)
        .map_err(|e| anyhow::anyhow!("failed to load edit '{}': {}", name, e))?;
    Ok(Json(doc))
}

async fn get_transcript(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let transcript = ar_edit_core::transcript_ops::read(&state.project_dir, &id)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(Json(transcript))
}

// ---------------------------------------------------------------------------
// Video streaming
// ---------------------------------------------------------------------------

async fn get_source_video(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    req: axum::extract::Request,
) -> Result<Response, AppError> {
    let manifest = ar_edit_core::project::read_manifest(&state.project_dir)?;
    let source = manifest
        .sources
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| anyhow::anyhow!("source '{}' not found", id))?;

    let video_path = state.project_dir.join(&source.path);
    let mut service = ServeDir::new(video_path.parent().unwrap_or(&state.project_dir));
    let file_name = video_path.file_name().unwrap_or_default().to_string_lossy().to_string();

    // Rewrite the URI to just the filename so ServeDir can find it
    let (mut parts, body) = req.into_parts();
    parts.uri = format!("/{file_name}").parse().unwrap();
    let req = axum::extract::Request::from_parts(parts, body);

    let resp = service.try_call(req).await
        .map_err(|e| anyhow::anyhow!("failed to serve video: {}", e))?;
    Ok(resp.into_response())
}

// ---------------------------------------------------------------------------
// Thumbnail endpoint
// ---------------------------------------------------------------------------

async fn get_source_thumbnail(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Response, AppError> {
    let thumbnail_dir = state.project_dir.join("thumbnails");
    if thumbnail_dir.is_dir() {
        // Find the first thumbnail matching {source_id}_*.jpg
        let prefix = format!("{}_", id);
        if let Ok(entries) = std::fs::read_dir(&thumbnail_dir) {
            let mut matches: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(&prefix) && n.ends_with(".jpg"))
                        .unwrap_or(false)
                })
                .collect();
            matches.sort();
            if let Some(path) = matches.first() {
                let bytes = std::fs::read(path)?;
                return Ok((
                    StatusCode::OK,
                    [("content-type", "image/jpeg")],
                    bytes,
                )
                    .into_response());
            }
        }
    }
    Ok(StatusCode::NOT_FOUND.into_response())
}

// ---------------------------------------------------------------------------
// Rotation endpoints
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, serde::Serialize)]
struct RotationResponse {
    degrees: u16,
}

async fn get_source_rotation(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let path = state
        .project_dir
        .join("web-meta")
        .join(format!("{id}.rotation.json"));
    if path.exists() {
        let data = std::fs::read_to_string(&path)?;
        let rot: RotationResponse = serde_json::from_str(&data)?;
        Ok(Json(rot))
    } else {
        Ok(Json(RotationResponse { degrees: 0 }))
    }
}

async fn post_source_rotation(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RotationResponse>,
) -> Result<impl IntoResponse, AppError> {
    let degrees = body.degrees;
    if degrees != 0 && degrees != 90 && degrees != 180 && degrees != 270 {
        return Err(AppError(anyhow::anyhow!(
            "degrees must be 0, 90, 180, or 270 (got {})",
            degrees
        )));
    }
    let meta_dir = state.project_dir.join("web-meta");
    std::fs::create_dir_all(&meta_dir)?;
    let path = meta_dir.join(format!("{id}.rotation.json"));
    let data = serde_json::to_string_pretty(&RotationResponse { degrees })?;
    std::fs::write(&path, data)?;
    Ok(Json(RotationResponse { degrees }))
}

// ---------------------------------------------------------------------------
// Play endpoint
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct PlayRequest {
    /// Source ID for direct source playback.
    #[serde(default)]
    target: Option<String>,
    /// Shot ID for edit-based playback (requires `edit` to be set).
    #[serde(default)]
    shot: Option<String>,
    /// Edit name for shot-based playback.
    #[serde(default)]
    edit: Option<String>,
}

async fn post_play(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PlayRequest>,
) -> Result<impl IntoResponse, AppError> {
    let player = ar_edit_core::playback::detect_player()
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Determine what to play: either a shot from an edit, or a source directly.
    if let (Some(edit_name), Some(shot_id)) = (&req.edit, &req.shot) {
        // Shot playback: load the edit document, find the shot, resolve source path
        let edit_path = state
            .project_dir
            .join("edits")
            .join(format!("{edit_name}.edit.json"));
        let doc = ar_edit_core::models::EditDocument::load(&edit_path)
            .map_err(|e| anyhow::anyhow!("failed to load edit '{}': {}", edit_name, e))?;

        let shot = doc
            .snapshot
            .shots
            .iter()
            .find(|s| s.id == *shot_id)
            .ok_or_else(|| anyhow::anyhow!("shot '{}' not found in edit '{}'", shot_id, edit_name))?;

        let (file_path, _source) =
            ar_edit_core::playback::resolve_source_path(&shot.source, &state.project_dir)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

        let (start_ms, end_ms) = match &shot.range {
            ar_edit_core::models::ShotRange::Time { from_ms, to_ms } => (*from_ms, Some(*to_ms)),
            ar_edit_core::models::ShotRange::Words { .. }
            | ar_edit_core::models::ShotRange::Scenes { .. } => (0, None),
        };

        let play_req = ar_edit_core::playback::PlayRequest {
            file: file_path,
            start_ms,
            end_ms,
            ipc_socket: None,
            source_id: None,
            mpv_script: None,
            marker_file: None,
        };

        ar_edit_core::playback::launch_player(&player, &play_req)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
    } else if let Some(source_id) = &req.target {
        // Direct source playback from the beginning
        let (file_path, _source) =
            ar_edit_core::playback::resolve_source_path(source_id, &state.project_dir)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

        let play_req = ar_edit_core::playback::PlayRequest {
            file: file_path,
            start_ms: 0,
            end_ms: None,
            ipc_socket: None,
            source_id: None,
            mpv_script: None,
            marker_file: None,
        };

        ar_edit_core::playback::launch_player(&player, &play_req)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
    } else {
        return Err(AppError(anyhow::anyhow!(
            "must provide either 'target' (source id) or both 'edit' and 'shot'"
        )));
    }

    Ok(Json(serde_json::json!({ "status": "playing" })))
}
