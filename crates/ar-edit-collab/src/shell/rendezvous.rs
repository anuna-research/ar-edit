//! Rendezvous relay (SPEC-003 CON-014, REQ-071; task s7).
//!
//! A meeting point that matches two peers on a channel and relays their
//! length-prefixed frames **opaquely** — the server never interprets the
//! SPAKE2 (`PakeMsg`) or `Ticket` payloads, learns no session key, and sees no
//! project data (REQ-071). Frames are recognised with the [`crate::recognise::wire`]
//! CON-014 grammar and the [`MAX_FRAME`] cap is enforced on every read.
//!
//! Transport here is plain TCP; once peers have exchanged tickets they hand off
//! to a direct iroh connection (Phase S transport task). The relay is
//! deliberately transport-agnostic and crypto-free, which is why it is safe to
//! implement and test without the no-go cryptographic review that gates the
//! live SPAKE2 task.

use crate::recognise::wire::{parse_rendezvous, RendezvousFrame, MAX_FRAME};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

type Inbound = mpsc::UnboundedSender<Vec<u8>>;

struct Pending {
    /// Sink that delivers frames *to* the already-waiting peer.
    peer_tx: Inbound,
    /// Used by the second peer to hand the waiting peer its own sink.
    notify: oneshot::Sender<Inbound>,
}

#[derive(Clone, Default)]
struct State {
    channels: Arc<Mutex<HashMap<u16, Pending>>>,
}

/// A bound rendezvous server. Call [`RendezvousServer::run`] to serve.
pub struct RendezvousServer {
    listener: TcpListener,
    local_addr: SocketAddr,
    state: State,
}

impl RendezvousServer {
    /// Bind the relay (use `127.0.0.1:0` for an ephemeral test port).
    pub async fn bind(addr: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        Ok(Self {
            listener,
            local_addr,
            state: State::default(),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Accept connections forever, relaying paired peers.
    pub async fn run(self) {
        loop {
            match self.listener.accept().await {
                Ok((stream, _)) => {
                    let state = self.state.clone();
                    tokio::spawn(async move {
                        let _ = handle_conn(stream, state).await;
                    });
                }
                Err(_) => break,
            }
        }
    }
}

/// Read one `[u32 len][len bytes]` frame, enforcing [`MAX_FRAME`]. Returns the
/// whole framed message (prefix included) for opaque relay, or `None` on EOF.
async fn read_frame<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame exceeds MAX_FRAME",
        ));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    let mut framed = Vec::with_capacity(4 + len);
    framed.extend_from_slice(&len_buf);
    framed.extend_from_slice(&body);
    Ok(Some(framed))
}

async fn handle_conn(stream: TcpStream, state: State) -> std::io::Result<()> {
    let (mut read_half, mut write_half) = stream.into_split();

    // First frame MUST be BIND(channel) — full recognition before matching.
    let first = match read_frame(&mut read_half).await? {
        Some(f) => f,
        None => return Ok(()),
    };
    let channel = match parse_rendezvous(&first) {
        Ok(RendezvousFrame::Bind(c)) => c,
        _ => return Ok(()), // not a bind → drop (fail closed)
    };

    // Frames destined TO this peer.
    let (my_tx, mut my_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // Decide the pairing under the lock (no await held), then await outside it.
    enum Pairing {
        Ready(Inbound),
        Wait(oneshot::Receiver<Inbound>),
    }
    let pairing = {
        let mut map = state.channels.lock().unwrap();
        match map.remove(&channel) {
            Some(pending) => {
                // Second peer: hand the waiter our sink, take theirs.
                let _ = pending.notify.send(my_tx.clone());
                Pairing::Ready(pending.peer_tx)
            }
            None => {
                let (notify_tx, notify_rx) = oneshot::channel();
                map.insert(
                    channel,
                    Pending {
                        peer_tx: my_tx.clone(),
                        notify: notify_tx,
                    },
                );
                Pairing::Wait(notify_rx)
            }
        }
    };
    let other_tx: Inbound = match pairing {
        Pairing::Ready(tx) => tx,
        Pairing::Wait(mut rx) => {
            // Wait for a partner while still servicing this peer's socket: buffer
            // any frames it sends early, and — crucially — detect its disconnect
            // (EOF) so the channel is freed instead of leaving a dead waiter the
            // next peer would be matched to.
            let mut buffered: Vec<Vec<u8>> = Vec::new();
            let tx = loop {
                tokio::select! {
                    biased;
                    res = &mut rx => {
                        break match res {
                            Ok(tx) => tx,
                            Err(_) => {
                                state.channels.lock().unwrap().remove(&channel);
                                return Ok(());
                            }
                        };
                    }
                    frame = read_frame(&mut read_half) => match frame {
                        Ok(Some(f)) => buffered.push(f), // relay once paired
                        _ => {
                            // EOF or error before a partner arrived: free channel.
                            state.channels.lock().unwrap().remove(&channel);
                            return Ok(());
                        }
                    },
                }
            };
            // Flush anything the waiter sent before its partner arrived.
            for f in buffered {
                if tx.send(f).is_err() {
                    return Ok(());
                }
            }
            tx
        }
    };

    // Writer: deliver inbound frames to this peer's socket.
    let writer = tokio::spawn(async move {
        while let Some(frame) = my_rx.recv().await {
            if write_half.write_all(&frame).await.is_err() {
                break;
            }
        }
    });

    // Reader: relay this peer's frames to the other peer, opaquely.
    while let Some(frame) = read_frame(&mut read_half).await? {
        if other_tx.send(frame).is_err() {
            break; // other side gone
        }
    }

    drop(other_tx);
    let _ = writer.await;
    Ok(())
}
