//! Demo bundle import (SPEC-004).
//!
//! An `ar-crawl session --record` bundle is a silent screencast plus the
//! structured data an editor needs: per-step timings with narration titles
//! (`manifest.json`) and a cursor event log (`cursor.json`). This module turns
//! that data into ar-edit's own layers — transcript, scene boundaries, marker
//! and POI specs — so a recorded demo is editable as text like any other
//! source. The only I/O here is reading the bundle; writing into the project is
//! the CLI's job (CON-019).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::index::SceneBoundary;
use crate::models::{PoiCategory, Transcript, TranscriptSegment, Word};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum DemoError {
    #[error("not a demo bundle: {0} (missing manifest.json)")]
    NotABundle(PathBuf),
    #[error("bundle video not found: {0}")]
    VideoMissing(PathBuf),
    #[error("unsupported bundle version {0} (expected 1)")]
    UnsupportedVersion(u32),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse bundle JSON: {0}")]
    Json(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// Bundle format (as written by ar-crawl; camelCase on disk)
// ---------------------------------------------------------------------------

/// `manifest.json` — step timings, narration and capture parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoManifest {
    pub version: u32,
    /// Video file, relative to the bundle directory.
    pub video: String,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub recording: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    pub viewport: Viewport,
    /// Device scale factor: video pixels = viewport CSS px × scale.
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default)]
    pub profile: Option<String>,
    pub duration_ms: u64,
    pub steps: Vec<DemoStep>,
}

fn default_scale() -> f64 {
    1.0
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

/// One recorded action. `start_ms`/`end_ms` are milliseconds from video start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoStep {
    pub index: u32,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(default = "default_true")]
    pub success: bool,
    #[serde(default)]
    pub pause: Option<u64>,
}

fn default_true() -> bool {
    true
}

/// `cursor.json` — the cursor event log, kept verbatim for the compositor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorLog {
    #[serde(default)]
    pub coordinate_space: Option<String>,
    #[serde(default = "default_scale")]
    pub scale: f64,
    pub events: Vec<CursorEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorEvent {
    pub t_ms: u64,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
}

/// A loaded bundle: manifest, optional cursor log, and the resolved video path.
#[derive(Debug, Clone)]
pub struct DemoBundle {
    pub dir: PathBuf,
    pub manifest: DemoManifest,
    pub cursor: Option<CursorLog>,
    pub video_path: PathBuf,
}

/// Read a bundle directory (REQ-092). The cursor log is optional
/// (`--no-cursor` bundles); the video is not.
pub fn load_bundle(dir: &Path) -> Result<DemoBundle, DemoError> {
    let manifest_path = dir.join("manifest.json");
    if !manifest_path.is_file() {
        return Err(DemoError::NotABundle(dir.to_path_buf()));
    }
    let manifest: DemoManifest = serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
    if manifest.version != 1 {
        return Err(DemoError::UnsupportedVersion(manifest.version));
    }

    let video_path = dir.join(&manifest.video);
    if !video_path.is_file() {
        return Err(DemoError::VideoMissing(video_path));
    }

    let cursor = match &manifest.cursor {
        Some(name) if dir.join(name).is_file() => {
            Some(serde_json::from_slice(&std::fs::read(dir.join(name))?)?)
        }
        _ => None,
    };

    Ok(DemoBundle {
        dir: dir.to_path_buf(),
        manifest,
        cursor,
        video_path,
    })
}

// ---------------------------------------------------------------------------
// Derivations
// ---------------------------------------------------------------------------

/// Steps that leave no trace on screen: they never start a scene.
fn is_visual(step: &DemoStep) -> bool {
    !matches!(step.kind.as_str(), "cursor" | "marker")
}

