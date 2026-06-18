//! OQ-1 de-risking spike for SPEC-003 (Realtime Collaborative Editing).
//! See BRIEF.md. Runs three experiments; prints a structured report and
//! panics on any correctness-assertion failure (nonzero exit == spike fail).

use std::time::Instant;

fn main() {
    println!("== OQ-1 SPIKE REPORT ==");
    println!("model: claude-opus-4-8[1m]  date: 2026-06-18\n");
    let a = exp_a_loro();
    let b = exp_b_spake2();
    let c = exp_c_blake3();
    println!("\n== SUMMARY ==");
    println!("H-a Loro convergence (move/move, move/field): {}", a);
    println!("H-b SPAKE2 handshake feasibility:             {}", b);
    println!("H-c BLAKE3 2GB content-addressing throughput: {}", c);
}

// ---------------------------------------------------------------------------
// Experiment A — Loro MovableList convergence (ADR-007 / REQ-080,081,083)
// ---------------------------------------------------------------------------
//
// Model: shot ORDER is a MovableList<shot-id>; each shot's RANGE lives in a
// Map keyed by shot-id. This matches a plausible SPEC-003 implementation and
// lets us test: (1) identity-preserving concurrent MOVE of the same shot,
// (2) MOVE concurrent with a FIELD edit on the same shot, (3) order-independent
// merge (SEC). Equality is checked via Loro's whole-document deep value.

use loro::{ExportMode, LoroDoc, LoroValue};

fn new_replica(peer: u64) -> LoroDoc {
    let d = LoroDoc::new();
    d.set_peer_id(peer).unwrap();
    d
}

fn deep(d: &LoroDoc) -> LoroValue {
    d.get_deep_value()
}

/// Build a base edit doc: shots [s1,s2,s3] with ranges, return its snapshot.
fn base_snapshot() -> Vec<u8> {
    let d = new_replica(1);
    let list = d.get_movable_list("shots");
    for (i, id) in ["s1", "s2", "s3"].iter().enumerate() {
        list.insert(i, *id).unwrap();
    }
    let ranges = d.get_map("ranges");
    ranges.insert("s1", "0..52").unwrap();
    ranges.insert("s2", "200..280").unwrap();
    ranges.insert("s3", "0..2").unwrap();
    d.commit();
    d.export(ExportMode::Snapshot).unwrap()
}

fn shot_order(d: &LoroDoc) -> Vec<String> {
    let list = d.get_movable_list("shots");
    (0..list.len())
        .map(|i| match list.get(i).unwrap() {
            loro::ValueOrContainer::Value(LoroValue::String(s)) => s.to_string(),
            other => format!("{other:?}"),
        })
        .collect()
}

