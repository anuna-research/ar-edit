//! Collaboration CLI tests (SPEC-003 CON-012; task s6).

use assert_cmd::Command;

/// SPEC-003 TEST-078: a malformed pairing phrase is rejected with exit code 1
/// before any network action (the recogniser runs first).
#[test]
fn pair_rejects_malformed_phrase() {
    Command::cargo_bin("ar-edit")
        .unwrap()
        .args(["pair", "not-a-valid-phrase-zzz"])
        .assert()
        .failure()
        .code(1);
}

/// `ar-edit share` generates a pairing phrase and exits successfully.
#[test]
fn share_emits_a_phrase() {
    let out = Command::cargo_bin("ar-edit")
        .unwrap()
        .args(["share", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("\"phrase\""), "share --json must emit a phrase: {s}");
}

/// A well-formed phrase is accepted by the recogniser (exit 0). It will not
/// establish a live session without the reviewed transport build, but the
/// phrase surface itself is valid.
#[test]
fn pair_accepts_wellformed_phrase() {
    // Get a valid phrase from `share --json`, then feed it to `pair`.
    let out = Command::cargo_bin("ar-edit")
        .unwrap()
        .args(["share", "--json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let phrase = v["phrase"].as_str().unwrap();
    Command::cargo_bin("ar-edit")
        .unwrap()
        .args(["pair", phrase])
        .assert()
        .success();
}