/// A titled step and the span its narration covers.
#[derive(Debug, Clone, PartialEq)]
pub struct Narration {
    pub step_index: u32,
    pub title: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Narration spans (REQ-094): a titled step's narration runs from its start to
/// the start of the *next titled* step (or the end of the video). `end_ms` in
/// the manifest is when the action finished; the hold after it — where the
/// viewer looks at the result — belongs to the same line. Untitled actions in
/// between are absorbed, so the transcript covers the whole timeline and
/// zero-duration `marker` steps get a real span.
pub fn narration_spans(manifest: &DemoManifest) -> Vec<Narration> {
    let titled: Vec<&DemoStep> = manifest
        .steps
        .iter()
        .filter(|s| s.title.as_deref().is_some_and(|t| !t.trim().is_empty()))
        .collect();

    let mut out = Vec::with_capacity(titled.len());
    for (i, step) in titled.iter().enumerate() {
        let end_ms = titled
            .get(i + 1)
            .map(|n| n.start_ms)
            .unwrap_or(manifest.duration_ms)
            .max(step.start_ms);
        if end_ms == step.start_ms {
            continue;
        }
        out.push(Narration {
            step_index: step.index,
            title: step.title.clone().unwrap_or_default().trim().to_string(),
            start_ms: step.start_ms,
            end_ms,
        });
    }
    out
}

/// Build a transcript from the narration (REQ-094): one segment per titled
/// step, words spread evenly across the span so word ranges resolve.
pub fn build_transcript(manifest: &DemoManifest, source_id: &str) -> Transcript {
    let mut segments = Vec::new();
    let mut word_index: u32 = 0;

    for (seg_index, n) in narration_spans(manifest).iter().enumerate() {
        let tokens: Vec<&str> = n.title.split_whitespace().collect();
        let span = n.end_ms - n.start_ms;
        let count = tokens.len() as u64;
        let mut words = Vec::with_capacity(tokens.len());
        for (i, tok) in tokens.iter().enumerate() {
            let i = i as u64;
            let start_ms = n.start_ms + span * i / count;
            let end_ms = if i + 1 == count {
                n.end_ms
            } else {
                n.start_ms + span * (i + 1) / count
            };
            words.push(Word {
                index: word_index,
                text: (*tok).to_string(),
                start_ms,
                end_ms,
                confidence: 1.0,
            });
            word_index += 1;
        }
        segments.push(TranscriptSegment {
            index: seg_index as u32,
            start_ms: n.start_ms,
            end_ms: n.end_ms,
            text: n.title.clone(),
            words,
        });
    }

    Transcript {
        source_id: source_id.to_string(),
        model: "ar-crawl-demo".to_string(),
        language: "en".to_string(),
        duration_ms: manifest.duration_ms,
        word_count: word_index,
        segments,
    }
}

/// Scene boundaries (REQ-095): what is *shown* changes at each visual step,
/// so scenes partition the timeline at those starts. A leading gap before the
/// first action is its own untitled scene.
pub fn scene_boundaries(manifest: &DemoManifest) -> Vec<SceneBoundary> {
    let mut visual: Vec<&DemoStep> = manifest.steps.iter().filter(|s| is_visual(s)).collect();
    visual.sort_by_key(|s| s.start_ms);

    let mut scenes: Vec<SceneBoundary> = Vec::new();
    if let Some(first) = visual.first() {
        if first.start_ms > 0 {
            scenes.push(SceneBoundary {
                start_ms: 0,
                end_ms: first.start_ms,
                description: None,
            });
        }
    }
    for (i, step) in visual.iter().enumerate() {
        let end_ms = visual
            .get(i + 1)
            .map(|n| n.start_ms)
            .unwrap_or(manifest.duration_ms);
        if end_ms <= step.start_ms {
            continue;
        }
        scenes.push(SceneBoundary {
            start_ms: step.start_ms,
            end_ms,
            description: step.title.clone(),
        });
    }
    if scenes.is_empty() && manifest.duration_ms > 0 {
        scenes.push(SceneBoundary {
            start_ms: 0,
            end_ms: manifest.duration_ms,
            description: None,
        });
    }
    scenes
}

/// A marker to create: the narration span, labelled with its title.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerSpec {
    pub start_ms: u64,
    pub end_ms: u64,
    pub label: String,
    pub note: Option<String>,
}

/// One marker per narration span (REQ-096), so `ar-edit markers` navigates
/// the demo chapter by chapter. The note records the action behind it.
pub fn marker_specs(manifest: &DemoManifest) -> Vec<MarkerSpec> {
    narration_spans(manifest)
        .into_iter()
        .map(|n| {
            let note = manifest
                .steps
                .iter()
                .find(|s| s.index == n.step_index)
                .and_then(action_note);
            MarkerSpec {
                start_ms: n.start_ms,
                end_ms: n.end_ms,
                label: n.title,
                note,
            }
        })
        .collect()
}

/// A POI to create at a step's start.
#[derive(Debug, Clone, PartialEq)]
pub struct PoiSpec {
    pub at_ms: u64,
    pub category: PoiCategory,
    pub note: Option<String>,
}

/// Map a step type onto the POI vocabulary (REQ-096). A failed step is an
/// `issue` whatever its type — that's the moment to cut around.
pub fn poi_category(step: &DemoStep) -> PoiCategory {
    if !step.success {
        return PoiCategory::Issue;
    }
    match step.kind.as_str() {
        "goto" | "navigate" | "goBack" | "goForward" | "reload" => PoiCategory::Transition,
        "marker" => PoiCategory::Note,
        "click" | "dblclick" | "doubleClick" | "press" | "keyDown" | "keyUp" | "check"
        | "uncheck" | "selectOption" | "type" | "fill" | "change" | "clear" | "insertText" => {
            PoiCategory::Cue
        }
        "hover" | "scroll" | "scrollIntoView" | "zoom" => PoiCategory::Highlight,
        _ => PoiCategory::Note,
    }
}

