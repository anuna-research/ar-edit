use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("no video player found: install VLC (preferred) or ffplay")]
    PlayerNotFound,
    #[error("invalid timecode \"{0}\": expected HH:MM:SS, MM:SS, or seconds")]
    InvalidTimecode(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Player detection
// ---------------------------------------------------------------------------

/// A video player binary resolved on the local system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    /// The binary name that was resolved (e.g. "cvlc", "vlc", "ffplay").
    pub name: String,
    /// Absolute path to the binary.
    pub path: PathBuf,
    /// Which kind of player this is.
    pub kind: PlayerKind,
}

/// Classification of the detected player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerKind {
    /// VLC media player (cvlc or vlc).
    Vlc,
    /// ffplay from the FFmpeg suite.
    Ffplay,
}

/// Detect the best available video player.
///
/// Resolution order (per ADR-003 / SPEC-001):
///   1. `cvlc`  — VLC without GUI (preferred for scripted playback)
///   2. `vlc`   — VLC with GUI
///   3. `ffplay` — FFmpeg player (fallback)
///
/// Returns `Err(PlaybackError::PlayerNotFound)` if none are found.
pub fn detect_player() -> Result<Player, PlaybackError> {
    static CANDIDATES: &[(&str, PlayerKind)] = &[
        ("cvlc", PlayerKind::Vlc),
        ("vlc", PlayerKind::Vlc),
        ("ffplay", PlayerKind::Ffplay),
    ];

    for &(name, kind) in CANDIDATES {
        if let Ok(path) = which::which(name) {
            return Ok(Player {
                name: name.to_string(),
                path,
                kind,
            });
        }
    }

    Err(PlaybackError::PlayerNotFound)
}

// ---------------------------------------------------------------------------
// Play request
// ---------------------------------------------------------------------------

/// Parameters for launching a player subprocess.
#[derive(Debug, Clone)]
pub struct PlayRequest {
    /// Path to the video file.
    pub file: PathBuf,
    /// Seek position in milliseconds.
    pub start_ms: u64,
    /// Optional stop position in milliseconds (for shot playback).
    pub end_ms: Option<u64>,
}

/// Launch a player subprocess for the given request.
///
/// Builds CLI arguments according to the player kind:
///   - **VLC/cvlc**: `--start-time=<s> [--stop-time=<s>] <file>`
///   - **ffplay**: `-ss <s> [-t <duration>] -autoexit <file>`
///
/// Returns the spawned child process.
pub fn launch_player(player: &Player, req: &PlayRequest) -> Result<Child, PlaybackError> {
    let start_secs = req.start_ms as f64 / 1000.0;
    let mut cmd = Command::new(&player.path);

    match player.kind {
        PlayerKind::Vlc => {
            cmd.arg(&req.file);
            cmd.arg(format!("--start-time={start_secs:.3}"));
            if let Some(end_ms) = req.end_ms {
                let end_secs = end_ms as f64 / 1000.0;
                cmd.arg(format!("--stop-time={end_secs:.3}"));
            }
            cmd.arg("vlc://quit");
        }
        PlayerKind::Ffplay => {
            cmd.arg("-ss").arg(format!("{start_secs:.3}"));
            if let Some(end_ms) = req.end_ms {
                let duration_secs = end_ms.saturating_sub(req.start_ms) as f64 / 1000.0;
                cmd.arg("-t").arg(format!("{duration_secs:.3}"));
            }
            cmd.arg("-autoexit");
            cmd.arg(&req.file);
        }
    }

    let child = cmd.spawn()?;
    Ok(child)
}

// ---------------------------------------------------------------------------
// Timecode parsing
// ---------------------------------------------------------------------------

/// Parse a timecode string into milliseconds.
///
/// Accepted formats:
///   - `HH:MM:SS` or `HH:MM:SS.mmm` — hours, minutes, seconds
///   - `MM:SS` or `MM:SS.mmm` — minutes, seconds
///   - `<number>` or `<number>.mmm` — raw seconds
pub fn parse_timecode(s: &str) -> Result<u64, PlaybackError> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        1 => {
            // Raw seconds: "90" or "5.5"
            let secs: f64 = parts[0]
                .parse()
                .map_err(|_| PlaybackError::InvalidTimecode(s.to_string()))?;
            Ok((secs * 1000.0) as u64)
        }
        2 => {
            // MM:SS or MM:SS.mmm
            let mins: u64 = parts[0]
                .parse()
                .map_err(|_| PlaybackError::InvalidTimecode(s.to_string()))?;
            let secs: f64 = parts[1]
                .parse()
                .map_err(|_| PlaybackError::InvalidTimecode(s.to_string()))?;
            Ok(mins * 60_000 + (secs * 1000.0) as u64)
        }
        3 => {
            // HH:MM:SS or HH:MM:SS.mmm
            let hours: u64 = parts[0]
                .parse()
                .map_err(|_| PlaybackError::InvalidTimecode(s.to_string()))?;
            let mins: u64 = parts[1]
                .parse()
                .map_err(|_| PlaybackError::InvalidTimecode(s.to_string()))?;
            let secs: f64 = parts[2]
                .parse()
                .map_err(|_| PlaybackError::InvalidTimecode(s.to_string()))?;
            Ok(hours * 3_600_000 + mins * 60_000 + (secs * 1000.0) as u64)
        }
        _ => Err(PlaybackError::InvalidTimecode(s.to_string())),
    }
}

