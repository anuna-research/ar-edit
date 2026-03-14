use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use thiserror::Error;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("no video player found: install mpv (preferred), VLC, or ffplay")]
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
    /// mpv media player with IPC support.
    Mpv,
    /// VLC media player (cvlc or vlc).
    Vlc,
    /// ffplay from the FFmpeg suite.
    Ffplay,
}

/// Detect the best available video player.
///
/// Resolution order:
///   1. `mpv`   — mpv media player (preferred, best IPC support)
///   2. `cvlc`  — VLC without GUI
///   3. `vlc`   — VLC with GUI
///   4. macOS VLC.app bundle — `/Applications/VLC.app/Contents/MacOS/VLC`
///   5. `ffplay` — FFmpeg player (fallback)
///
/// Returns `Err(PlaybackError::PlayerNotFound)` if none are found.
pub fn detect_player() -> Result<Player, PlaybackError> {
    static CANDIDATES: &[(&str, PlayerKind)] = &[
        ("mpv", PlayerKind::Mpv),
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

    // Check for macOS .app bundle installations
    #[cfg(target_os = "macos")]
    {
        let vlc_app = PathBuf::from("/Applications/VLC.app/Contents/MacOS/VLC");
        if vlc_app.exists() {
            return Ok(Player {
                name: "VLC".to_string(),
                path: vlc_app,
                kind: PlayerKind::Vlc,
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
    /// Optional IPC socket path for player position capture (mpv or VLC RC).
    pub ipc_socket: Option<PathBuf>,
    /// Source ID for POI creation during playback.
    pub source_id: Option<String>,
    /// Optional Lua script path for mpv in-player POI keybindings.
    pub mpv_script: Option<PathBuf>,
    /// Optional marker file path where mpv Lua script writes POI marks.
    pub marker_file: Option<PathBuf>,
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
        PlayerKind::Mpv => {
            cmd.arg(&req.file);
            cmd.arg(format!("--start={start_secs:.3}"));
            if let Some(end_ms) = req.end_ms {
                let length_secs = (end_ms - req.start_ms) as f64 / 1000.0;
                cmd.arg(format!("--length={length_secs:.3}"));
            }
            // IPC socket path for position capture
            if let Some(ref ipc_path) = req.ipc_socket {
                cmd.arg(format!("--input-ipc-server={}", ipc_path.display()));
            }
            // Lua script for in-player POI keybindings
            if let Some(ref script) = req.mpv_script {
                cmd.arg(format!("--script={}", script.display()));
            }
        }
        PlayerKind::Vlc => {
            // Enable HTTP interface for precise position queries during POI capture
            if req.marker_file.is_some() {
                cmd.arg("--extraintf").arg("http");
                cmd.arg(format!("--http-port={}", VLC_HTTP_PORT));
                cmd.arg("--http-password=ar-edit");
            }
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
    let manifest = crate::project::read_manifest(project_dir)
        .map_err(|e| PlaybackError::Io(std::io::Error::other(e.to_string())))?;

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
// mpv IPC client
// ---------------------------------------------------------------------------

/// Query mpv for the current playback position via IPC socket.
///
/// Sends `{ "command": ["get_property", "time-pos"] }` and parses the response.
/// Returns the position in milliseconds, or None if the socket isn't available.
pub fn mpv_get_position(socket_path: &Path) -> Option<u64> {
    use std::io::{BufRead, BufReader, Write};
    #[cfg(unix)]
    use std::os::unix::net::UnixStream;

    #[cfg(not(unix))]
    return None;

    #[cfg(unix)]
    {
        let mut stream = UnixStream::connect(socket_path).ok()?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_millis(500)))
            .ok()?;

        let cmd = r#"{ "command": ["get_property", "time-pos"] }"#;
        writeln!(stream, "{}", cmd).ok()?;

        let reader = BufReader::new(&stream);
        for line in reader.lines() {
            let line = line.ok()?;
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(pos) = json.get("data").and_then(|v| v.as_f64()) {
                    return Some((pos * 1000.0) as u64);
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// mpv Lua script for in-player POI capture
// ---------------------------------------------------------------------------

/// Generate a Lua script for mpv that captures POI keypresses in-player.
///
/// Press `i` in mpv to mark a POI. An OSD menu appears with category choices:
/// `h`=highlight, `i`=issue, `t`=transition, `c`=cue, `n`=note, `Esc`=cancel.
/// The script writes `<timestamp_ms> <category>\n` to the marker file.
pub fn generate_mpv_poi_script(marker_file: &Path) -> String {
    format!(
        r#"-- ar-edit POI marker script
local marker_path = "{marker_file}"
local pending_time = nil

local function clear_bindings()
    mp.remove_key_binding("poi-h")
    mp.remove_key_binding("poi-i")
    mp.remove_key_binding("poi-t")
    mp.remove_key_binding("poi-c")
    mp.remove_key_binding("poi-n")
    mp.remove_key_binding("poi-esc")
end

local function write_marker(category)
    if pending_time == nil then return end
    local ms = math.floor(pending_time * 1000)
    local f = io.open(marker_path, "a")
    if f then
        f:write(string.format("%d %s\n", ms, category))
        f:close()
    end
    local t = string.format("%d:%02d.%d", math.floor(pending_time/60), math.floor(pending_time) % 60, math.floor((pending_time * 10) % 10))
    mp.osd_message("\u{{2713}} " .. category .. " @ " .. t, 2)
    pending_time = nil
    clear_bindings()
end

local function cancel()
    mp.osd_message("cancelled", 1)
    pending_time = nil
    clear_bindings()
end

mp.add_key_binding("i", "poi-mark", function()
    pending_time = mp.get_property_number("time-pos")
    mp.osd_message("[h]ighlight  [i]ssue  [t]ransition  [c]ue  [n]ote  (Esc cancel)", 10)
    mp.add_forced_key_binding("h", "poi-h", function() write_marker("highlight") end)
    mp.add_forced_key_binding("i", "poi-i", function() write_marker("issue") end)
    mp.add_forced_key_binding("t", "poi-t", function() write_marker("transition") end)
    mp.add_forced_key_binding("c", "poi-c", function() write_marker("cue") end)
    mp.add_forced_key_binding("n", "poi-n", function() write_marker("note") end)
    mp.add_forced_key_binding("ESC", "poi-esc", cancel)
end)
"#,
        marker_file = marker_file.display()
    )
}

// ---------------------------------------------------------------------------
// VLC HTTP interface for position queries
// ---------------------------------------------------------------------------

/// Default VLC HTTP interface port used by ar-edit.
pub const VLC_HTTP_PORT: u16 = 9090;

/// Query VLC for the current playback position via its HTTP interface.
///
/// VLC is launched with `--extraintf http --http-port <port> --http-password ar-edit`.
/// Returns position in milliseconds, or None if VLC isn't responding.
pub fn vlc_http_get_position(port: u16) -> Option<u64> {
    // Use a minimal HTTP GET — avoid pulling in reqwest for this
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}")).ok()?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_millis(500)))
        .ok()?;

    // Basic auth: ":ar-edit" base64-encoded
    let auth = base64_encode(b":ar-edit");
    let request = format!(
        "GET /requests/status.json HTTP/1.0\r\n\
         Authorization: Basic {auth}\r\n\
         Host: 127.0.0.1:{port}\r\n\
         \r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;

    // Read the full response
    let mut response = String::new();
    let reader = BufReader::new(&stream);
    for line in reader.lines() {
        match line {
            Ok(l) => response.push_str(&l),
            Err(_) => break,
        }
    }

    // Find the JSON body (after empty line in HTTP response)
    // Look for "time" field in JSON
    if let Some(json_start) = response.find('{') {
        let json_str = &response[json_start..];
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
            // VLC HTTP returns "time" in seconds (integer)
            if let Some(time_secs) = json.get("time").and_then(|v| v.as_u64()) {
                return Some(time_secs * 1000);
            }
        }
    }
    None
}

/// Minimal base64 encoder (avoids pulling in a crate for this one use).
fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[(n >> 18 & 63) as usize] as char);
        result.push(CHARS[(n >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[(n >> 6 & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
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
                    PlayerKind::Mpv => {
                        assert_eq!(player.name, "mpv");
                    }
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
        assert_eq!(PlayerKind::Mpv, PlayerKind::Mpv);
        assert_eq!(PlayerKind::Vlc, PlayerKind::Vlc);
        assert_ne!(PlayerKind::Vlc, PlayerKind::Ffplay);
        assert_ne!(PlayerKind::Mpv, PlayerKind::Vlc);
    }

    #[test]
    fn player_not_found_error_message() {
        let err = PlaybackError::PlayerNotFound;
        let msg = err.to_string();
        assert!(msg.contains("mpv"), "error should mention mpv");
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
            ipc_socket: None,
            source_id: None, mpv_script: None, marker_file: None,
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
            ipc_socket: None,
            source_id: None, mpv_script: None, marker_file: None,
        };
        // Verify duration calculation
        let duration_secs = req.end_ms.unwrap().saturating_sub(req.start_ms) as f64 / 1000.0;
        assert!((duration_secs - 4.5).abs() < 0.001);

        let prog = Command::new(&player.path)
            .get_program()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(prog, "/usr/bin/ffplay");
    }

    // -- PlayRequest ----------------------------------------------------------

    #[test]
    fn play_request_clone() {
        let req = PlayRequest {
            file: PathBuf::from("/tmp/test.mp4"),
            start_ms: 1000,
            end_ms: Some(5000),
            ipc_socket: None,
            source_id: None, mpv_script: None, marker_file: None,
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