fn exp_a_loro() -> &'static str {
    println!("--- Experiment A: Loro MovableList convergence ---");
    let base = base_snapshot();

    // --- Case 1: concurrent MOVE of the SAME shot by two peers --------------
    // A moves s3 (idx2) to front; B moves s3 to middle. Must converge to ONE
    // position on both, with s3 appearing exactly once (REQ-080).
    let a = new_replica(10);
    a.import(&base).unwrap();
    let b = new_replica(20);
    b.import(&base).unwrap();

    a.get_movable_list("shots").mov(2, 0).unwrap();
    a.commit();
    b.get_movable_list("shots").mov(2, 1).unwrap();
    b.commit();

    // exchange full state, both directions, different orders
    let a_up = a.export(ExportMode::Snapshot).unwrap();
    let b_up = b.export(ExportMode::Snapshot).unwrap();
    a.import(&b_up).unwrap();
    b.import(&a_up).unwrap();
    a.commit();
    b.commit();

    let oa = shot_order(&a);
    let ob = shot_order(&b);
    println!("  case1 concurrent move-same: A={:?} B={:?}", oa, ob);
    assert_eq!(oa, ob, "case1: replicas diverged on concurrent move");
    let count_s3 = oa.iter().filter(|x| *x == "s3").count();
    assert_eq!(count_s3, 1, "case1: s3 duplicated or lost ({count_s3})");
    assert_eq!(oa.len(), 3, "case1: shot count changed: {:?}", oa);

    // --- Case 2: MOVE concurrent with FIELD edit on the same shot ----------
    // A moves s2 to front; B trims s2's range. Both must survive on the SAME
    // identity (REQ-080 + REQ-081).
    let a = new_replica(11);
    a.import(&base).unwrap();
    let b = new_replica(21);
    b.import(&base).unwrap();

    a.get_movable_list("shots").mov(1, 0).unwrap(); // move s2 to front
    a.commit();
    b.get_map("ranges").insert("s2", "210..265").unwrap(); // trim s2
    b.commit();

    let a_up = a.export(ExportMode::Snapshot).unwrap();
    let b_up = b.export(ExportMode::Snapshot).unwrap();
    b.import(&a_up).unwrap();
    a.import(&b_up).unwrap();
    a.commit();
    b.commit();

    assert_eq!(deep(&a), deep(&b), "case2: replicas diverged");
    let order = shot_order(&a);
    let s2_range = a.get_map("ranges").get("s2");
    println!("  case2 move+trim: order={:?} s2_range={:?}", order, s2_range);
    assert_eq!(order[0], "s2", "case2: move lost");
    let rng = format!("{:?}", s2_range);
    assert!(rng.contains("210..265"), "case2: field edit lost: {rng}");

    // --- Case 3: SEC — order-independent convergence over 3 peers ----------
    // Three peers each make a distinct concurrent change; deliver the three
    // updates to two fresh observers in DIFFERENT orders; both must converge.
    let p1 = new_replica(31);
    p1.import(&base).unwrap();
    let p2 = new_replica(32);
    p2.import(&base).unwrap();
    let p3 = new_replica(33);
    p3.import(&base).unwrap();

    p1.get_movable_list("shots").insert(3, "s4").unwrap(); // insert
    p1.commit();
    p2.get_movable_list("shots").mov(0, 2).unwrap(); // move s1 back
    p2.commit();
    p3.get_map("ranges").insert("s3", "5..9").unwrap(); // edit
    p3.commit();

    let u1 = p1.export(ExportMode::Snapshot).unwrap();
    let u2 = p2.export(ExportMode::Snapshot).unwrap();
    let u3 = p3.export(ExportMode::Snapshot).unwrap();

    let obs_x = new_replica(40);
    obs_x.import(&base).unwrap();
    let obs_y = new_replica(41);
    obs_y.import(&base).unwrap();

    // X: order 1,2,3 ; Y: order 3,1,2
    for u in [&u1, &u2, &u3] {
        obs_x.import(u).unwrap();
    }
    for u in [&u3, &u1, &u2] {
        obs_y.import(u).unwrap();
    }
    obs_x.commit();
    obs_y.commit();

    println!(
        "  case3 SEC: X={:?} Y={:?}",
        shot_order(&obs_x),
        shot_order(&obs_y)
    );
    assert_eq!(
        deep(&obs_x),
        deep(&obs_y),
        "case3: SEC violated — delivery order changed final state"
    );
    assert!(shot_order(&obs_x).contains(&"s4".to_string()), "case3: insert lost");

    println!("  => PASS: identity-preserving moves + SEC hold");
    "PASS"
}

// ---------------------------------------------------------------------------
// Experiment B — SPAKE2 handshake feasibility (ADR-009 / REQ-070, NFR-010/014)
// ---------------------------------------------------------------------------

use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

/// One full symmetric SPAKE2 handshake; returns (key_a, key_b).
fn spake2_once(phrase_a: &[u8], phrase_b: &[u8], app_id: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let (s_a, msg_a) =
        Spake2::<Ed25519Group>::start_symmetric(&Password::new(phrase_a), &Identity::new(app_id));
    let (s_b, msg_b) =
        Spake2::<Ed25519Group>::start_symmetric(&Password::new(phrase_b), &Identity::new(app_id));
    let key_a = s_a.finish(&msg_b).unwrap_or_default();
    let key_b = s_b.finish(&msg_a).unwrap_or_default();
    (key_a, key_b)
}