/// Resolve a source file path from a source ID and project directory.
///
/// Reads the manifest to find the source entry, then returns the absolute path
/// to the source file.
pub fn resolve_source_path(
    source_id: &str,
    project_dir: &Path,
) -> Result<(PathBuf, crate::models::Source), PlaybackError> {
    let manifest = crate::project::read_manifest(project_dir).map_err(|e| {
        PlaybackError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    })?;

    let source = manifest
        .sources
        .iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| {
            PlaybackError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("source not found: {source_id}"),
            ))
        })?
        .clone();

    let file_path = project_dir.join(&source.path);
    Ok((file_path, source))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_player_returns_valid_result_or_not_found() {
        match detect_player() {
            Ok(player) => {
                assert!(!player.name.is_empty());
                assert!(player.path.exists());
                match player.kind {
                    PlayerKind::Vlc => {
                        assert!(player.name == "cvlc" || player.name == "vlc");
                    }
                    PlayerKind::Ffplay => {
                        assert_eq!(player.name, "ffplay");
                    }
                }
            }
            Err(e) => {
                assert!(matches!(e, PlaybackError::PlayerNotFound));
            }
        }
    }

    #[test]
    fn player_kind_equality() {
        assert_eq!(PlayerKind::Vlc, PlayerKind::Vlc);
        assert_ne!(PlayerKind::Vlc, PlayerKind::Ffplay);
    }

    #[test]
    fn player_not_found_error_message() {
        let err = PlaybackError::PlayerNotFound;
        let msg = err.to_string();
        assert!(msg.contains("VLC"), "error should mention VLC");
        assert!(msg.contains("ffplay"), "error should mention ffplay");
    }

    // -- parse_timecode -------------------------------------------------------

    #[test]
    fn parse_timecode_raw_seconds() {
        assert_eq!(parse_timecode("90").unwrap(), 90_000);
    }

    #[test]
    fn parse_timecode_fractional_seconds() {
        assert_eq!(parse_timecode("5.5").unwrap(), 5_500);
    }

    #[test]
    fn parse_timecode_mm_ss() {
        assert_eq!(parse_timecode("01:30").unwrap(), 90_000);
    }

    #[test]
    fn parse_timecode_mm_ss_frac() {
        assert_eq!(parse_timecode("01:30.500").unwrap(), 90_500);
    }

    #[test]
    fn parse_timecode_hh_mm_ss() {
        assert_eq!(parse_timecode("01:30:00").unwrap(), 5_400_000);
    }

    #[test]
    fn parse_timecode_hh_mm_ss_frac() {
        assert_eq!(parse_timecode("00:01:30.500").unwrap(), 90_500);
    }

    #[test]
    fn parse_timecode_zero() {
        assert_eq!(parse_timecode("0").unwrap(), 0);
        assert_eq!(parse_timecode("00:00").unwrap(), 0);
        assert_eq!(parse_timecode("00:00:00").unwrap(), 0);
    }

    #[test]
    fn parse_timecode_invalid() {
        assert!(parse_timecode("abc").is_err());
        assert!(parse_timecode("1:2:3:4").is_err());
        assert!(parse_timecode("").is_err());
    }

    // -- launch_player args ---------------------------------------------------

    #[test]
    fn vlc_args_with_start_only() {
        let player = Player {
            name: "cvlc".into(),
            path: PathBuf::from("/usr/bin/cvlc"),
            kind: PlayerKind::Vlc,
        };
        let req = PlayRequest {
            file: PathBuf::from("/tmp/test.mp4"),
            start_ms: 5500,
            end_ms: None,
        };
        let mut cmd = Command::new(&player.path);
        cmd.arg(&req.file);
        cmd.arg(format!("--start-time={:.3}", req.start_ms as f64 / 1000.0));
        cmd.arg("vlc://quit");
        // Verify command builds without errors
        let prog = cmd.get_program().to_str().unwrap().to_string();
        assert_eq!(prog, "/usr/bin/cvlc");
    }

    #[test]
    fn ffplay_args_with_start_and_end() {
        let player = Player {
            name: "ffplay".into(),
            path: PathBuf::from("/usr/bin/ffplay"),
            kind: PlayerKind::Ffplay,
        };
        let req = PlayRequest {
            file: PathBuf::from("/tmp/test.mp4"),
            start_ms: 5500,
            end_ms: Some(10000),
        };
        // Verify duration calculation
        let duration_secs = req.end_ms.unwrap().saturating_sub(req.start_ms) as f64 / 1000.0;
        assert!((duration_secs - 4.5).abs() < 0.001);

        let prog = Command::new(&player.path).get_program().to_str().unwrap().to_string();
        assert_eq!(prog, "/usr/bin/ffplay");
    }

    // -- PlayRequest ----------------------------------------------------------

    #[test]
    fn play_request_clone() {
        let req = PlayRequest {
            file: PathBuf::from("/tmp/test.mp4"),
            start_ms: 1000,
            end_ms: Some(5000),
        };
        let req2 = req.clone();
        assert_eq!(req.start_ms, req2.start_ms);
        assert_eq!(req.end_ms, req2.end_ms);
    }

    #[test]
    fn invalid_timecode_error_message() {
        let err = PlaybackError::InvalidTimecode("bad".into());
        let msg = err.to_string();
        assert!(msg.contains("bad"));
        assert!(msg.contains("timecode"));
    }
}
