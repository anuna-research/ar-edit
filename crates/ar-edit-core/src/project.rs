use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::{Defaults, Manifest, Source};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("directory already exists: {0}")]
    DirectoryExists(PathBuf),
    #[error("file not found: {0}")]
    FileNotFound(PathBuf),
    #[error("not a video file: {path}: {reason}")]
    NotAVideo { path: PathBuf, reason: String },
    #[error("ffprobe failed: {0}")]
    FfprobeFailed(String),
    #[error("not an ar-edit project directory (missing manifest.json)")]
    NotAProject,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse JSON: {0}")]
    Json(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// Doctor types
// ---------------------------------------------------------------------------

/// Result of `doctor` — presence and version of each external dependency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoctorResult {
    pub ffmpeg: DepStatus,
    pub ffprobe: DepStatus,
    pub whisper: DepStatus,
    pub vlc: DepStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DepStatus {
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_hint: Option<String>,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Subdirectories created inside every project.
const PROJECT_DIRS: &[&str] = &[
    "sources",
    "transcripts",
    "index",
    "thumbnails",
    "edits",
    "annotations",
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new project directory at `project_dir` with subdirectories and manifest.json.
pub fn init(project_dir: &Path) -> Result<Manifest, ProjectError> {
    if project_dir.exists() {
        return Err(ProjectError::DirectoryExists(project_dir.to_path_buf()));
    }

    fs::create_dir_all(project_dir)?;
    for sub in PROJECT_DIRS {
        fs::create_dir(project_dir.join(sub))?;
    }

    let name = project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("untitled")
        .to_string();

    let manifest = Manifest {
        version: "1.0.0".into(),
        name,
        created: Utc::now(),
        sources: vec![],
        next_source_id: 1,
        defaults: Defaults {
            whisper_model: "base".into(),
            thumbnail_interval_sec: 10,
            render_codec: "h264".into(),
            render_container: "mp4".into(),
        },
    };

    write_manifest(project_dir, &manifest)?;
    Ok(manifest)
}

/// Add source files to the project. Validates each file via ffprobe (REQ-003),
/// symlinks into sources/, and updates manifest.json.
pub fn add(project_dir: &Path, files: &[PathBuf]) -> Result<Vec<Source>, ProjectError> {
    let mut manifest = read_manifest(project_dir)?;
    let mut added = Vec::new();

    for file in files {
        if !file.exists() {
            return Err(ProjectError::FileNotFound(file.clone()));
        }

        let probe = run_ffprobe(file)?;
        let source = register_source(project_dir, file, &probe, &mut manifest)?;
        added.push(source);
    }

    write_manifest(project_dir, &manifest)?;
    Ok(added)
}

/// Detect external dependencies (ffmpeg, ffprobe, whisper-cli, vlc/ffplay).
pub fn doctor() -> DoctorResult {
    let vlc_status = check_dep("vlc");
    let vlc = if vlc_status.found {
        vlc_status
    } else {
        let ffplay = check_dep("ffplay");
        DepStatus {
            found: false,
            path: None,
            version: None,
            fallback: if ffplay.found {
                Some("ffplay".into())
            } else {
                None
            },
            install_hint: install_hint_for("vlc"),
        }
    };

    DoctorResult {
        ffmpeg: check_dep("ffmpeg"),
        ffprobe: check_dep("ffprobe"),
        whisper: check_dep("whisper-cli"),
        vlc,
    }
}

/// Read manifest.json from a project directory.
pub fn read_manifest(project_dir: &Path) -> Result<Manifest, ProjectError> {
    let path = project_dir.join("manifest.json");
    if !path.exists() {
        return Err(ProjectError::NotAProject);
    }
    let content = fs::read_to_string(&path)?;
    let manifest: Manifest = serde_json::from_str(&content)?;
    Ok(manifest)
}

/// Write manifest.json to a project directory.
pub fn write_manifest(project_dir: &Path, manifest: &Manifest) -> Result<(), ProjectError> {
    let path = project_dir.join("manifest.json");
    let json = serde_json::to_string_pretty(manifest)?;
    fs::write(&path, json)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Run ffprobe on a file and parse the output, validating REQ-003
/// (readable video container with at least one audio stream).
fn run_ffprobe(file: &Path) -> Result<ProbeResult, ProjectError> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(file)
        .output()
        .map_err(|e| ProjectError::FfprobeFailed(format!("failed to run ffprobe: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ProjectError::FfprobeFailed(format!(
            "ffprobe exited with {}: {}",
            output.status,
            stderr.trim()
        )));
    }

    let ffprobe: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|e| ProjectError::FfprobeFailed(format!("failed to parse ffprobe output: {e}")))?;

    let video = ffprobe
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"))
        .ok_or_else(|| ProjectError::NotAVideo {
            path: file.to_path_buf(),
            reason: "no video stream found".into(),
        })?;

    let audio = ffprobe
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"))
        .ok_or_else(|| ProjectError::NotAVideo {
            path: file.to_path_buf(),
            reason: "no audio stream found".into(),
        })?;

    let duration_secs: f64 = ffprobe
        .format
        .duration
        .as_deref()
        .and_then(|d| d.parse().ok())
        .unwrap_or(0.0);

    let frame_rate = video
        .r_frame_rate
        .as_deref()
        .map(parse_frame_rate)
        .unwrap_or(0.0);

    let sample_rate: u32 = audio
        .sample_rate
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    Ok(ProbeResult {
        duration_ms: (duration_secs * 1000.0) as u64,
        video_codec: video.codec_name.clone().unwrap_or_default(),
        audio_codec: audio.codec_name.clone().unwrap_or_default(),
        resolution: (video.width.unwrap_or(0), video.height.unwrap_or(0)),
        frame_rate,
        audio_channels: audio.channels.unwrap_or(0),
        audio_sample_rate: sample_rate,
    })
}

fn register_source(
    project_dir: &Path,
    file: &Path,
    probe: &ProbeResult,
    manifest: &mut Manifest,
) -> Result<Source, ProjectError> {
    let id_num = manifest.next_source_id;
    manifest.next_source_id += 1;
    let id = format!("src-{:03}", id_num);

    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("mp4");

    let dest_name = format!("{id}.{ext}");
    let dest_path = project_dir.join("sources").join(&dest_name);
    let rel_path = PathBuf::from("sources").join(&dest_name);

    let abs_source = fs::canonicalize(file)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&abs_source, &dest_path)?;
    #[cfg(not(unix))]
    fs::copy(&abs_source, &dest_path)?;

    let original_filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let source = Source {
        id,
        path: rel_path,
        original_filename,
        duration_ms: probe.duration_ms,
        video_codec: probe.video_codec.clone(),
        audio_codec: probe.audio_codec.clone(),
        resolution: probe.resolution,
        frame_rate: probe.frame_rate,
        audio_channels: probe.audio_channels,
        audio_sample_rate: probe.audio_sample_rate,
        added: Utc::now(),
        transcribed: false,
        indexed: false,
    };

    manifest.sources.push(source.clone());
    Ok(source)
}

