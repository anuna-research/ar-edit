use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Overlay mode (REQ-023)
// ---------------------------------------------------------------------------

/// Overlay mode for playback and rendering per REQ-023.
///
/// Three modes:
///   - `Clean` — no overlay (default, no `--overlay` flag)
///   - `Full`  — timecode + shot ID + source ID + transcript/scene snippet
///   - `Minimal` — timecode only
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    Clean,
    Full,
    Minimal,
}

impl OverlayMode {
    /// Parse overlay mode from the CLI `--overlay` flag value.
    ///
    ///   - `None`            → `Clean`
    ///   - `Some("minimal")` → `Minimal`
    ///   - `Some(_)`         → `Full`
    pub fn from_flag(value: Option<&str>) -> OverlayMode {
        match value {
            None => OverlayMode::Clean,
            Some("minimal") => OverlayMode::Minimal,
            _ => OverlayMode::Full,
        }
    }
}

// ---------------------------------------------------------------------------
// Overlay info (per-shot)
// ---------------------------------------------------------------------------

/// Per-shot information needed to build the overlay drawtext filter.
#[derive(Debug, Clone)]
pub struct OverlayInfo {
    /// Shot identifier (e.g. "shot-003").
    pub shot_id: String,
    /// Source identifier (e.g. "src-002").
    pub source_id: String,
    /// Transcript text or scene description snippet.
    pub snippet: Option<String>,
    /// Seconds to add to PTS for running timecode in the edit timeline.
    pub timecode_offset_sec: f64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build an ffmpeg drawtext video filter string for a segment's overlay.
///
/// Returns `None` for `Clean` mode (no filter needed).
///
/// The generated filter uses:
///   - Semi-transparent black background box
///   - White monospace text
///   - Top-left corner positioning
///   - Running timecode via `%{pts:hms:OFFSET}` expansion
pub fn build_drawtext_filter(mode: OverlayMode, info: &OverlayInfo) -> Option<String> {
    match mode {
        OverlayMode::Clean => None,
        OverlayMode::Minimal => Some(build_minimal_filter(info.timecode_offset_sec)),
        OverlayMode::Full => Some(build_full_filter(info)),
    }
}

// ---------------------------------------------------------------------------
// Filter builders
// ---------------------------------------------------------------------------

/// Minimal overlay: running timecode only.
fn build_minimal_filter(offset_sec: f64) -> String {
    let font = font_spec();
    let tc = timecode_expr(offset_sec);
    format!(
        "drawtext=text='{tc}':{font}:fontsize=16:fontcolor=white\
         :box=1:boxcolor=black@0.5:boxborderw=8:x=10:y=10"
    )
}

/// Full overlay: line 1 = timecode + shot ID + source ID,
///               line 2 = transcript/scene snippet (if available).
fn build_full_filter(info: &OverlayInfo) -> String {
    let font = font_spec();
    let tc = timecode_expr(info.timecode_offset_sec);
    let shot = &info.shot_id;
    let src = &info.source_id;

    let line1 = format!(
        "drawtext=text='{tc}  {shot}  {src}':{font}:fontsize=16:fontcolor=white\
         :box=1:boxcolor=black@0.5:boxborderw=8:x=10:y=10"
    );

    if let Some(ref snippet) = info.snippet {
        let safe = escape_drawtext_text(snippet);
        let line2 = format!(
            "drawtext=text='{safe}':{font}:fontsize=14:fontcolor=white\
             :box=1:boxcolor=black@0.5:boxborderw=6:x=10:y=42"
        );
        format!("{line1},{line2}")
    } else {
        line1
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build the ffmpeg pts-based timecode expression with an offset.
///
/// Produces `%{pts:hms:OFFSET}` which ffmpeg expands to `HH:MM:SS.mmm`.
fn timecode_expr(offset_sec: f64) -> String {
    format!("%{{pts:hms:{offset_sec:.3}}}")
}

/// Escape text for use inside an ffmpeg drawtext `text='...'` parameter.
///
/// Handles characters that are special in the filter or text expansion:
///   - `'`  → removed (would break the single-quote delimiters)
///   - `\\` → `\\\\` (literal backslash)
///   - `%`  → `%%` (literal percent, avoids expansion)
fn escape_drawtext_text(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\'' => {}
            '\\' => result.push_str("\\\\"),
            '%' => result.push_str("%%"),
            _ => result.push(c),
        }
    }
    result
}

/// Build the font specification portion of the drawtext filter.
///
/// Tries to find a monospace font file on the system; falls back to
/// fontconfig's `monospace` family.
fn font_spec() -> String {
    match find_monospace_font() {
        Some(path) => format!("fontfile='{}'", path.display()),
        None => "font=monospace".to_string(),
    }
}

/// Try to locate a monospace font file at well-known system paths.
fn find_monospace_font() -> Option<PathBuf> {
    static CANDIDATES: &[&str] = &[
        // macOS
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/Courier.dfont",
        "/Library/Fonts/Courier New.ttf",
        // Linux (common distros)
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    ];

    for path in CANDIDATES {
        if Path::new(path).exists() {
            return Some(PathBuf::from(path));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- OverlayMode::from_flag -----------------------------------------------

    #[test]
    fn from_flag_none_is_clean() {
        assert_eq!(OverlayMode::from_flag(None), OverlayMode::Clean);
    }

    #[test]
    fn from_flag_full() {
        assert_eq!(OverlayMode::from_flag(Some("full")), OverlayMode::Full);
    }

    #[test]
    fn from_flag_minimal() {
        assert_eq!(
            OverlayMode::from_flag(Some("minimal")),
            OverlayMode::Minimal
        );
    }

    #[test]
    fn from_flag_unknown_defaults_to_full() {
        assert_eq!(OverlayMode::from_flag(Some("other")), OverlayMode::Full);
    }

    // -- build_drawtext_filter ------------------------------------------------

    #[test]
    fn clean_mode_returns_none() {
        let info = make_info();
        assert!(build_drawtext_filter(OverlayMode::Clean, &info).is_none());
    }

    #[test]
    fn minimal_mode_contains_timecode() {
        let info = make_info();
        let filter = build_drawtext_filter(OverlayMode::Minimal, &info).unwrap();
        assert!(filter.contains("drawtext="));
        assert!(filter.contains("pts:hms:"));
        assert!(filter.contains("fontsize=16"));
        assert!(filter.contains("boxcolor=black@0.5"));
        // Should NOT contain shot/source info
        assert!(!filter.contains("shot-003"));
        assert!(!filter.contains("src-002"));
    }

    #[test]
    fn full_mode_contains_all_info() {
        let info = make_info();
        let filter = build_drawtext_filter(OverlayMode::Full, &info).unwrap();
        assert!(filter.contains("shot-003"));
        assert!(filter.contains("src-002"));
        assert!(filter.contains("pts:hms:"));
        assert!(filter.contains("boxcolor=black@0.5"));
    }

    #[test]
    fn full_mode_with_snippet_has_two_drawtext() {
        let info = make_info();
        let filter = build_drawtext_filter(OverlayMode::Full, &info).unwrap();
        let count = filter.matches("drawtext=").count();
        assert_eq!(
            count, 2,
            "full overlay with snippet should have 2 drawtext filters"
        );
    }

    #[test]
    fn full_mode_without_snippet_has_one_drawtext() {
        let info = OverlayInfo {
            shot_id: "shot-001".into(),
            source_id: "src-001".into(),
            snippet: None,
            timecode_offset_sec: 0.0,
        };
        let filter = build_drawtext_filter(OverlayMode::Full, &info).unwrap();
        let count = filter.matches("drawtext=").count();
        assert_eq!(
            count, 1,
            "full overlay without snippet should have 1 drawtext filter"
        );
    }

    #[test]
    fn timecode_offset_included() {
        let info = OverlayInfo {
            shot_id: "shot-001".into(),
            source_id: "src-001".into(),
            snippet: None,
            timecode_offset_sec: 90.5,
        };
        let filter = build_drawtext_filter(OverlayMode::Minimal, &info).unwrap();
        assert!(
            filter.contains("pts:hms:90.500"),
            "filter should contain offset"
        );
    }

    #[test]
    fn zero_offset() {
        let info = OverlayInfo {
            shot_id: "shot-001".into(),
            source_id: "src-001".into(),
            snippet: None,
            timecode_offset_sec: 0.0,
        };
        let filter = build_drawtext_filter(OverlayMode::Minimal, &info).unwrap();
        assert!(filter.contains("pts:hms:0.000"));
    }

    // -- escape_drawtext_text -------------------------------------------------

    #[test]
    fn escape_plain_text() {
        assert_eq!(escape_drawtext_text("hello world"), "hello world");
    }

    #[test]
    fn escape_single_quotes() {
        assert_eq!(escape_drawtext_text("it's a test"), "its a test");
    }

    #[test]
    fn escape_backslashes() {
        assert_eq!(escape_drawtext_text("path\\to\\file"), "path\\\\to\\\\file");
    }

    #[test]
    fn escape_percent() {
        assert_eq!(escape_drawtext_text("100% done"), "100%% done");
    }

    #[test]
    fn escape_combined() {
        assert_eq!(
            escape_drawtext_text("it's 100% done\\finished"),
            "its 100%% done\\\\finished"
        );
    }

    // -- timecode_expr --------------------------------------------------------

    #[test]
    fn timecode_expr_format() {
        let expr = timecode_expr(90.5);
        assert_eq!(expr, "%{pts:hms:90.500}");
    }

    #[test]
    fn timecode_expr_zero() {
        let expr = timecode_expr(0.0);
        assert_eq!(expr, "%{pts:hms:0.000}");
    }

    // -- font_spec ------------------------------------------------------------

    #[test]
    fn font_spec_returns_nonempty() {
        let spec = font_spec();
        assert!(!spec.is_empty());
        // Should contain either fontfile= or font=
        assert!(
            spec.contains("fontfile=") || spec.contains("font="),
            "font spec should specify a font"
        );
    }

    // -- helpers --------------------------------------------------------------

    fn make_info() -> OverlayInfo {
        OverlayInfo {
            shot_id: "shot-003".into(),
            source_id: "src-002".into(),
            snippet: Some("Welcome to the interview".into()),
            timecode_offset_sec: 30.0,
        }
    }
}