fn exp_b_spake2() -> &'static str {
    println!("\n--- Experiment B: SPAKE2 handshake feasibility ---");
    let app = b"ar-edit/spake2/v1";

    // Correctness: same phrase -> equal keys; different phrase -> unequal.
    let (ka, kb) = spake2_once(b"7-saturn-pioneer", b"7-saturn-pioneer", app);
    assert!(!ka.is_empty() && ka == kb, "B: matching phrases did not agree");
    let (wa, wb) = spake2_once(b"7-saturn-pioneer", b"7-saturn-wrongone", app);
    assert!(wa != wb, "B: mismatched phrases must NOT agree on a key");
    println!(
        "  key agreement: match=>equal({} bytes), mismatch=>unequal  OK",
        ka.len()
    );

    // Compute cost: time N full handshakes.
    let n = 200;
    let t = Instant::now();
    for _ in 0..n {
        let _ = spake2_once(b"7-saturn-pioneer", b"7-saturn-pioneer", app);
    }
    let per = t.elapsed().as_secs_f64() / n as f64 * 1000.0;
    println!("  full-handshake compute: {:.3} ms/handshake (n={n})", per);

    // Rendezvous-relay floor: loopback TCP round-trip (one small frame each way).
    let rtt_ms = loopback_rtt_ms(500);
    println!("  loopback TCP RTT floor (relay hop): {:.4} ms median", rtt_ms);

    let verdict = if per < 100.0 { "PASS" } else { "REVIEW" };
    println!(
        "  => {}: crypto is {:.3} ms; NFR-010 budget is 5000 ms (network-bound, not crypto-bound)",
        verdict, per
    );
    verdict
}

fn loopback_rtt_ms(iters: usize) -> f64 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = [0u8; 32];
        for _ in 0..iters {
            s.read_exact(&mut buf).unwrap();
            s.write_all(&buf).unwrap();
        }
    });
    let mut c = TcpStream::connect(addr).unwrap();
    c.set_nodelay(true).unwrap();
    let buf = [0u8; 32];
    let mut samples = Vec::with_capacity(iters);
    let mut rbuf = [0u8; 32];
    for _ in 0..iters {
        let t = Instant::now();
        c.write_all(&buf).unwrap();
        c.read_exact(&mut rbuf).unwrap();
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    drop(c);
    server.join().unwrap();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    samples[samples.len() / 2]
}

// ---------------------------------------------------------------------------
// Experiment C — BLAKE3 content-addressing throughput (ADR-010 / REQ-075,076)
// ---------------------------------------------------------------------------

fn exp_c_blake3() -> &'static str {
    println!("\n--- Experiment C: BLAKE3 2GB content-addressing (in-RAM) ---");
    let size: usize = 2 * 1024 * 1024 * 1024; // 2 GiB, no disk
    println!("  allocating {} MiB buffer in RAM...", size / 1024 / 1024);
    // Pseudo-random-ish fill so the optimiser can't elide; cheap LCG.
    let mut data = vec![0u8; size];
    let mut x: u32 = 0x9e3779b9;
    for chunk in data.chunks_mut(4096) {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        chunk[0] = (x >> 24) as u8;
        if chunk.len() > 1 {
            chunk[chunk.len() - 1] = x as u8;
        }
    }

    let t = Instant::now();
    let hash = blake3::hash(&data);
    let secs = t.elapsed().as_secs_f64();
    let gbps = (size as f64 / 1e9) / secs;
    println!(
        "  single-thread: {:.3} s  -> {:.2} GB/s  (hash={}...)",
        secs,
        gbps,
        &hash.to_hex()[..16]
    );

    // Verified-streaming sanity: re-hash in 1 MiB chunks via Hasher, compare.
    let t2 = Instant::now();
    let mut hasher = blake3::Hasher::new();
    for chunk in data.chunks(1024 * 1024) {
        hasher.update(chunk);
    }
    let streamed = hasher.finalize();
    let secs2 = t2.elapsed().as_secs_f64();
    assert_eq!(hash, streamed, "C: streamed hash != one-shot hash");
    println!(
        "  streamed (1MiB chunks): {:.3} s -> {:.2} GB/s  (matches one-shot OK)",
        secs2,
        (size as f64 / 1e9) / secs2
    );

    let verdict = if gbps >= 1.0 { "PASS" } else { "REVIEW" };
    println!(
        "  => {}: a 2GB source hashes in {:.2}s; far above typical link bandwidth, so BLAKE3 is not the sync bottleneck",
        verdict, secs
    );
    verdict
}