fn check_dep(name: &str) -> DepStatus {
    match which::which(name) {
        Ok(path) => {
            let version = get_version(name);
            DepStatus {
                found: true,
                path: Some(path),
                version,
                fallback: None,
                install_hint: None,
            }
        }
        Err(_) => DepStatus {
            found: false,
            path: None,
            version: None,
            fallback: None,
            install_hint: install_hint_for(name),
        },
    }
}

/// Return a platform-specific install hint for a missing dependency.
fn install_hint_for(dep: &str) -> Option<String> {
    if cfg!(target_os = "macos") {
        let cmd = match dep {
            "ffmpeg" => "brew install ffmpeg",
            "ffprobe" => "brew install ffmpeg",
            "whisper-cli" => "brew install whisper-cpp",
            "vlc" => "brew install --cask vlc",
            "ffplay" => "brew install ffmpeg",
            _ => return None,
        };
        Some(cmd.to_string())
    } else if cfg!(target_os = "linux") {
        let cmd = match dep {
            "ffmpeg" => "sudo apt install ffmpeg",
            "ffprobe" => "sudo apt install ffmpeg",
            "whisper-cli" => "see https://github.com/ggerganov/whisper.cpp",
            "vlc" => "sudo apt install vlc",
            "ffplay" => "sudo apt install ffmpeg",
            _ => return None,
        };
        Some(cmd.to_string())
    } else {
        None
    }
}

