//! Session daemon + local IPC (SPEC-003 REQ-089/090, CON-018, ADR-014; task s9).
//!
//! A long-running process that **owns the canonical edit store** ([`PersistentEdit`],
//! ADR-011) for one edit, so discrete one-shot CLI/agent invocations can
//! participate in a single live session. Clients attach over a Unix-domain
//! socket and exchange length-prefixed JSON frames ([`Request`]/[`Response`]);
//! every request is fully recognised before any mutation (LangSec / CON-018).
//!
//! Single store: the daemon applies each mutation to the `PersistentEdit` and
//! persists it back to the edit file. There is no separate daemon CRDT to
//! reconcile with disk — the daemon *is* the store while it is live, so a
//! one-shot command that attaches and the file never diverge (OQ-8). Undo/redo
//! use the store's durable oplog cursor; while live, that revert is persisted
//! like any other change.

use crate::store::PersistentEdit;
use ar_edit_core::models::{Shot, ShotRange};
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
///
/// Mutations carry no operation id and undo/redo carry no tag: the daemon owns a
/// single store, so there is no second stack to coordinate (the review #5–#8
/// op-id/gating apparatus is retired). Undo/redo are the store's durable cursor.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Attach to the session (informational; the daemon owns one edit).
    Attach {
        edit: String,
    },
    AddShot {
        shot: Shot,
    },
    MoveShot {
        shot_id: String,
        to: usize,
    },
    TrimShot {
        shot_id: String,
        range: ShotRange,
    },
    AddNote {
        shot_id: String,
        text: String,
        author: String,
    },
    RemoveShot {
        shot_id: String,
    },
    /// Undo the most recent change via the store's durable cursor (REQ-086).
    Undo,
    /// Redo the most recently undone change.
    Redo,
    /// Read the materialised shot list.
    Snapshot,
    /// Lightweight status (shot count + edit name).
    Status,
}

/// A daemon response (CON-018). Externally tagged on `kind`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Ok,
    /// A live insert succeeded; carries the minted, actor-scoped shot id.
    Added {
        shot_id: String,
    },
    /// Whether an undo/redo actually reverted (nothing to revert → `false`).
    Reverted {
        reverted: bool,
    },
    Snapshot {
        shots: Vec<Shot>,
    },
    Status {
        shot_count: usize,
        edit: String,
    },
    Error {
        message: String,
    },
}

/// Recognise a framed JSON request body into a typed [`Request`] (CON-018,
/// fail-closed: a malformed body never reaches the document).
pub fn parse_request(body: &[u8]) -> Result<Request, DaemonError> {
    serde_json::from_slice(body).map_err(|e| DaemonError::Parse(e.to_string()))
}

/// Apply a recognised request to the owned store and return the response.
fn apply(store: &mut PersistentEdit, req: Request) -> Response {
    match req {
        Request::Attach { .. } => Response::Ok,
        Request::AddShot { shot } => {
            match store.add_shot(&shot.source, shot.range.clone(), None, &shot.author) {
                Ok(shot_id) => {
                    for note in &shot.notes {
                        store.add_note(&shot_id, note, None);
                    }
                    Response::Added { shot_id }
                }
                Err(e) => Response::Error {
                    message: e.to_string(),
                },
            }
        }
        Request::MoveShot { shot_id, to } => {
            store.move_shot(&shot_id, to, None);
            Response::Ok
        }
        Request::TrimShot { shot_id, range } => match store.trim_shot(&shot_id, range, None) {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error {
                message: e.to_string(),
            },
        },
        Request::AddNote {
            shot_id,
            text,
            author,
        } => {
            if !store.has_shot(&shot_id) {
                return Response::Error {
                    message: format!("shot '{shot_id}' not found"),
                };
            }
            store.add_note_text(&shot_id, &text, None, &author);
            Response::Ok
        }
        Request::RemoveShot { shot_id } => {
            store.remove_shot(&shot_id, None);
            Response::Ok
        }
        Request::Undo => Response::Reverted {
            reverted: store.undo(),
        },
        Request::Redo => Response::Reverted {
            reverted: store.redo(),
        },
        Request::Snapshot => Response::Snapshot {
            shots: store.snapshot().shots,
        },
        Request::Status => Response::Status {
            shot_count: store.snapshot().shots.len(),
            edit: store.name().to_string(),
        },
    }
}

