//! Session daemon + local IPC (SPEC-003 REQ-089/090, CON-018, ADR-014; task s9).
//!
//! A long-running process that owns the live [`CollabDoc`] so discrete one-shot
//! CLI/agent invocations can participate in a single live session. Clients
//! attach over a Unix-domain socket and exchange length-prefixed JSON frames
//! ([`Request`]/[`Response`]); every request is fully recognised before any
//! mutation (LangSec / CON-018). Local mutations and (under the `transport`
//! feature) remote CRDT deltas apply to the same in-memory document, so they
//! converge by the CRDT itself.

use crate::crdt::CollabDoc;
use crate::materialise::materialise;
use ar_edit_core::models::{Shot, ShotNote, ShotRange};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

/// Frame length cap (CON-018): reject larger declared frames without buffering.
pub const MAX_FRAME: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("io: {0}")]
    Io(String),
    #[error("malformed request: {0}")]
    Parse(String),
    #[error("frame exceeds {MAX_FRAME}-byte cap")]
    TooLong,
    #[error("connection closed")]
    Closed,
    #[error("a session daemon is already running on this socket")]
    AlreadyRunning,
}

/// A client request (CON-018). Externally tagged on `op`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Attach to the session for `edit` (informational; the daemon owns one doc).
    Attach { edit: String },
    AddShot { shot: Shot },
    MoveShot { shot_id: String, to: usize },
    TrimShot { shot_id: String, range: ShotRange },
    AddNote { shot_id: String, note: ShotNote },
    RemoveShot { shot_id: String },
    /// Read the materialised shot list.
    Snapshot,
    /// Lightweight status.
    Status,
}

/// A daemon response (CON-018). Externally tagged on `kind`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Ok,
    /// A live insert succeeded; carries the daemon-minted, actor-scoped shot id.
    Added { shot_id: String },
    Snapshot { shots: Vec<Shot> },
    Status { shot_count: usize },
    Error { message: String },
}

/// Recognise a framed JSON request body into a typed [`Request`] (CON-018,
/// fail-closed: a malformed body never reaches the document).
pub fn parse_request(body: &[u8]) -> Result<Request, DaemonError> {
    serde_json::from_slice(body).map_err(|e| DaemonError::Parse(e.to_string()))
}

fn apply(doc: &CollabDoc, req: Request) -> Response {
    match req {
        Request::Attach { .. } => Response::Ok,
        Request::AddShot { shot } => {
            // Mint a fresh actor-scoped id rather than preserving the client's
            // (possibly sequential) id: two peer daemons handed the same id would
            // each keep it as "locally free", then collide on merge. add_new_shot
            // guarantees a globally-unique id (REQ-080).
            let shot_id = doc.add_new_shot(&shot.source, &shot.range);
            for note in &shot.notes {
                doc.add_note(&shot_id, note);
            }
            Response::Added { shot_id }
        }
        Request::MoveShot { shot_id, to } => {
            doc.move_shot(&shot_id, to);
            Response::Ok
        }
        Request::TrimShot { shot_id, range } => {
            doc.trim_shot(&shot_id, &range);
            Response::Ok
        }
        Request::AddNote { shot_id, note } => {
            doc.add_note(&shot_id, &note);
            Response::Ok
        }
        Request::RemoveShot { shot_id } => {
            doc.remove_shot(&shot_id);
            Response::Ok
        }
        Request::Snapshot => Response::Snapshot {
            shots: materialise(doc).shots,
        },
        Request::Status => Response::Status {
            shot_count: materialise(doc).shots.len(),
        },
    }
}

/// The session daemon: owns the live document and serves IPC clients.
pub struct Daemon {
    doc: Arc<Mutex<CollabDoc>>,
    listener: UnixListener,
    path: PathBuf,
}

