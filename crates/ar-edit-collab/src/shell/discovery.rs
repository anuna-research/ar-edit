//! Serverless peer discovery via phrase-keyed [[pkarr]] / Mainline DHT
//! (SPEC-003 REQ-068/069/071, ADR-013, CON-017; task s8).
//!
//! The pairing phrase deterministically derives an Ed25519 [`pkarr::Keypair`].
//! The host publishes a signed pkarr record (a DNS TXT under `_ar-edit-pair`)
//! advertising its [`iroh`] [`EndpointAddr`] under that key; a joiner derives
//! the same key, resolves the record, and dials the host directly. SPAKE2 then
//! runs over that connection ([`crate::pairing`]). No rendezvous server.
//!
//! Pattern adapted from `../did-crdt` (ADR-006 / CON-006), changed for iroh 1.0
//! and keyed by the *low-entropy phrase* — see ADR-013 for the enumerability
//! trade-off and its SPAKE2 + TTL + burn mitigations.

use crate::recognise::phrase::Phrase;
use iroh::{EndpointAddr, EndpointId, RelayUrl};
use pkarr::dns::{self, rdata::RData, ResourceRecord};
use pkarr::{Keypair, SignedPacket};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// CON-017 record name.
const RECORD_NAME: &str = "_ar-edit-pair";
/// Record TTL — kept near the pairing-phrase TTL (REQ-068, ~10 min).
const RECORD_TTL: u32 = 600;
/// Freshness window for a resolved discovery packet (REQ-068 replay guard). A
/// signed record whose authored timestamp is older than this is treated as
/// not-found: a relay must not be able to replay a host's expired record after
/// its pairing phrase has lapsed. Matches the record TTL.
const PAIRING_FRESHNESS_SECS: u64 = RECORD_TTL as u64;
/// Domain separation for the phrase → discovery-key derivation (ADR-013).
const KDF_DOMAIN: &[u8] = b"ar-edit/pair/discovery/v1";
/// DNS character strings cap at 255 bytes; a few dialling hints is plenty.
const MAX_ADDRS: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("dns record build: {0}")]
    Dns(String),
    #[error("pkarr: {0}")]
    Pkarr(String),
}

/// Derive the deterministic pkarr keypair for a pairing phrase (ADR-013).
/// Not secret — anyone with the phrase can reproduce it; the [[SPAKE2]]
/// handshake (not key secrecy) is what protects the session.
pub fn derive_keypair(phrase: &Phrase) -> Keypair {
    let rendered = phrase.render();
    let seed = blake3::hash(&[KDF_DOMAIN, rendered.as_bytes()].concat());
    Keypair::from_secret_key(seed.as_bytes())
}

/// Build the signed CON-017 discovery record advertising `addr`.
pub fn build_record(
    keypair: &Keypair,
    addr: &EndpointAddr,
) -> Result<SignedPacket, DiscoveryError> {
    let nid_string = format!("nid={}", addr.id);
    let addrs: Vec<String> = addr
        .ip_addrs()
        .take(MAX_ADDRS)
        .map(|a| a.to_string())
        .collect();
    let addrs_string = (!addrs.is_empty()).then(|| format!("addrs={}", addrs.join(",")));
    let relay_string = addr.relay_urls().next().map(|u| format!("relay={u}"));

    let mut txt = dns::rdata::TXT::new();
    let dns_err = |e: dns::SimpleDnsError| DiscoveryError::Dns(e.to_string());
    txt.add_string("v=1").map_err(dns_err)?;
    txt.add_string(&nid_string).map_err(dns_err)?;
    if let Some(ref s) = relay_string {
        txt.add_string(s).map_err(dns_err)?;
    }
    if let Some(ref s) = addrs_string {
        txt.add_string(s).map_err(dns_err)?;
    }

    let mut packet = dns::Packet::new_reply(0);
    packet.answers.push(ResourceRecord::new(
        dns::Name::new(RECORD_NAME).expect("static record name"),
        dns::CLASS::IN,
        RECORD_TTL,
        RData::TXT(txt),
    ));
    SignedPacket::from_packet(keypair, &packet).map_err(|e| DiscoveryError::Pkarr(e.to_string()))
}