/// One POI per step (REQ-096), skipping cursor visibility toggles.
pub fn poi_specs(manifest: &DemoManifest) -> Vec<PoiSpec> {
    manifest
        .steps
        .iter()
        .filter(|s| s.kind != "cursor")
        .map(|s| PoiSpec {
            at_ms: s.start_ms,
            category: poi_category(s),
            note: s
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .or_else(|| action_note(s)),
        })
        .collect()
}

/// "click #submit", "goto https://…", or just the action type.
fn action_note(step: &DemoStep) -> Option<String> {
    let target = step.selector.as_deref().or(step.url.as_deref());
    Some(match target {
        Some(t) => format!("{} {}", step.kind, t),
        None => step.kind.clone(),
    })
}

/// Path of the preserved cursor log inside a project (REQ-097).
pub fn cursor_log_path(project_dir: &Path, source_id: &str) -> PathBuf {
    project_dir
        .join("annotations")
        .join(format!("{source_id}.cursor.json"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn step(index: u32, kind: &str, title: Option<&str>, start: u64, end: u64) -> DemoStep {
        DemoStep {
            index,
            kind: kind.into(),
            title: title.map(String::from),
            selector: None,
            url: None,
            start_ms: start,
            end_ms: end,
            success: true,
            pause: None,
        }
    }

    fn manifest(steps: Vec<DemoStep>, duration_ms: u64) -> DemoManifest {
        DemoManifest {
            version: 1,
            video: "video.webm".into(),
            cursor: Some("cursor.json".into()),
            recording: None,
            started_at: None,
            viewport: Viewport {
                width: 1280,
                height: 720,
            },
            scale: 2.0,
            profile: Some("demo".into()),
            duration_ms,
            steps,
        }
    }

    // -- narration spans (REQ-094) -------------------------------------------

    #[test]
    fn narration_runs_to_the_next_titled_step_not_action_end() {
        let m = manifest(
            vec![
                step(0, "goto", Some("Open the dashboard"), 0, 1000),
                step(1, "marker", Some("Sorted by activity"), 1000, 1000),
                step(2, "click", None, 1500, 1800), // untitled: absorbed
                step(3, "type", Some("Name it"), 2000, 3300),
            ],
            5000,
        );
        let spans = narration_spans(&m);
        assert_eq!(spans.len(), 3);
        assert_eq!((spans[0].start_ms, spans[0].end_ms), (0, 1000));
        // zero-duration marker gets a real span, covering the untitled click
        assert_eq!((spans[1].start_ms, spans[1].end_ms), (1000, 2000));
        // last narration runs to the end of the video
        assert_eq!((spans[2].start_ms, spans[2].end_ms), (2000, 5000));
    }

    #[test]
    fn blank_titles_are_not_narration() {
        let m = manifest(vec![step(0, "goto", Some("  "), 0, 500)], 1000);
        assert!(narration_spans(&m).is_empty());
    }

    // -- transcript ------------------------------------------------------------

    #[test]
    fn transcript_words_tile_each_span_with_global_indices() {
        let m = manifest(
            vec![
                step(0, "goto", Some("Open the dashboard"), 0, 900),
                step(1, "click", Some("Create it"), 3000, 3400),
            ],
            4000,
        );
        let t = build_transcript(&m, "src-007");
        assert_eq!(t.source_id, "src-007");
        assert_eq!(t.model, "ar-crawl-demo");
        assert_eq!(t.duration_ms, 4000);
        assert_eq!(t.word_count, 5);
        assert_eq!(t.segments.len(), 2);

        let w = &t.segments[0].words;
        assert_eq!(
            w.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
            ["Open", "the", "dashboard"]
        );
        assert_eq!((w[0].start_ms, w[0].end_ms), (0, 1000));
        assert_eq!((w[2].start_ms, w[2].end_ms), (2000, 3000));
        // words are contiguous and cover the span exactly
        assert_eq!(w[1].start_ms, w[0].end_ms);
        assert_eq!(t.segments[1].words[0].index, 3);
        assert_eq!(t.segments[1].words[1].end_ms, 4000);
    }

    // -- scenes (REQ-095) ------------------------------------------------------

    #[test]
    fn scenes_partition_at_visual_steps_only() {
        let m = manifest(
            vec![
                step(0, "goto", Some("Open"), 200, 1000),
                step(1, "marker", Some("Narration"), 1000, 1000),
                step(2, "cursor", None, 1200, 1200),
                step(3, "click", Some("Create"), 2000, 2300),
            ],
            4000,
        );
        let s = scene_boundaries(&m);
        assert_eq!(s.len(), 3, "{s:?}");
        // leading gap before the first action
        assert_eq!(
            (s[0].start_ms, s[0].end_ms, s[0].description.clone()),
            (0, 200, None)
        );
        assert_eq!((s[1].start_ms, s[1].end_ms), (200, 2000));
        assert_eq!(s[1].description.as_deref(), Some("Open"));
        assert_eq!((s[2].start_ms, s[2].end_ms), (2000, 4000));
    }

    #[test]
    fn no_visual_steps_yields_one_scene() {
        let m = manifest(vec![step(0, "marker", Some("Only words"), 0, 0)], 3000);
        let s = scene_boundaries(&m);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].start_ms, s[0].end_ms), (0, 3000));
    }

    // -- markers & POIs (REQ-096) ----------------------------------------------

    #[test]
    fn markers_follow_narration_and_note_the_action() {
        let mut click = step(1, "click", Some("Create it"), 1000, 1300);
        click.selector = Some("#b".into());
        let m = manifest(vec![step(0, "goto", Some("Open"), 0, 900), click], 2000);
        let specs = marker_specs(&m);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].label, "Open");
        assert_eq!((specs[0].start_ms, specs[0].end_ms), (0, 1000));
        assert_eq!(specs[1].note.as_deref(), Some("click #b"));
    }

    #[test]
    fn poi_categories_map_from_step_type_and_failure() {
        let ok = |k: &str| step(0, k, None, 0, 1);
        assert_eq!(poi_category(&ok("goto")), PoiCategory::Transition);
        assert_eq!(poi_category(&ok("click")), PoiCategory::Cue);
        assert_eq!(poi_category(&ok("type")), PoiCategory::Cue);
        assert_eq!(poi_category(&ok("hover")), PoiCategory::Highlight);
        assert_eq!(poi_category(&ok("marker")), PoiCategory::Note);
        assert_eq!(poi_category(&ok("setViewport")), PoiCategory::Note);
        let mut failed = ok("click");
        failed.success = false;
        assert_eq!(poi_category(&failed), PoiCategory::Issue);
    }

    #[test]
    fn poi_specs_skip_cursor_toggles_and_prefer_titles() {
        let m = manifest(
            vec![
                step(0, "goto", Some("Open"), 0, 900),
                step(1, "cursor", None, 900, 900),
                step(2, "click", None, 1000, 1200),
            ],
            2000,
        );
        let p = poi_specs(&m);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].note.as_deref(), Some("Open"));
        assert_eq!(p[1].note.as_deref(), Some("click"));
        assert_eq!(p[1].at_ms, 1000);
    }

    // -- bundle parsing --------------------------------------------------------

    #[test]
    fn manifest_and_cursor_parse_from_ar_crawl_json() {
        let m: DemoManifest = serde_json::from_str(
            r#"{"version":1,"video":"video.webm","cursor":"cursor.json","recording":"recording.json",
                "startedAt":"2026-09-18T10:00:00.000Z","viewport":{"width":1280,"height":720},
                "scale":2,"profile":"demo","durationMs":4210,
                "steps":[{"index":0,"type":"goto","title":"Open","startMs":50,"endMs":1069,"success":true,
                          "url":"https://x"},
                         {"index":1,"type":"cursor","startMs":4206,"endMs":4206,"success":true}]}"#,
        )
        .unwrap();
        assert_eq!(m.scale, 2.0);
        assert_eq!(m.steps[0].kind, "goto");
        assert_eq!(m.steps[0].url.as_deref(), Some("https://x"));
        assert!(m.steps[1].title.is_none());

        let c: CursorLog = serde_json::from_str(
            r#"{"coordinateSpace":"viewport-css-px","scale":2,"events":[
                {"tMs":1631,"type":"move","x":215,"y":135,"transitionMs":0,"style":"text"},
                {"tMs":3301,"type":"ripple","x":366,"y":135,"size":100},
                {"tMs":4206,"type":"hide"}]}"#,
        )
        .unwrap();
        assert_eq!(c.events.len(), 3);
        assert_eq!(c.events[0].style.as_deref(), Some("text"));
        assert_eq!(c.events[2].x, None);
        // round-trips without inventing fields
        let back = serde_json::to_value(&c.events[2]).unwrap();
        assert_eq!(back, serde_json::json!({"tMs": 4206, "type": "hide"}));
    }

    #[test]
    fn load_bundle_rejects_missing_pieces() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(matches!(
            load_bundle(tmp.path()),
            Err(DemoError::NotABundle(_))
        ));

        std::fs::write(
            tmp.path().join("manifest.json"),
            serde_json::to_string(&manifest(vec![], 100)).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            load_bundle(tmp.path()),
            Err(DemoError::VideoMissing(_))
        ));

        std::fs::write(tmp.path().join("video.webm"), b"not really").unwrap();
        let b = load_bundle(tmp.path()).unwrap();
        assert!(b.cursor.is_none(), "cursor log is optional");
        assert_eq!(b.video_path, tmp.path().join("video.webm"));
    }
}