impl Daemon {
    /// Bind the daemon's IPC socket (mode 0600) and take ownership of `doc`.
    pub fn bind(socket_path: impl AsRef<Path>, doc: CollabDoc) -> Result<Self, DaemonError> {
        let path = socket_path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Only remove the socket after proving it is stale: if a live daemon is
        // already listening, refuse rather than unlink its socket (which would
        // orphan it and split the session).
        if path.exists() {
            match std::os::unix::net::UnixStream::connect(&path) {
                Ok(_) => return Err(DaemonError::AlreadyRunning),
                Err(_) => {
                    let _ = std::fs::remove_file(&path); // stale socket — safe to clear
                }
            }
        }
        let listener = UnixListener::bind(&path).map_err(|e| DaemonError::Io(e.to_string()))?;
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        Ok(Self {
            doc: Arc::new(Mutex::new(doc)),
            listener,
            path,
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.path
    }

    /// Shared handle to the live document (so transport/sync can apply remote
    /// deltas into the same doc the IPC clients mutate).
    pub fn doc(&self) -> Arc<Mutex<CollabDoc>> {
        self.doc.clone()
    }

    /// Serve clients until the listener closes.
    pub async fn run(self) {
        loop {
            match self.listener.accept().await {
                Ok((stream, _)) => {
                    let doc = self.doc.clone();
                    tokio::spawn(handle_client(stream, doc));
                }
                Err(_) => break,
            }
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn handle_client(mut stream: UnixStream, doc: Arc<Mutex<CollabDoc>>) {
    loop {
        let body = match read_frame(&mut stream).await {
            Ok(Some(b)) => b,
            _ => break,
        };
        // Recognise, then apply under the lock (no await held).
        let resp = match parse_request(&body) {
            Ok(req) => {
                let guard = doc.lock().unwrap();
                apply(&guard, req)
            }
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        };
        let bytes = serde_json::to_vec(&resp).unwrap_or_default();
        if write_frame(&mut stream, &bytes).await.is_err() {
            break;
        }
    }
}

async fn read_frame(s: &mut UnixStream) -> Result<Option<Vec<u8>>, DaemonError> {
    let mut len = [0u8; 4];
    match s.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(DaemonError::Io(e.to_string())),
    }
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(DaemonError::TooLong);
    }
    let mut body = vec![0u8; n];
    s.read_exact(&mut body)
        .await
        .map_err(|e| DaemonError::Io(e.to_string()))?;
    Ok(Some(body))
}

async fn write_frame(s: &mut UnixStream, body: &[u8]) -> Result<(), DaemonError> {
    s.write_all(&(body.len() as u32).to_be_bytes())
        .await
        .map_err(|e| DaemonError::Io(e.to_string()))?;
    s.write_all(body)
        .await
        .map_err(|e| DaemonError::Io(e.to_string()))?;
    Ok(())
}

/// Client used by discrete CLI/agent commands to attach to a running daemon
/// (REQ-090). When [`DaemonClient::connect`] fails, the caller falls back to a
/// one-shot on-disk operation.
pub struct DaemonClient {
    stream: UnixStream,
}

impl DaemonClient {
    pub async fn connect(socket_path: impl AsRef<Path>) -> Result<Self, DaemonError> {
        let stream = UnixStream::connect(socket_path.as_ref())
            .await
            .map_err(|e| DaemonError::Io(e.to_string()))?;
        Ok(Self { stream })
    }

    /// Is a daemon reachable at this socket path?
    pub async fn is_running(socket_path: impl AsRef<Path>) -> bool {
        UnixStream::connect(socket_path.as_ref()).await.is_ok()
    }

    pub async fn request(&mut self, req: &Request) -> Result<Response, DaemonError> {
        let bytes = serde_json::to_vec(req).map_err(|e| DaemonError::Parse(e.to_string()))?;
        write_frame(&mut self.stream, &bytes).await?;
        let body = read_frame(&mut self.stream)
            .await?
            .ok_or(DaemonError::Closed)?;
        serde_json::from_slice(&body).map_err(|e| DaemonError::Parse(e.to_string()))
    }
}
