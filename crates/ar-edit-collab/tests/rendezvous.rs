//! Rendezvous relay integration test (SPEC-003 CON-014, REQ-071; task s7).
//! Loopback TCP, hermetic. Empty without the `rendezvous` feature.
#![cfg(feature = "rendezvous")]

use ar_edit_collab::recognise::wire::{self, RendezvousFrame};
use ar_edit_collab::shell::rendezvous::RendezvousServer;
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