/// Recognise a CON-017 record into a dialable [`EndpointAddr`]. Returns `None`
/// if the record is malformed or lacks a valid `nid`. The `addrs`/`relay`
/// fields are unauthenticated dialling hints — a forged hint costs only a
/// failed iroh handshake (which authenticates the NodeId).
pub fn parse_record(packet: &SignedPacket) -> Option<EndpointAddr> {
    for rr in packet.resource_records(RECORD_NAME) {
        let RData::TXT(ref txt) = rr.rdata else {
            continue;
        };
        let attrs = txt.attributes();
        if attrs.get("v").and_then(|v| v.as_deref()) != Some("1") {
            continue;
        }
        let Some(Some(nid)) = attrs.get("nid") else {
            continue;
        };
        let Ok(id) = nid.parse::<EndpointId>() else {
            continue;
        };
        let mut ea = EndpointAddr::new(id);
        if let Some(Some(list)) = attrs.get("addrs") {
            for s in list.split(',') {
                if let Ok(sa) = s.parse::<std::net::SocketAddr>() {
                    ea = ea.with_ip_addr(sa);
                }
            }
        }
        // Restore the relay path so a host reachable only via a relay (WAN/NAT)
        // is still dialable after discovery.
        if let Some(Some(relay)) = attrs.get("relay") {
            if let Ok(url) = relay.parse::<RelayUrl>() {
                ea = ea.with_relay_url(url);
            }
        }
        return Some(ea);
    }
    None
}

/// Where discovery records live. `InProcess` is an in-memory stand-in for the
/// DHT (hermetic tests); `Http` publishes to / resolves from a pkarr relay
/// backed by the [[Mainline DHT]].
pub enum Discovery {
    InProcess(Arc<Mutex<HashMap<[u8; 32], SignedPacket>>>),
    Http(pkarr::PkarrRelayClientAsync),
}

impl Discovery {
    /// In-memory backend (tests / single-process). Both peers share the `Arc`.
    pub fn in_process(store: Arc<Mutex<HashMap<[u8; 32], SignedPacket>>>) -> Self {
        Discovery::InProcess(store)
    }

    /// Production backend over a pkarr HTTP relay (e.g. `https://relay.pkarr.org`).
    pub fn http(relay_url: &str) -> Result<Self, DiscoveryError> {
        let settings = pkarr::RelaySettings {
            relays: vec![relay_url.to_owned()],
            ..pkarr::RelaySettings::default()
        };
        let client = pkarr::PkarrRelayClient::new(settings)
            .map_err(|e| DiscoveryError::Pkarr(e.to_string()))?
            .as_async();
        Ok(Discovery::Http(client))
    }

    /// Publish this host's `addr` under the phrase-derived key (REQ-068).
    pub async fn publish(
        &self,
        phrase: &Phrase,
        addr: &EndpointAddr,
    ) -> Result<(), DiscoveryError> {
        let keypair = derive_keypair(phrase);
        let packet = build_record(&keypair, addr)?;
        match self {
            Discovery::InProcess(store) => {
                store
                    .lock()
                    .unwrap()
                    .insert(keypair.public_key().to_bytes(), packet);
                Ok(())
            }
            Discovery::Http(client) => client
                .publish(&packet)
                .await
                .map_err(|e| DiscoveryError::Pkarr(e.to_string())),
        }
    }

    /// Resolve the host's [`EndpointAddr`] for `phrase` (REQ-069). `Ok(None)`
    /// means no record found — or a found record that is stale (outside the
    /// pairing freshness window), which is rejected so a relay cannot replay an
    /// expired host record after the phrase has lapsed.
    pub async fn lookup(&self, phrase: &Phrase) -> Result<Option<EndpointAddr>, DiscoveryError> {
        let keypair = derive_keypair(phrase);
        let public_key = keypair.public_key();
        let packet = match self {
            Discovery::InProcess(store) => {
                store.lock().unwrap().get(&public_key.to_bytes()).cloned()
            }
            Discovery::Http(client) => client
                .resolve(&public_key)
                .await
                .map_err(|e| DiscoveryError::Pkarr(e.to_string()))?,
        };
        Ok(packet
            .as_ref()
            .filter(|p| is_fresh(p))
            .and_then(parse_record))
    }
}

/// Whether a signed discovery packet's authored timestamp is within the pairing
/// freshness window (replay guard, REQ-068). `SignedPacket::timestamp()` is in
/// microseconds since the UNIX epoch. A timestamp in the future (clock skew) is
/// accepted; only records authored too far in the past are rejected.
fn is_fresh(packet: &SignedPacket) -> bool {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now_us = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);
    let authored_us = packet.timestamp();
    let age_us = now_us.saturating_sub(authored_us);
    age_us <= PAIRING_FRESHNESS_SECS.saturating_mul(1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recognise::phrase;

    /// Regression (P2): a relay URL written into the record must be restored on
    /// parse, so a relay-only (WAN/NAT) host stays dialable after discovery.
    #[test]
    fn record_preserves_relay_url() {
        let kp = derive_keypair(&phrase::generate_secure());
        let relay: RelayUrl = "https://relay.example.com".parse().expect("relay url");
        let id = iroh::SecretKey::generate().public();
        let addr = EndpointAddr::new(id).with_relay_url(relay.clone());

        let packet = build_record(&kp, &addr).expect("build record");
        let parsed = parse_record(&packet).expect("record parses");
        assert!(
            parsed.relay_urls().any(|u| *u == relay),
            "relay url must survive the record round-trip"
        );
    }
}