/// The session daemon: owns the canonical edit store and serves IPC clients.
pub struct Daemon {
    store: Arc<Mutex<PersistentEdit>>,
    /// The canonical edit file the store is persisted to after each mutation.
    edit_path: PathBuf,
    listener: UnixListener,
    path: PathBuf,
}

impl Daemon {
    /// Bind the IPC socket (mode 0600) and take ownership of `store`, persisting
    /// it to `edit_path` after every mutation.
    pub fn bind(
        socket_path: impl AsRef<Path>,
        edit_path: impl Into<PathBuf>,
        store: PersistentEdit,
    ) -> Result<Self, DaemonError> {
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
            store: Arc::new(Mutex::new(store)),
            edit_path: edit_path.into(),
            listener,
            path,
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.path
    }

    /// Shared handle to the live store, so a future transport/sync task can apply
    /// remote edits into the same store the IPC clients mutate.
    pub fn store(&self) -> Arc<Mutex<PersistentEdit>> {
        self.store.clone()
    }

    /// Serve clients until the listener closes.
    pub async fn run(self) {
        // `Daemon` implements Drop, so fields can't be moved out of `self`.
        let edit_path = Arc::new(self.edit_path.clone());
        loop {
            match self.listener.accept().await {
                Ok((stream, _)) => {
                    let store = self.store.clone();
                    tokio::spawn(handle_client(stream, store, edit_path.clone()));
                }
                Err(_) => break,
            }
        }
    }
}

/// Persist the store to the canonical edit file via a temp file + rename, so a
/// crash mid-write never leaves a truncated edit. Best-effort.
fn persist_store(store: &PersistentEdit, path: &Path) {
    let bytes = store.to_bytes();
    let tmp = path.with_extension("edit.tmp");
    if std::fs::write(&tmp, &bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn handle_client(
    mut stream: UnixStream,
    store: Arc<Mutex<PersistentEdit>>,
    edit_path: Arc<PathBuf>,
) {
    loop {
        let body = match read_frame(&mut stream).await {
            Ok(Some(b)) => b,
            _ => break,
        };
        // Recognise, then apply under the lock (no await held).
        let resp = match parse_request(&body) {
            Ok(req) => {
                let mut guard = store.lock().unwrap();
                // Persist iff the document actually changed (the CRDT version
                // advanced). This covers reads, validation errors, no-op
                // move/remove/trim of an absent shot, and refused undo/redo —
                // none persist — so a client can't drive disk churn with no-ops.
                let before = guard.frontier();
                let resp = apply(&mut guard, req);
                if guard.frontier() != before {
                    persist_store(&guard, &edit_path);
                }
                resp
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

    /// Round-trip a request, bounded by [`request_timeout`]. A host that accepts
    /// the socket but stalls (or sends a partial frame) must not hang the caller
    /// forever — an expiry is a hard error so callers can fail closed (REQ-090).
    pub async fn request(&mut self, req: &Request) -> Result<Response, DaemonError> {
        match tokio::time::timeout(request_timeout(), self.request_inner(req)).await {
            Ok(result) => result,
            Err(_) => Err(DaemonError::Io(
                "daemon did not respond within the timeout".into(),
            )),
        }
    }

    async fn request_inner(&mut self, req: &Request) -> Result<Response, DaemonError> {
        let bytes = serde_json::to_vec(req).map_err(|e| DaemonError::Parse(e.to_string()))?;
        write_frame(&mut self.stream, &bytes).await?;
        let body = read_frame(&mut self.stream)
            .await?
            .ok_or(DaemonError::Closed)?;
        serde_json::from_slice(&body).map_err(|e| DaemonError::Parse(e.to_string()))
    }
}

/// Deadline for a client round-trip (default 5s; override with
/// `AR_EDIT_DAEMON_TIMEOUT_MS`, mainly for tests).
fn request_timeout() -> std::time::Duration {
    std::env::var("AR_EDIT_DAEMON_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_secs(5))
}
