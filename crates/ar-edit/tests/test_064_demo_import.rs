//! TEST-122..127 — `ar-edit demo import` (SPEC-004): an ar-crawl demo bundle
//! becomes a silent source with a narration transcript, per-step scenes,
//! markers and POIs in the annotation store, and the cursor log preserved.
//! Needs ffmpeg/ffprobe (the screencast is probed and thumbnailed); skipped
//! when they are unavailable, like the rest of the media-backed suite.
#![allow(deprecated)] // assert_cmd::cargo_bin — matches the rest of the suite

use assert_cmd::Command;
use std::fs;
use std::path::Path;
use std::process::Command as Proc;
use tempfile::TempDir;

use ar_edit_collab::annotations::AnnotationStore;
use ar_edit_collab::ids::ActorId;
use ar_edit_core::models::{Manifest, PoiCategory, SourceIndex, Transcript};

fn has_fftools() -> bool {
    ["ffmpeg", "ffprobe"].iter().all(|t| {
        Proc::new(t)
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// A 3-second silent 320x240 screencast stand-in (mp4 so libvpx isn't needed).
fn create_silent_video(path: &Path) -> bool {
    Proc::new("ffmpeg")
        .args([
            "-f",
            "lavfi",
            "-i",
            "color=black:s=320x240:d=3",
            "-c:v",
            "libx264",
            "-y",
        ])
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Bundle as ar-crawl writes it: goto / marker / click / failed hover / cursor.
fn write_bundle(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    assert!(
        create_silent_video(&dir.join("video.mp4")),
        "ffmpeg fixture"
    );
    fs::write(
        dir.join("manifest.json"),
        r##"{"version":1,"video":"video.mp4","cursor":"cursor.json","recording":"recording.json",
            "startedAt":"2026-09-18T10:00:00.000Z","viewport":{"width":1280,"height":720},
            "scale":2,"profile":"demo","durationMs":3000,
            "steps":[
              {"index":0,"type":"goto","title":"Open the dashboard","url":"https://app.local",
               "startMs":0,"endMs":1000,"success":true},
              {"index":1,"type":"marker","title":"Sorted by activity","startMs":1000,"endMs":1000,"success":true},
              {"index":2,"type":"click","title":"Create it","selector":"#b","startMs":1500,"endMs":1800,"success":true},
              {"index":3,"type":"hover","selector":"#missing","startMs":2000,"endMs":2100,"success":false},
              {"index":4,"type":"cursor","startMs":2900,"endMs":2900,"success":true}
            ]}"##,
    )
    .unwrap();
    fs::write(
        dir.join("cursor.json"),
        r##"{"coordinateSpace":"viewport-css-px","scale":2,"events":[
            {"tMs":1520,"type":"move","x":366,"y":135,"transitionMs":0,"style":"pointer"},
            {"tMs":1790,"type":"ripple","x":366,"y":135,"size":100},
            {"tMs":2900,"type":"hide"}]}"##,
    )
    .unwrap();
    fs::write(dir.join("recording.json"), r#"{"title":"t","steps":[]}"#).unwrap();
}

fn ar(project: &Path) -> Command {
    let mut c = Command::cargo_bin("ar-edit").unwrap();
    c.current_dir(project);
    c.env("AR_EDIT_AUTHOR", "crawler");
    c
}

fn init_project(tmp: &Path) -> std::path::PathBuf {
    let out = Command::cargo_bin("ar-edit")
        .unwrap()
        .current_dir(tmp)
        .args(["init", "proj"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "init: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    tmp.join("proj")
}

fn store(project: &Path) -> AnnotationStore {
    let bytes = fs::read(project.join("annotations/src-001.annot.json")).unwrap();
    AnnotationStore::from_bytes(&bytes, ActorId(1)).unwrap()
}

/// TEST-122..127 — the whole import in one pass (each layer asserted).
#[test]
fn demo_import_populates_every_layer() {
    if !has_fftools() {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let project = init_project(tmp.path());
    let bundle = tmp.path().join("bundle");
    write_bundle(&bundle);

    let out = ar(&project)
        .args(["--json", "demo", "import"])
        .arg(&bundle)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let summary: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(summary["source_id"], "src-001");
    assert_eq!(summary["silent"], true);
    assert_eq!(summary["segments"], 3);
    assert_eq!(summary["words"], 8);
    assert_eq!(summary["scenes"], 3);
    assert_eq!(summary["markers"], 3);
    assert_eq!(summary["pois"], 4);
    assert_eq!(summary["cursor_events"], 3);

    // TEST-123: registered as a silent source, flagged transcribed + indexed.
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(project.join("manifest.json")).unwrap()).unwrap();
    let src = &manifest.sources[0];
    assert_eq!(src.audio_channels, 0);
    assert!(!src.has_audio());
    assert!(src.transcribed && src.indexed);
    assert!(project.join(&src.path).exists());

    // TEST-124: narration transcript; spans run to the next titled step.
    let t: Transcript = serde_json::from_slice(
        &fs::read(project.join("transcripts/src-001.transcript.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(t.model, "ar-crawl-demo");
    let texts: Vec<&str> = t.segments.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(
        texts,
        ["Open the dashboard", "Sorted by activity", "Create it"]
    );
    assert_eq!((t.segments[1].start_ms, t.segments[1].end_ms), (1000, 1500));
    assert_eq!(t.segments[2].end_ms, 3000, "last narration runs to the end");
    assert_eq!(t.word_count, 8);

    // TEST-125: scenes partition at visual steps (goto, click, hover), each
    // with a thumbnail on disk; marker/cursor never start a scene.
    let idx: SourceIndex =
        serde_json::from_slice(&fs::read(project.join("index/src-001.index.json")).unwrap())
            .unwrap();
    let bounds: Vec<(u64, u64)> = idx.scenes.iter().map(|s| (s.start_ms, s.end_ms)).collect();
    assert_eq!(bounds, [(0, 1500), (1500, 2000), (2000, 3000)]);
    assert_eq!(
        idx.scenes[0].description.as_deref(),
        Some("Open the dashboard")
    );
    assert_eq!(idx.scenes[2].description, None);
    for s in &idx.scenes {
        assert!(project.join(&s.thumbnail).exists(), "thumbnail for {s:?}");
    }

    // TEST-126: markers per narration, POIs per step with mapped categories;
    // the failed hover is an `issue`. All attributed to the local author.
    let store = store(&project);
    let markers = store.markers();
    let labels: Vec<&str> = markers.iter().map(|m| m.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Open the dashboard", "Sorted by activity", "Create it"]
    );
    assert_eq!(markers[2].note.as_deref(), Some("click #b"));
    assert!(markers.iter().all(|m| m.author == "crawler"));
    let pois = store.pois();
    let cats: Vec<PoiCategory> = pois.iter().map(|p| p.category).collect();
    assert_eq!(
        cats,
        [
            PoiCategory::Transition,
            PoiCategory::Note,
            PoiCategory::Cue,
            PoiCategory::Issue
        ]
    );
    assert_eq!(pois[3].note.as_deref(), Some("hover #missing"));

    // TEST-127: cursor log preserved verbatim.
    let cursor: serde_json::Value =
        serde_json::from_slice(&fs::read(project.join("annotations/src-001.cursor.json")).unwrap())
            .unwrap();
    assert_eq!(cursor["scale"], 2.0);
    assert_eq!(cursor["events"].as_array().unwrap().len(), 3);
    assert_eq!(cursor["events"][0]["style"], "pointer");

    // The derived layers drive ordinary editing: a word-range shot resolves.
    let out = ar(&project)
        .args(["edit", "create", "cut"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = ar(&project)
        .args([
            "edit",
            "add-segment",
            "cut",
            "--source",
            "src-001",
            "--from-word",
            "6",
            "--to-word",
            "7",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "word-range shot on the narration transcript: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// TEST-122 — a directory without manifest.json is refused before the project
/// is touched.
#[test]
fn demo_import_rejects_non_bundle() {
    let tmp = TempDir::new().unwrap();
    let project = init_project(tmp.path());
    let not_a_bundle = tmp.path().join("nope");
    fs::create_dir_all(&not_a_bundle).unwrap();

    let out = ar(&project)
        .args(["demo", "import"])
        .arg(&not_a_bundle)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not a demo bundle"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(project.join("manifest.json")).unwrap()).unwrap();
    assert!(manifest.sources.is_empty(), "project untouched");
}

/// Dry run reports the plan and writes nothing.
#[test]
fn demo_import_dry_run_writes_nothing() {
    if !has_fftools() {
        eprintln!("SKIPPED: ffmpeg/ffprobe not available");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let project = init_project(tmp.path());
    let bundle = tmp.path().join("bundle");
    write_bundle(&bundle);

    let out = ar(&project)
        .args(["--dry-run", "demo", "import"])
        .arg(&bundle)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Would import"));
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(project.join("manifest.json")).unwrap()).unwrap();
    assert!(manifest.sources.is_empty());
    assert!(!project.join("transcripts/src-001.transcript.json").exists());
}
