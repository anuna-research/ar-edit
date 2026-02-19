use std::path::PathBuf;

use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("no video player found: install VLC (preferred) or ffplay")]
    PlayerNotFound,
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
}