fn get_version(name: &str) -> Option<String> {
    let output = Command::new(name).arg("-version").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next()?;
    extract_version(first_line)
}

fn extract_version(line: &str) -> Option<String> {
    let re = regex::Regex::new(r"(\d+\.\d+(?:\.\d+)?)").ok()?;
    re.find(line).map(|m| m.as_str().to_string())
}

fn parse_frame_rate(s: &str) -> f64 {
    if let Some((num, den)) = s.split_once('/') {
        let n: f64 = num.parse().unwrap_or(0.0);
        let d: f64 = den.parse().unwrap_or(1.0);
        if d == 0.0 {
            0.0
        } else {
            n / d
        }
    } else {
        s.parse().unwrap_or(0.0)
    }
}

// ---------------------------------------------------------------------------
// ffprobe output types (internal)
// ---------------------------------------------------------------------------

struct ProbeResult {
    duration_ms: u64,
    video_codec: String,
    audio_codec: String,
    resolution: (u32, u32),
    frame_rate: f64,
    audio_channels: u8,
    audio_sample_rate: u32,
}

#[derive(Deserialize)]
struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
    format: FfprobeFormat,
}

#[derive(Deserialize)]
struct FfprobeStream {
    codec_name: Option<String>,
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    r_frame_rate: Option<String>,
    channels: Option<u8>,
    sample_rate: Option<String>,
}

