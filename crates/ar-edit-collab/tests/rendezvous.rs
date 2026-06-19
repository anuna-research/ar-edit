//! Rendezvous relay integration test (SPEC-003 CON-014, REQ-071; task s7).
//! Loopback TCP, hermetic. Empty without the `rendezvous` feature.
#![cfg(feature = "rendezvous")]

use ar_edit_collab::recognise::wire::{self, RendezvousFrame};
use ar_edit_collab::shell::rendezvous::RendezvousServer;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn read_framed(s: &mut TcpStream) -> Vec<u8> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).await.unwrap();
    let n = u32::from_be_bytes(len) as usize;
    let mut body = vec![0u8; n];
    s.read_exact(&mut body).await.unwrap();
    let mut framed = len.to_vec();
    framed.extend_from_slice(&body);
    framed
}

fn pake(payload: &[u8]) -> Vec<u8> {
    let mut b = vec![0x03u8];
    b.extend_from_slice(payload);
    wire::frame(&b)
}

fn ticket(payload: &[u8]) -> Vec<u8> {
    let mut b = vec![0x04u8];
    b.extend_from_slice(payload);
    wire::frame(&b)
}

/// Two peers bind the same channel; the relay forwards their opaque PAKE and
/// ticket frames bidirectionally without interpreting them (REQ-071).
#[tokio::test]
async fn relays_opaque_frames_between_paired_peers() {
    let server = RendezvousServer::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr();
    tokio::spawn(server.run());

    let mut a = TcpStream::connect(addr).await.unwrap();
    let mut b = TcpStream::connect(addr).await.unwrap();

    // Both BIND channel 7.
    let bind7 = wire::frame(&[0x01, 0x00, 0x07]);
    a.write_all(&bind7).await.unwrap();
    b.write_all(&bind7).await.unwrap();

    // A -> B: an opaque SPAKE2 message.
    a.write_all(&pake(b"spake2-msg-A")).await.unwrap();
    let got = read_framed(&mut b).await;
    assert_eq!(
        wire::parse_rendezvous(&got),
        Ok(RendezvousFrame::PakeMsg(b"spake2-msg-A")),
        "B must receive A's opaque PAKE frame verbatim"
    );

    // B -> A: an opaque node ticket.
    b.write_all(&ticket(b"node-ticket-B")).await.unwrap();
    let got2 = read_framed(&mut a).await;
    assert_eq!(
        wire::parse_rendezvous(&got2),
        Ok(RendezvousFrame::Ticket(b"node-ticket-B")),
        "A must receive B's opaque ticket frame verbatim"
    );
}

/// Regression (P2): a peer that binds a channel and disconnects before a
/// partner arrives must free the channel, so the next two peers pair with each
/// other instead of a dead waiter.
#[tokio::test]
async fn waiter_freed_on_disconnect() {
    let server = RendezvousServer::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr();
    tokio::spawn(server.run());

    // Peer 1 binds channel 7, then disconnects before any partner arrives.
    {
        let mut p1 = TcpStream::connect(addr).await.unwrap();
        p1.write_all(&wire::frame(&[0x01, 0x00, 0x07])).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await; // let server register it
    } // p1 dropped → disconnects
    tokio::time::sleep(Duration::from_millis(100)).await; // let server free channel 7

    // Two fresh peers on channel 7 must pair with each other.
    let mut a = TcpStream::connect(addr).await.unwrap();
    let mut b = TcpStream::connect(addr).await.unwrap();
    a.write_all(&wire::frame(&[0x01, 0x00, 0x07])).await.unwrap();
    b.write_all(&wire::frame(&[0x01, 0x00, 0x07])).await.unwrap();
    a.write_all(&pake(b"hello")).await.unwrap();

    let got = tokio::time::timeout(Duration::from_secs(2), read_framed(&mut b))
        .await
        .expect("B must receive A's frame — the dead waiter was freed");
    assert_eq!(
        wire::parse_rendezvous(&got),
        Ok(RendezvousFrame::PakeMsg(b"hello"))
    );
}

/// Regression (P2): after BIND, a frame that fails the CON-014 recogniser
/// (unknown tag) is dropped fail-closed and the connection is torn down — the
/// relay must not become an arbitrary framed-data tunnel.
#[tokio::test]
async fn unrecognised_frame_after_bind_is_dropped() {
    let server = RendezvousServer::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr();
    tokio::spawn(server.run());

    let mut a = TcpStream::connect(addr).await.unwrap();
    let mut b = TcpStream::connect(addr).await.unwrap();
    let bind7 = wire::frame(&[0x01, 0x00, 0x07]);
    a.write_all(&bind7).await.unwrap();
    b.write_all(&bind7).await.unwrap();

    // Well-formed length prefix, but tag 0x7f is not a recognised rendezvous
    // frame — the relay must drop it (stop forwarding), never tunnel it.
    a.write_all(&wire::frame(&[0x7f, 0xde, 0xad])).await.unwrap();

    // B must receive nothing: the unrecognised frame is not relayed.
    let mut buf = [0u8; 16];
    let r = tokio::time::timeout(Duration::from_millis(300), b.read(&mut buf)).await;
    let relayed = matches!(r, Ok(Ok(n)) if n > 0);
    assert!(!relayed, "an unrecognised frame must not be relayed");

    // And a *recognised* frame sent on the same channel afterwards is also not
    // forwarded (the relay stopped reading A's malformed stream — fail closed).
    a.write_all(&pake(b"after-bad")).await.unwrap();
    let r2 = tokio::time::timeout(Duration::from_millis(300), b.read(&mut buf)).await;
    let relayed2 = matches!(r2, Ok(Ok(n)) if n > 0);
    assert!(!relayed2, "relay must stop forwarding after a malformed frame");
}

/// Peers on different channels are not paired (no cross-talk).
#[tokio::test]
async fn different_channels_are_isolated() {
    let server = RendezvousServer::bind("127.0.0.1:0").await.unwrap();
    let addr = server.local_addr();
    tokio::spawn(server.run());

    let mut a = TcpStream::connect(addr).await.unwrap();
    let mut b = TcpStream::connect(addr).await.unwrap();
    a.write_all(&wire::frame(&[0x01, 0x00, 0x07])).await.unwrap(); // channel 7
    b.write_all(&wire::frame(&[0x01, 0x00, 0x09])).await.unwrap(); // channel 9
    a.write_all(&pake(b"hello")).await.unwrap();

    // B should receive nothing within a short window.
    let mut buf = [0u8; 16];
    let r = tokio::time::timeout(std::time::Duration::from_millis(200), b.read(&mut buf)).await;
    assert!(r.is_err(), "peers on different channels must not be paired");
}