#[derive(Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // -- init -----------------------------------------------------------------

    #[test]
    fn init_creates_structure() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("my-project");

        let manifest = init(&project).unwrap();

        assert_eq!(manifest.name, "my-project");
        assert_eq!(manifest.version, "1.0.0");
        assert!(manifest.sources.is_empty());
        assert_eq!(manifest.next_source_id, 1);
        assert_eq!(manifest.defaults.whisper_model, "base");
        assert_eq!(manifest.defaults.thumbnail_interval_sec, 10);
        assert_eq!(manifest.defaults.render_codec, "h264");
        assert_eq!(manifest.defaults.render_container, "mp4");

        for sub in PROJECT_DIRS {
            assert!(project.join(sub).is_dir(), "missing dir: {sub}");
        }

        let content = fs::read_to_string(project.join("manifest.json")).unwrap();
        let loaded: Manifest = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.name, "my-project");
    }

    #[test]
    fn init_rejects_existing_directory() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("existing");
        fs::create_dir(&project).unwrap();

        let err = init(&project).unwrap_err();
        assert!(matches!(err, ProjectError::DirectoryExists(_)));
    }

    // -- read_manifest / write_manifest ----------------------------------------

    #[test]
    fn manifest_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("test-project");
        let manifest = init(&project).unwrap();

        let loaded = read_manifest(&project).unwrap();
        assert_eq!(loaded.name, manifest.name);
        assert_eq!(loaded.version, manifest.version);
        assert_eq!(loaded.next_source_id, manifest.next_source_id);
        assert_eq!(loaded.defaults, manifest.defaults);
    }

    #[test]
    fn read_manifest_rejects_non_project() {
        let tmp = TempDir::new().unwrap();
        let err = read_manifest(tmp.path()).unwrap_err();
        assert!(matches!(err, ProjectError::NotAProject));
    }

    // -- add: error cases -----------------------------------------------------

    #[test]
    fn add_rejects_missing_file() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("test-project");
        init(&project).unwrap();

        let err = add(&project, &[PathBuf::from("/nonexistent/video.mp4")]).unwrap_err();
        assert!(matches!(err, ProjectError::FileNotFound(_)));
    }

    // -- ffprobe output parsing -----------------------------------------------

    #[test]
    fn parse_ffprobe_json() {
        let json = r#"{
            "streams": [
                {
                    "codec_name": "h264",
                    "codec_type": "video",
                    "width": 1920,
                    "height": 1080,
                    "r_frame_rate": "30000/1001"
                },
                {
                    "codec_name": "aac",
                    "codec_type": "audio",
                    "channels": 2,
                    "sample_rate": "48000"
                }
            ],
            "format": {
                "duration": "124.500000"
            }
        }"#;

        let output: FfprobeOutput = serde_json::from_str(json).unwrap();
        assert_eq!(output.streams.len(), 2);
        assert_eq!(output.streams[0].codec_type.as_deref(), Some("video"));
        assert_eq!(output.streams[0].codec_name.as_deref(), Some("h264"));
        assert_eq!(output.streams[0].width, Some(1920));
        assert_eq!(output.streams[0].height, Some(1080));
        assert_eq!(output.streams[1].codec_type.as_deref(), Some("audio"));
        assert_eq!(output.streams[1].codec_name.as_deref(), Some("aac"));
        assert_eq!(output.streams[1].channels, Some(2));
        assert_eq!(output.streams[1].sample_rate.as_deref(), Some("48000"));
        assert_eq!(output.format.duration.as_deref(), Some("124.500000"));
    }

    #[test]
    fn parse_frame_rate_fraction() {
        assert!((parse_frame_rate("30000/1001") - 29.97).abs() < 0.01);
        assert!((parse_frame_rate("24/1") - 24.0).abs() < 0.01);
        assert!((parse_frame_rate("30") - 30.0).abs() < 0.01);
        assert!((parse_frame_rate("0/0") - 0.0).abs() < 0.01);
    }

    #[test]
    fn extract_version_from_tool_output() {
        assert_eq!(
            extract_version("ffmpeg version 6.1 Copyright (c) 2000-2023"),
            Some("6.1".into())
        );
        assert_eq!(
            extract_version("ffprobe version 7.0.1 Copyright (c) 2000-2024"),
            Some("7.0.1".into())
        );
        assert_eq!(extract_version("no version here"), None);
    }

    // -- doctor ---------------------------------------------------------------

    #[test]
    fn doctor_returns_structured_result() {
        let result = doctor();
        let json = serde_json::to_value(&result).unwrap();
        assert!(json.get("ffmpeg").is_some());
        assert!(json.get("ffprobe").is_some());
        assert!(json.get("whisper").is_some());
        assert!(json.get("vlc").is_some());
    }

    #[test]
    fn doctor_result_serialization() {
        let result = DoctorResult {
            ffmpeg: DepStatus {
                found: true,
                path: Some(PathBuf::from("/usr/bin/ffmpeg")),
                version: Some("6.1".into()),
                fallback: None,
                install_hint: None,
            },
            ffprobe: DepStatus {
                found: true,
                path: Some(PathBuf::from("/usr/bin/ffprobe")),
                version: Some("6.1".into()),
                fallback: None,
                install_hint: None,
            },
            whisper: DepStatus {
                found: true,
                path: Some(PathBuf::from("/usr/local/bin/whisper-cli")),
                version: Some("1.5.4".into()),
                fallback: None,
                install_hint: None,
            },
            vlc: DepStatus {
                found: false,
                path: None,
                version: None,
                fallback: Some("ffplay".into()),
                install_hint: Some("brew install --cask vlc".into()),
            },
        };

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["ffmpeg"]["found"], true);
        assert_eq!(json["ffmpeg"]["version"], "6.1");
        assert_eq!(json["vlc"]["found"], false);
        assert_eq!(json["vlc"]["fallback"], "ffplay");
        assert_eq!(json["vlc"]["install_hint"], "brew install --cask vlc");
        // Optional fields absent when None
        assert!(json["vlc"].get("path").is_none());
        assert!(json["vlc"].get("version").is_none());
        assert!(json["ffmpeg"].get("install_hint").is_none());

        let back: DoctorResult = serde_json::from_value(json).unwrap();
        assert_eq!(back, result);
    }
}
