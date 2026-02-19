use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use thiserror::Error;

use crate::models::{Transcript, TranscriptSegment, Word};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum TranscriptError {
    #[error("ffmpeg not found: {0}")]
    FfmpegNotFound(std::io::Error),
    #[error("source file has no audio stream: {0}")]
    NoAudioStream(PathBuf),
    #[error("ffmpeg failed: {0}")]
    FfmpegFailed(String),
    #[error("whisper-cli not found: {0}")]
    WhisperNotFound(std::io::Error),
    #[error("whisper-cli failed: {0}")]
    WhisperFailed(String),
    #[error("invalid whisper model '{0}': expected one of tiny, base, small, medium, large")]
    InvalidModel(String),
    #[error("whisper model not found: searched {searched:?} for ggml-{model}.bin")]
    ModelNotFound {
        model: String,
        searched: Vec<PathBuf>,
    },
    #[error("whisper JSON output not found at {0}")]
    WhisperOutputMissing(PathBuf),
    #[error("failed to parse whisper JSON at {path}: {source}")]
    WhisperOutputParse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("failed to parse whisper JSON: {0}")]
    WhisperJsonParse(serde_json::Error),
    #[error("unsupported import format: {0}")]
    UnsupportedFormat(String),
    #[error("invalid SRT: {0}")]
    InvalidSrt(String),
    #[error("invalid VTT: {0}")]
    InvalidVtt(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Supported whisper.cpp model sizes.
pub const WHISPER_MODELS: &[&str] = &["tiny", "base", "small", "medium", "large"];

/// Default model when none specified (REQ-006).
pub const DEFAULT_WHISPER_MODEL: &str = "base";

/// Environment variable for custom model directory.
const MODEL_DIR_ENV: &str = "WHISPER_MODEL_DIR";

// ---------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------

/// A progress update parsed from whisper.cpp stderr.
#[derive(Debug, Clone, PartialEq)]
pub struct WhisperProgress {
    pub percent: u8,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Extract WAV audio from a video file using ffmpeg.
///
/// Runs: `ffmpeg -i <source> -vn -acodec pcm_s16le -ar 16000 -ac 1 <output>`
///
/// The output is 16 kHz mono PCM suitable for whisper.cpp ingestion.
/// Returns the path to the generated WAV file.
pub fn extract_audio(source: &Path, output: &Path) -> Result<PathBuf, TranscriptError> {
    let result = Command::new("ffmpeg")
        .args(["-y", "-i"])
        .arg(source)
        .args(["-vn", "-acodec", "pcm_s16le", "-ar", "16000", "-ac", "1"])
        .arg(output)
        .output()
        .map_err(TranscriptError::FfmpegNotFound)?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);

        if stderr.contains("does not contain any stream")
            || stderr.contains("matches no streams")
            || stderr.contains("no audio stream")
        {
            return Err(TranscriptError::NoAudioStream(source.to_path_buf()));
        }

        return Err(TranscriptError::FfmpegFailed(format!(
            "ffmpeg audio extraction failed: {}",
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    Ok(output.to_path_buf())
}

/// Validate that `model` is a supported whisper.cpp model name.
pub fn validate_model(model: &str) -> Result<(), TranscriptError> {
    if WHISPER_MODELS.contains(&model) {
        Ok(())
    } else {
        Err(TranscriptError::InvalidModel(model.to_string()))
    }
}

/// Search standard locations for a whisper model file (`ggml-{model}.bin`).
///
/// Search order:
/// 1. `WHISPER_MODEL_DIR` environment variable
/// 2. `$HOME/.cache/whisper/`
/// 3. `/usr/local/share/whisper-cpp/models/`
/// 4. `/opt/homebrew/share/whisper-cpp/models/`
pub fn find_model(model: &str) -> Result<PathBuf, TranscriptError> {
    validate_model(model)?;

    let filename = format!("ggml-{model}.bin");
    let mut searched = Vec::new();

    // 1. Environment variable
    if let Ok(dir) = std::env::var(MODEL_DIR_ENV) {
        let path = PathBuf::from(&dir).join(&filename);
        if path.is_file() {
            return Ok(path);
        }
        searched.push(PathBuf::from(dir));
    }

    // 2. Standard search paths
    let home = std::env::var("HOME").ok().map(PathBuf::from);
    let standard_dirs: Vec<PathBuf> = [
        home.map(|h| h.join(".cache/whisper")),
        Some(PathBuf::from("/usr/local/share/whisper-cpp/models")),
        Some(PathBuf::from("/opt/homebrew/share/whisper-cpp/models")),
    ]
    .into_iter()
    .flatten()
    .collect();

    for dir in &standard_dirs {
        let path = dir.join(&filename);
        if path.is_file() {
            return Ok(path);
        }
        searched.push(dir.clone());
    }

    Err(TranscriptError::ModelNotFound {
        model: model.to_string(),
        searched,
    })
}

/// Invoke whisper-cli to transcribe an audio file.
///
/// Runs: `whisper-cli -m <model_path> -f <audio> --output-json --print-progress`
///
/// The JSON output file is written by whisper-cli alongside the audio file
/// (as `<audio_filename>.json`). It is parsed into a [`Transcript`] and then
/// deleted.
///
/// Returns the parsed transcript and any progress updates captured from stderr.
pub fn invoke_whisper(
    audio: &Path,
    model_path: &Path,
    source_id: &str,
) -> Result<(Transcript, Vec<WhisperProgress>), TranscriptError> {
    let result = Command::new("whisper-cli")
        .arg("-m")
        .arg(model_path)
        .arg("-f")
        .arg(audio)
        .args(["--output-json", "--print-progress"])
        .output()
        .map_err(TranscriptError::WhisperNotFound)?;

    let stderr = String::from_utf8_lossy(&result.stderr);
    let progress = parse_progress(&stderr);

    if !result.status.success() {
        return Err(TranscriptError::WhisperFailed(format!(
            "whisper-cli exited with {}: {}",
            result.status,
            stderr.lines().last().unwrap_or("unknown error")
        )));
    }

    // whisper-cli writes <audio_filename>.json in the same directory
    let json_path = audio.with_file_name(format!(
        "{}.json",
        audio.file_name().unwrap_or_default().to_string_lossy()
    ));

    if !json_path.is_file() {
        return Err(TranscriptError::WhisperOutputMissing(json_path));
    }

    let model_name = model_path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("ggml-"))
        .unwrap_or("unknown")
        .to_string();

    let data = std::fs::read_to_string(&json_path)?;
    let transcript = parse_whisper_json(&data, source_id, &model_name).map_err(|e| match e {
        TranscriptError::WhisperJsonParse(source) => TranscriptError::WhisperOutputParse {
            path: json_path.clone(),
            source,
        },
        other => other,
    })?;

    // Clean up the intermediate whisper JSON file
    let _ = std::fs::remove_file(&json_path);

    Ok((transcript, progress))
}

/// Parse progress percentage lines from whisper.cpp stderr output.
///
/// whisper.cpp outputs lines like:
/// `whisper_print_progress_callback: progress =  42%`
pub fn parse_progress(stderr: &str) -> Vec<WhisperProgress> {
    let re = regex::Regex::new(r"progress\s*=\s*(\d+)%").unwrap();
    re.captures_iter(stderr)
        .filter_map(|cap| cap.get(1)?.as_str().parse::<u8>().ok())
        .map(|percent| WhisperProgress { percent })
        .collect()
}

// ---------------------------------------------------------------------------
// whisper.cpp JSON output types (internal)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct WhisperJson {
    result: WhisperResultBlock,
    transcription: Vec<WhisperSegmentBlock>,
}

#[derive(Deserialize)]
struct WhisperResultBlock {
    language: String,
}

#[derive(Deserialize)]
struct WhisperSegmentBlock {
    offsets: WhisperOffsets,
    text: String,
    #[serde(default)]
    tokens: Vec<WhisperTokenBlock>,
}

#[derive(Deserialize)]
struct WhisperOffsets {
    from: u64,
    to: u64,
}

#[derive(Deserialize)]
struct WhisperTokenBlock {
    text: String,
    offsets: WhisperOffsets,
    p: f32,
}

// ---------------------------------------------------------------------------
// Ingest (REQ-005)
// ---------------------------------------------------------------------------

/// Parse whisper.cpp JSON output into our internal [`Transcript`] model (REQ-005).
///
/// This is the core ingest function. It:
/// 1. Filters word tokens only (strips special tokens like `[BLANK]`, `[SOT]`, etc.)
/// 2. Assigns sequential global word indices across all segments
/// 3. Timestamps are integer milliseconds (whisper.cpp `offsets.from` / `offsets.to`)
/// 4. Preserves segment boundaries from whisper.cpp output
///
/// The `json` parameter should contain the raw JSON string produced by
/// `whisper-cli --output-json`.
pub fn parse_whisper_json(
    json: &str,
    source_id: &str,
    model: &str,
) -> Result<Transcript, TranscriptError> {
    let whisper: WhisperJson =
        serde_json::from_str(json).map_err(TranscriptError::WhisperJsonParse)?;

    let mut segments = Vec::new();
    let mut global_word_index: u32 = 0;
    let mut duration_ms: u64 = 0;

    for (seg_idx, seg) in whisper.transcription.iter().enumerate() {
        let words: Vec<Word> = seg
            .tokens
            .iter()
            .filter(|t| is_word_token(&t.text))
            .map(|t| {
                let word = Word {
                    index: global_word_index,
                    text: t.text.trim().to_string(),
                    start_ms: t.offsets.from,
                    end_ms: t.offsets.to,
                    confidence: t.p,
                };
                global_word_index += 1;
                word
            })
            .collect();

        if seg.offsets.to > duration_ms {
            duration_ms = seg.offsets.to;
        }

        segments.push(TranscriptSegment {
            index: seg_idx as u32,
            start_ms: seg.offsets.from,
            end_ms: seg.offsets.to,
            text: seg.text.trim().to_string(),
            words,
        });
    }

    Ok(Transcript {
        source_id: source_id.to_string(),
        model: model.to_string(),
        language: whisper.result.language,
        duration_ms,
        word_count: global_word_index,
        segments,
    })
}

/// Returns true if a whisper token represents an actual word (not a special token).
///
/// Rejects:
/// - Empty or whitespace-only strings
/// - Bracket tokens like `[_BEG_]`, `[_SOT_]`, `[_EOT_]`, `[BLANK]`
/// - Angle-bracket tokens like `<|0.00|>`, `<|endoftext|>`
pub fn is_word_token(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with('[')
        && !trimmed.starts_with("<|")
        && !trimmed.ends_with("|>")
}

// ---------------------------------------------------------------------------
// Import (REQ-007)
// ---------------------------------------------------------------------------

/// Import a transcript from an external file (SRT, VTT, or whisper.cpp JSON).
///
/// The format is auto-detected from the file extension:
/// - `.srt` → SubRip subtitle format
/// - `.vtt` → WebVTT subtitle format
/// - `.json` → whisper.cpp JSON passthrough
///
/// For SRT/VTT, word-level timestamps are interpolated from segment boundaries
/// (even distribution across words within each segment), with confidence set to
/// 1.0 since these formats carry no confidence information.
///
/// For whisper.cpp JSON, word-level timestamps and confidence values are
/// preserved directly via [`parse_whisper_json`].
pub fn import_transcript(
    path: &Path,
    source_id: &str,
) -> Result<Transcript, TranscriptError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let content = std::fs::read_to_string(path)?;

    match ext.as_str() {
        "srt" => parse_srt(&content, source_id),
        "vtt" => parse_vtt(&content, source_id),
        "json" => parse_whisper_json(&content, source_id, "imported"),
        _ => Err(TranscriptError::UnsupportedFormat(ext)),
    }
}

/// Parse an SRT (SubRip) subtitle file into a [`Transcript`].
///
/// SRT format:
/// ```text
/// 1
/// 00:00:00,000 --> 00:00:05,230
/// Welcome to the interview
///
/// 2
/// 00:00:05,230 --> 00:00:11,800
/// This is the second segment
/// ```
///
/// Timestamps use comma as the millisecond separator: `HH:MM:SS,mmm`.
/// Words within each segment receive evenly interpolated timestamps.
pub fn parse_srt(content: &str, source_id: &str) -> Result<Transcript, TranscriptError> {
    let blocks = split_subtitle_blocks(content);
    let mut segments = Vec::new();
    let mut global_word_index: u32 = 0;
    let mut duration_ms: u64 = 0;

    for (seg_idx, block) in blocks.iter().enumerate() {
        let lines: Vec<&str> = block.lines().collect();

        // Find the timestamp line (contains "-->")
        let ts_line_idx = lines
            .iter()
            .position(|l| l.contains("-->"))
            .ok_or_else(|| {
                TranscriptError::InvalidSrt(format!("block {} has no timestamp line", seg_idx + 1))
            })?;

        let (start_ms, end_ms) = parse_srt_timestamp_line(lines[ts_line_idx])?;

        // Text is everything after the timestamp line
        let text: String = lines[ts_line_idx + 1..]
            .iter()
            .copied()
            .collect::<Vec<&str>>()
            .join(" ");
        let text = text.trim().to_string();

        if end_ms > duration_ms {
            duration_ms = end_ms;
        }

        let words = interpolate_words(&text, start_ms, end_ms, &mut global_word_index);

        segments.push(TranscriptSegment {
            index: seg_idx as u32,
            start_ms,
            end_ms,
            text,
            words,
        });
    }

    Ok(Transcript {
        source_id: source_id.to_string(),
        model: "imported".to_string(),
        language: "und".to_string(),
        duration_ms,
        word_count: global_word_index,
        segments,
    })
}

/// Parse a WebVTT subtitle file into a [`Transcript`].
///
/// VTT format:
/// ```text
/// WEBVTT
///
/// 00:00:00.000 --> 00:00:05.230
/// Welcome to the interview
///
/// 00:00:05.230 --> 00:00:11.800
/// This is the second segment
/// ```
///
/// Timestamps use period as the millisecond separator: `HH:MM:SS.mmm`.
/// Optional cue identifiers and NOTE blocks are ignored.
pub fn parse_vtt(content: &str, source_id: &str) -> Result<Transcript, TranscriptError> {
    // Validate WEBVTT header
    let trimmed = content.trim_start_matches('\u{feff}'); // strip BOM
    if !trimmed.starts_with("WEBVTT") {
        return Err(TranscriptError::InvalidVtt(
            "missing WEBVTT header".to_string(),
        ));
    }

    // Skip the header line(s) and any metadata before the first blank line
    let body = trimmed
        .splitn(2, "\n\n")
        .nth(1)
        .or_else(|| trimmed.splitn(2, "\r\n\r\n").nth(1))
        .unwrap_or("");

    let blocks = split_subtitle_blocks(body);
    let mut segments = Vec::new();
    let mut global_word_index: u32 = 0;
    let mut duration_ms: u64 = 0;

    for (seg_idx, block) in blocks.iter().enumerate() {
        // Skip NOTE blocks
        if block.trim_start().starts_with("NOTE") {
            continue;
        }

        let lines: Vec<&str> = block.lines().collect();

        // Find the timestamp line (contains "-->")
        let ts_line_idx = match lines.iter().position(|l| l.contains("-->")) {
            Some(idx) => idx,
            None => continue, // skip blocks without timestamps (e.g. STYLE)
        };

        let (start_ms, end_ms) = parse_vtt_timestamp_line(lines[ts_line_idx])?;

        // Text is everything after the timestamp line
        let text: String = lines[ts_line_idx + 1..]
            .iter()
            .copied()
            .collect::<Vec<&str>>()
            .join(" ");
        let text = text.trim().to_string();

        if text.is_empty() {
            continue;
        }

        if end_ms > duration_ms {
            duration_ms = end_ms;
        }

        let words = interpolate_words(&text, start_ms, end_ms, &mut global_word_index);

        segments.push(TranscriptSegment {
            index: seg_idx as u32,
            start_ms,
            end_ms,
            text,
            words,
        });
    }

    // Re-index segments sequentially (since we may have skipped NOTE/STYLE blocks)
    for (i, seg) in segments.iter_mut().enumerate() {
        seg.index = i as u32;
    }

    Ok(Transcript {
        source_id: source_id.to_string(),
        model: "imported".to_string(),
        language: "und".to_string(),
        duration_ms,
        word_count: global_word_index,
        segments,
    })
}

// ---------------------------------------------------------------------------
// Import helpers
// ---------------------------------------------------------------------------

/// Split subtitle content into blocks separated by blank lines.
fn split_subtitle_blocks(content: &str) -> Vec<String> {
    // Normalise line endings
    let normalised = content.replace("\r\n", "\n").replace('\r', "\n");

    normalised
        .split("\n\n")
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty())
        .collect()
}

/// Parse an SRT timestamp line: `00:00:00,000 --> 00:00:05,230`
fn parse_srt_timestamp_line(line: &str) -> Result<(u64, u64), TranscriptError> {
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return Err(TranscriptError::InvalidSrt(format!(
            "invalid timestamp line: {line}"
        )));
    }

    let start = parse_srt_time(parts[0].trim())?;
    let end = parse_srt_time(parts[1].trim())?;
    Ok((start, end))
}

/// Parse an SRT time: `HH:MM:SS,mmm` → milliseconds.
fn parse_srt_time(s: &str) -> Result<u64, TranscriptError> {
    // Split on comma to separate seconds from milliseconds
    let (time_part, ms_part) = s.split_once(',').ok_or_else(|| {
        TranscriptError::InvalidSrt(format!("missing comma in timestamp: {s}"))
    })?;

    let hms: Vec<&str> = time_part.split(':').collect();
    if hms.len() != 3 {
        return Err(TranscriptError::InvalidSrt(format!(
            "invalid time format: {s}"
        )));
    }

    let h: u64 = hms[0]
        .parse()
        .map_err(|_| TranscriptError::InvalidSrt(format!("invalid hours: {s}")))?;
    let m: u64 = hms[1]
        .parse()
        .map_err(|_| TranscriptError::InvalidSrt(format!("invalid minutes: {s}")))?;
    let sec: u64 = hms[2]
        .parse()
        .map_err(|_| TranscriptError::InvalidSrt(format!("invalid seconds: {s}")))?;
    let ms: u64 = ms_part
        .parse()
        .map_err(|_| TranscriptError::InvalidSrt(format!("invalid milliseconds: {s}")))?;

    Ok(h * 3_600_000 + m * 60_000 + sec * 1_000 + ms)
}

/// Parse a VTT timestamp line: `00:00:00.000 --> 00:00:05.230`
///
/// VTT allows optional settings after the end timestamp (e.g. `position:10%`).
fn parse_vtt_timestamp_line(line: &str) -> Result<(u64, u64), TranscriptError> {
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return Err(TranscriptError::InvalidVtt(format!(
            "invalid timestamp line: {line}"
        )));
    }

    let start = parse_vtt_time(parts[0].trim())?;
    // End timestamp may have positioning settings after it; take only the time
    let end_str = parts[1].trim().split_whitespace().next().unwrap_or("");
    let end = parse_vtt_time(end_str)?;
    Ok((start, end))
}

/// Parse a VTT time: `HH:MM:SS.mmm` or `MM:SS.mmm` → milliseconds.
fn parse_vtt_time(s: &str) -> Result<u64, TranscriptError> {
    let (time_part, ms_part) = s.split_once('.').ok_or_else(|| {
        TranscriptError::InvalidVtt(format!("missing period in timestamp: {s}"))
    })?;

    let hms: Vec<&str> = time_part.split(':').collect();
    let (h, m, sec) = match hms.len() {
        3 => {
            let h: u64 = hms[0]
                .parse()
                .map_err(|_| TranscriptError::InvalidVtt(format!("invalid hours: {s}")))?;
            let m: u64 = hms[1]
                .parse()
                .map_err(|_| TranscriptError::InvalidVtt(format!("invalid minutes: {s}")))?;
            let sec: u64 = hms[2]
                .parse()
                .map_err(|_| TranscriptError::InvalidVtt(format!("invalid seconds: {s}")))?;
            (h, m, sec)
        }
        2 => {
            let m: u64 = hms[0]
                .parse()
                .map_err(|_| TranscriptError::InvalidVtt(format!("invalid minutes: {s}")))?;
            let sec: u64 = hms[1]
                .parse()
                .map_err(|_| TranscriptError::InvalidVtt(format!("invalid seconds: {s}")))?;
            (0, m, sec)
        }
        _ => {
            return Err(TranscriptError::InvalidVtt(format!(
                "invalid time format: {s}"
            )));
        }
    };

    let ms: u64 = ms_part
        .parse()
        .map_err(|_| TranscriptError::InvalidVtt(format!("invalid milliseconds: {s}")))?;

    Ok(h * 3_600_000 + m * 60_000 + sec * 1_000 + ms)
}

/// Generate words from segment text with evenly interpolated timestamps.
///
/// Words inherit the segment time range, spread evenly across the duration.
/// Confidence is set to 1.0 since SRT/VTT have no confidence data.
fn interpolate_words(
    text: &str,
    start_ms: u64,
    end_ms: u64,
    global_word_index: &mut u32,
) -> Vec<Word> {
    let word_texts: Vec<&str> = text.split_whitespace().collect();
    let count = word_texts.len();

    if count == 0 {
        return Vec::new();
    }

    let duration = end_ms.saturating_sub(start_ms);

    word_texts
        .into_iter()
        .enumerate()
        .map(|(i, w)| {
            let word_start = start_ms + (duration * i as u64) / count as u64;
            let word_end = start_ms + (duration * (i + 1) as u64) / count as u64;
            let word = Word {
                index: *global_word_index,
                text: w.to_string(),
                start_ms: word_start,
                end_ms: word_end,
                confidence: 1.0,
            };
            *global_word_index += 1;
            word
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- extract_audio --------------------------------------------------------

    #[test]
    fn extract_audio_rejects_nonexistent_source() {
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("out.wav");
        let err = extract_audio(Path::new("/nonexistent/video.mp4"), &output).unwrap_err();
        // ffmpeg will fail (either not found or failed on missing input)
        let msg = format!("{err}");
        assert!(
            msg.contains("ffmpeg") || msg.contains("No such file"),
            "expected ffmpeg-related error, got: {msg}"
        );
    }

    #[test]
    fn extract_audio_returns_output_path() {
        // Verify the function signature returns the output path on success.
        // This is a compile-time check; actual ffmpeg tests are in integration tests.
        let _: fn(&Path, &Path) -> Result<PathBuf, TranscriptError> = extract_audio;
    }

    // -- validate_model -------------------------------------------------------

    #[test]
    fn validate_model_accepts_all_valid() {
        for model in WHISPER_MODELS {
            assert!(validate_model(model).is_ok(), "should accept '{model}'");
        }
    }

    #[test]
    fn validate_model_rejects_invalid() {
        let err = validate_model("huge").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("huge"));
        assert!(msg.contains("tiny"));
    }

    #[test]
    fn validate_model_rejects_empty() {
        assert!(validate_model("").is_err());
    }

    // -- parse_progress -------------------------------------------------------

    #[test]
    fn parse_progress_extracts_percentages() {
        let stderr = "\
whisper_print_progress_callback: progress =   5%
whisper_print_progress_callback: progress =  42%
whisper_print_progress_callback: progress = 100%";

        let progress = parse_progress(stderr);
        assert_eq!(
            progress,
            vec![
                WhisperProgress { percent: 5 },
                WhisperProgress { percent: 42 },
                WhisperProgress { percent: 100 },
            ]
        );
    }

    #[test]
    fn parse_progress_handles_empty_stderr() {
        assert!(parse_progress("").is_empty());
    }

    #[test]
    fn parse_progress_ignores_non_progress_lines() {
        let stderr = "\
whisper_init_from_file: loading model
whisper_print_progress_callback: progress =  10%
some other output
whisper_print_progress_callback: progress =  50%";

        let progress = parse_progress(stderr);
        assert_eq!(progress.len(), 2);
        assert_eq!(progress[0].percent, 10);
        assert_eq!(progress[1].percent, 50);
    }

    // -- is_word_token --------------------------------------------------------

    #[test]
    fn is_word_token_accepts_words() {
        assert!(is_word_token(" Hello"));
        assert!(is_word_token("world"));
        assert!(is_word_token(" the"));
    }

    #[test]
    fn is_word_token_rejects_special_tokens() {
        assert!(!is_word_token("[_BEG_]"));
        assert!(!is_word_token("[_SOT_]"));
        assert!(!is_word_token("[_EOT_]"));
        assert!(!is_word_token("<|0.00|>"));
        assert!(!is_word_token("<|endoftext|>"));
        assert!(!is_word_token(""));
        assert!(!is_word_token("   "));
    }

    // -- parse_whisper_json ---------------------------------------------------

    #[test]
    fn parse_whisper_json_basic() {
        let json = r#"{
            "systeminfo": "test",
            "model": { "type": "base" },
            "params": {},
            "result": { "language": "en" },
            "transcription": [
                {
                    "timestamps": { "from": "00:00:00,000", "to": "00:00:05,230" },
                    "offsets": { "from": 0, "to": 5230 },
                    "text": " Welcome to the interview",
                    "tokens": [
                        {
                            "text": " Welcome",
                            "timestamps": { "from": "00:00:00,000", "to": "00:00:00,420" },
                            "offsets": { "from": 0, "to": 420 },
                            "id": 6529,
                            "p": 0.95
                        },
                        {
                            "text": " to",
                            "timestamps": { "from": "00:00:00,420", "to": "00:00:00,540" },
                            "offsets": { "from": 420, "to": 540 },
                            "id": 289,
                            "p": 0.97
                        },
                        {
                            "text": " the",
                            "timestamps": { "from": "00:00:00,540", "to": "00:00:00,800" },
                            "offsets": { "from": 540, "to": 800 },
                            "id": 264,
                            "p": 0.98
                        },
                        {
                            "text": " interview",
                            "timestamps": { "from": "00:00:00,800", "to": "00:00:05,230" },
                            "offsets": { "from": 800, "to": 5230 },
                            "id": 7768,
                            "p": 0.92
                        }
                    ]
                }
            ]
        }"#;

        let transcript = parse_whisper_json(json, "src-001", "base").unwrap();

        assert_eq!(transcript.source_id, "src-001");
        assert_eq!(transcript.model, "base");
        assert_eq!(transcript.language, "en");
        assert_eq!(transcript.duration_ms, 5230);
        assert_eq!(transcript.word_count, 4);
        assert_eq!(transcript.segments.len(), 1);

        let seg = &transcript.segments[0];
        assert_eq!(seg.index, 0);
        assert_eq!(seg.start_ms, 0);
        assert_eq!(seg.end_ms, 5230);
        assert_eq!(seg.text, "Welcome to the interview");
        assert_eq!(seg.words.len(), 4);

        assert_eq!(seg.words[0].index, 0);
        assert_eq!(seg.words[0].text, "Welcome");
        assert_eq!(seg.words[0].start_ms, 0);
        assert_eq!(seg.words[0].end_ms, 420);
        assert_eq!(seg.words[0].confidence, 0.95);

        assert_eq!(seg.words[3].index, 3);
        assert_eq!(seg.words[3].text, "interview");
    }

    #[test]
    fn parse_whisper_json_filters_special_tokens() {
        let json = r#"{
            "result": { "language": "en" },
            "transcription": [
                {
                    "offsets": { "from": 0, "to": 3000 },
                    "text": " Hello world",
                    "tokens": [
                        {
                            "text": "[_BEG_]",
                            "offsets": { "from": 0, "to": 0 },
                            "p": 0.0
                        },
                        {
                            "text": " Hello",
                            "offsets": { "from": 0, "to": 1500 },
                            "p": 0.90
                        },
                        {
                            "text": " world",
                            "offsets": { "from": 1500, "to": 3000 },
                            "p": 0.88
                        },
                        {
                            "text": "<|endoftext|>",
                            "offsets": { "from": 3000, "to": 3000 },
                            "p": 0.0
                        }
                    ]
                }
            ]
        }"#;

        let transcript = parse_whisper_json(json, "src-001", "base").unwrap();
        assert_eq!(transcript.word_count, 2);
        assert_eq!(transcript.segments[0].words.len(), 2);
        assert_eq!(transcript.segments[0].words[0].text, "Hello");
        assert_eq!(transcript.segments[0].words[1].text, "world");
    }

    #[test]
    fn parse_whisper_json_multiple_segments() {
        let json = r#"{
            "result": { "language": "en" },
            "transcription": [
                {
                    "offsets": { "from": 0, "to": 5000 },
                    "text": " First segment",
                    "tokens": [
                        { "text": " First", "offsets": { "from": 0, "to": 2500 }, "p": 0.95 },
                        { "text": " segment", "offsets": { "from": 2500, "to": 5000 }, "p": 0.90 }
                    ]
                },
                {
                    "offsets": { "from": 5000, "to": 10000 },
                    "text": " Second segment",
                    "tokens": [
                        { "text": " Second", "offsets": { "from": 5000, "to": 7500 }, "p": 0.93 },
                        { "text": " segment", "offsets": { "from": 7500, "to": 10000 }, "p": 0.91 }
                    ]
                }
            ]
        }"#;

        let transcript = parse_whisper_json(json, "src-002", "small").unwrap();

        assert_eq!(transcript.model, "small");
        assert_eq!(transcript.duration_ms, 10000);
        assert_eq!(transcript.word_count, 4);
        assert_eq!(transcript.segments.len(), 2);

        // Word indices are globally sequential across segments
        assert_eq!(transcript.segments[0].words[0].index, 0);
        assert_eq!(transcript.segments[0].words[1].index, 1);
        assert_eq!(transcript.segments[1].words[0].index, 2);
        assert_eq!(transcript.segments[1].words[1].index, 3);

        // Segment indices
        assert_eq!(transcript.segments[0].index, 0);
        assert_eq!(transcript.segments[1].index, 1);
    }

    #[test]
    fn parse_whisper_json_no_tokens() {
        let json = r#"{
            "result": { "language": "en" },
            "transcription": [
                {
                    "offsets": { "from": 0, "to": 5000 },
                    "text": " No token data available"
                }
            ]
        }"#;

        let transcript = parse_whisper_json(json, "src-001", "base").unwrap();

        assert_eq!(transcript.segments.len(), 1);
        assert_eq!(transcript.word_count, 0);
        assert!(transcript.segments[0].words.is_empty());
        assert_eq!(transcript.segments[0].text, "No token data available");
    }

    #[test]
    fn parse_whisper_json_empty_transcription() {
        let json = r#"{
            "result": { "language": "en" },
            "transcription": []
        }"#;

        let transcript = parse_whisper_json(json, "src-001", "base").unwrap();

        assert_eq!(transcript.segments.len(), 0);
        assert_eq!(transcript.word_count, 0);
        assert_eq!(transcript.duration_ms, 0);
    }

    #[test]
    fn parse_whisper_json_segment_with_only_special_tokens() {
        let json = r#"{
            "result": { "language": "en" },
            "transcription": [
                {
                    "offsets": { "from": 0, "to": 2000 },
                    "text": "",
                    "tokens": [
                        { "text": "[_BEG_]", "offsets": { "from": 0, "to": 0 }, "p": 0.0 },
                        { "text": "[_SOT_]", "offsets": { "from": 0, "to": 0 }, "p": 0.0 },
                        { "text": "<|0.00|>", "offsets": { "from": 0, "to": 0 }, "p": 0.0 },
                        { "text": "<|endoftext|>", "offsets": { "from": 2000, "to": 2000 }, "p": 0.0 }
                    ]
                },
                {
                    "offsets": { "from": 2000, "to": 5000 },
                    "text": " Real words here",
                    "tokens": [
                        { "text": " Real", "offsets": { "from": 2000, "to": 3000 }, "p": 0.85 },
                        { "text": " words", "offsets": { "from": 3000, "to": 4000 }, "p": 0.90 },
                        { "text": " here", "offsets": { "from": 4000, "to": 5000 }, "p": 0.88 }
                    ]
                }
            ]
        }"#;

        let transcript = parse_whisper_json(json, "src-001", "base").unwrap();

        assert_eq!(transcript.segments.len(), 2);
        assert_eq!(transcript.word_count, 3);

        // First segment has no words after filtering
        assert!(transcript.segments[0].words.is_empty());

        // Second segment words start at global index 0 (no words in first segment)
        assert_eq!(transcript.segments[1].words[0].index, 0);
        assert_eq!(transcript.segments[1].words[0].text, "Real");
        assert_eq!(transcript.segments[1].words[2].index, 2);
    }

    #[test]
    fn parse_whisper_json_trims_whitespace_from_words() {
        let json = r#"{
            "result": { "language": "en" },
            "transcription": [
                {
                    "offsets": { "from": 0, "to": 3000 },
                    "text": " Hello world",
                    "tokens": [
                        { "text": " Hello", "offsets": { "from": 0, "to": 1500 }, "p": 0.90 },
                        { "text": "  world ", "offsets": { "from": 1500, "to": 3000 }, "p": 0.88 }
                    ]
                }
            ]
        }"#;

        let transcript = parse_whisper_json(json, "src-001", "base").unwrap();

        assert_eq!(transcript.segments[0].words[0].text, "Hello");
        assert_eq!(transcript.segments[0].words[1].text, "world");
    }

    #[test]
    fn parse_whisper_json_preserves_confidence() {
        let json = r#"{
            "result": { "language": "fr" },
            "transcription": [
                {
                    "offsets": { "from": 0, "to": 2000 },
                    "text": " Bonjour monde",
                    "tokens": [
                        { "text": " Bonjour", "offsets": { "from": 0, "to": 1000 }, "p": 0.42 },
                        { "text": " monde", "offsets": { "from": 1000, "to": 2000 }, "p": 0.99 }
                    ]
                }
            ]
        }"#;

        let transcript = parse_whisper_json(json, "src-003", "large").unwrap();

        assert_eq!(transcript.language, "fr");
        assert_eq!(transcript.model, "large");
        assert_eq!(transcript.segments[0].words[0].confidence, 0.42);
        assert_eq!(transcript.segments[0].words[1].confidence, 0.99);
    }

    #[test]
    fn parse_whisper_json_invalid_json() {
        let result = parse_whisper_json("not valid json", "src-001", "base");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("parse whisper JSON"));
    }

    #[test]
    fn parse_whisper_json_duration_from_max_segment() {
        let json = r#"{
            "result": { "language": "en" },
            "transcription": [
                {
                    "offsets": { "from": 0, "to": 5000 },
                    "text": " A",
                    "tokens": [
                        { "text": " A", "offsets": { "from": 0, "to": 5000 }, "p": 0.9 }
                    ]
                },
                {
                    "offsets": { "from": 5000, "to": 15000 },
                    "text": " B",
                    "tokens": [
                        { "text": " B", "offsets": { "from": 5000, "to": 15000 }, "p": 0.9 }
                    ]
                },
                {
                    "offsets": { "from": 15000, "to": 12000 },
                    "text": " C",
                    "tokens": [
                        { "text": " C", "offsets": { "from": 15000, "to": 12000 }, "p": 0.9 }
                    ]
                }
            ]
        }"#;

        let transcript = parse_whisper_json(json, "src-001", "base").unwrap();
        // duration_ms should be the max segment end time (15000)
        assert_eq!(transcript.duration_ms, 15000);
    }

    #[test]
    fn parse_whisper_json_transcript_roundtrips_as_serde() {
        let json = r#"{
            "result": { "language": "en" },
            "transcription": [
                {
                    "offsets": { "from": 0, "to": 5000 },
                    "text": " Hello world",
                    "tokens": [
                        { "text": " Hello", "offsets": { "from": 0, "to": 2500 }, "p": 0.95 },
                        { "text": " world", "offsets": { "from": 2500, "to": 5000 }, "p": 0.90 }
                    ]
                }
            ]
        }"#;

        let transcript = parse_whisper_json(json, "src-001", "base").unwrap();

        // Serialize to JSON and deserialize back — round-trip fidelity
        let serialized = serde_json::to_string(&transcript).unwrap();
        let deserialized: crate::models::Transcript = serde_json::from_str(&serialized).unwrap();
        assert_eq!(transcript, deserialized);
    }

    // -- invoke_whisper (error paths) -----------------------------------------

    #[test]
    fn invoke_whisper_rejects_missing_audio() {
        let err = invoke_whisper(
            Path::new("/nonexistent/audio.wav"),
            Path::new("/nonexistent/model.bin"),
            "src-001",
        );
        assert!(err.is_err());
    }

    // -- find_model -----------------------------------------------------------

    #[test]
    fn find_model_rejects_invalid_model_name() {
        let err = find_model("huge").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("huge"));
    }

    // -- parse_srt ------------------------------------------------------------

    #[test]
    fn parse_srt_basic() {
        let srt = "\
1
00:00:00,000 --> 00:00:05,230
Welcome to the interview

2
00:00:05,230 --> 00:00:11,800
This is the second segment";

        let transcript = parse_srt(srt, "src-001").unwrap();

        assert_eq!(transcript.source_id, "src-001");
        assert_eq!(transcript.model, "imported");
        assert_eq!(transcript.language, "und");
        assert_eq!(transcript.duration_ms, 11800);
        assert_eq!(transcript.segments.len(), 2);
        assert_eq!(transcript.word_count, 9);

        let seg0 = &transcript.segments[0];
        assert_eq!(seg0.index, 0);
        assert_eq!(seg0.start_ms, 0);
        assert_eq!(seg0.end_ms, 5230);
        assert_eq!(seg0.text, "Welcome to the interview");
        assert_eq!(seg0.words.len(), 4);

        // Words have interpolated timestamps
        assert_eq!(seg0.words[0].index, 0);
        assert_eq!(seg0.words[0].text, "Welcome");
        assert_eq!(seg0.words[0].start_ms, 0);
        assert_eq!(seg0.words[0].confidence, 1.0);

        assert_eq!(seg0.words[3].index, 3);
        assert_eq!(seg0.words[3].text, "interview");

        let seg1 = &transcript.segments[1];
        assert_eq!(seg1.index, 1);
        assert_eq!(seg1.start_ms, 5230);
        assert_eq!(seg1.end_ms, 11800);
        assert_eq!(seg1.text, "This is the second segment");
        assert_eq!(seg1.words.len(), 5);

        // Global word indices continue from previous segment
        assert_eq!(seg1.words[0].index, 4);
        assert_eq!(seg1.words[4].index, 8);
    }

    #[test]
    fn parse_srt_multiline_text() {
        let srt = "\
1
00:00:00,000 --> 00:00:03,000
First line
Second line";

        let transcript = parse_srt(srt, "src-001").unwrap();

        assert_eq!(transcript.segments[0].text, "First line Second line");
        assert_eq!(transcript.word_count, 4);
    }

    #[test]
    fn parse_srt_invalid_no_timestamp() {
        let srt = "1\nNo timestamp here\n";
        let result = parse_srt(srt, "src-001");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("no timestamp line"));
    }

    #[test]
    fn parse_srt_invalid_timestamp_format() {
        let srt = "1\n00:00:00.000 --> 00:00:05.230\nHello";
        let result = parse_srt(srt, "src-001");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("missing comma"));
    }

    #[test]
    fn parse_srt_windows_line_endings() {
        let srt = "1\r\n00:00:00,000 --> 00:00:05,000\r\nHello world\r\n\r\n2\r\n00:00:05,000 --> 00:00:10,000\r\nGoodbye\r\n";

        let transcript = parse_srt(srt, "src-001").unwrap();
        assert_eq!(transcript.segments.len(), 2);
        assert_eq!(transcript.word_count, 3);
    }

    #[test]
    fn parse_srt_with_hours() {
        let srt = "\
1
01:30:00,500 --> 01:30:05,000
Long video segment";

        let transcript = parse_srt(srt, "src-001").unwrap();
        assert_eq!(transcript.segments[0].start_ms, 5_400_500);
        assert_eq!(transcript.segments[0].end_ms, 5_405_000);
    }

    // -- parse_vtt ------------------------------------------------------------

    #[test]
    fn parse_vtt_basic() {
        let vtt = "\
WEBVTT

00:00:00.000 --> 00:00:05.230
Welcome to the interview

00:00:05.230 --> 00:00:11.800
This is the second segment";

        let transcript = parse_vtt(vtt, "src-001").unwrap();

        assert_eq!(transcript.source_id, "src-001");
        assert_eq!(transcript.model, "imported");
        assert_eq!(transcript.language, "und");
        assert_eq!(transcript.duration_ms, 11800);
        assert_eq!(transcript.segments.len(), 2);
        assert_eq!(transcript.word_count, 9);

        assert_eq!(transcript.segments[0].text, "Welcome to the interview");
        assert_eq!(transcript.segments[1].text, "This is the second segment");
    }

    #[test]
    fn parse_vtt_missing_header() {
        let vtt = "00:00:00.000 --> 00:00:05.000\nHello";
        let result = parse_vtt(vtt, "src-001");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("WEBVTT"));
    }

    #[test]
    fn parse_vtt_with_cue_identifiers() {
        let vtt = "\
WEBVTT

intro
00:00:00.000 --> 00:00:05.000
Hello world

outro
00:00:05.000 --> 00:00:10.000
Goodbye";

        let transcript = parse_vtt(vtt, "src-001").unwrap();
        assert_eq!(transcript.segments.len(), 2);
        assert_eq!(transcript.segments[0].text, "Hello world");
        assert_eq!(transcript.segments[1].text, "Goodbye");
    }

    #[test]
    fn parse_vtt_short_timestamps() {
        // VTT allows MM:SS.mmm format (no hours)
        let vtt = "\
WEBVTT

00:05.000 --> 00:10.000
Short format";

        let transcript = parse_vtt(vtt, "src-001").unwrap();
        assert_eq!(transcript.segments[0].start_ms, 5000);
        assert_eq!(transcript.segments[0].end_ms, 10000);
    }

    #[test]
    fn parse_vtt_with_positioning() {
        let vtt = "\
WEBVTT

00:00:00.000 --> 00:00:05.000 position:10% line:0
Hello world";

        let transcript = parse_vtt(vtt, "src-001").unwrap();
        assert_eq!(transcript.segments[0].start_ms, 0);
        assert_eq!(transcript.segments[0].end_ms, 5000);
        assert_eq!(transcript.segments[0].text, "Hello world");
    }

    #[test]
    fn parse_vtt_skips_note_blocks() {
        let vtt = "\
WEBVTT

NOTE This is a comment

00:00:00.000 --> 00:00:05.000
Hello world";

        let transcript = parse_vtt(vtt, "src-001").unwrap();
        assert_eq!(transcript.segments.len(), 1);
        assert_eq!(transcript.segments[0].text, "Hello world");
        assert_eq!(transcript.segments[0].index, 0);
    }

    #[test]
    fn parse_vtt_with_bom() {
        let vtt = "\u{feff}WEBVTT\n\n00:00:00.000 --> 00:00:05.000\nHello";

        let transcript = parse_vtt(vtt, "src-001").unwrap();
        assert_eq!(transcript.segments.len(), 1);
        assert_eq!(transcript.segments[0].text, "Hello");
    }

    // -- interpolate_words ----------------------------------------------------

    #[test]
    fn interpolate_words_even_distribution() {
        let mut idx = 0;
        let words = interpolate_words("one two three four", 0, 4000, &mut idx);

        assert_eq!(words.len(), 4);
        assert_eq!(idx, 4);

        assert_eq!(words[0].start_ms, 0);
        assert_eq!(words[0].end_ms, 1000);
        assert_eq!(words[1].start_ms, 1000);
        assert_eq!(words[1].end_ms, 2000);
        assert_eq!(words[2].start_ms, 2000);
        assert_eq!(words[2].end_ms, 3000);
        assert_eq!(words[3].start_ms, 3000);
        assert_eq!(words[3].end_ms, 4000);
    }

    #[test]
    fn interpolate_words_empty_text() {
        let mut idx = 5;
        let words = interpolate_words("", 0, 1000, &mut idx);
        assert!(words.is_empty());
        assert_eq!(idx, 5);
    }

    #[test]
    fn interpolate_words_single_word() {
        let mut idx = 0;
        let words = interpolate_words("hello", 100, 500, &mut idx);

        assert_eq!(words.len(), 1);
        assert_eq!(words[0].start_ms, 100);
        assert_eq!(words[0].end_ms, 500);
        assert_eq!(words[0].confidence, 1.0);
    }

    // -- import_transcript (file-based) ---------------------------------------

    #[test]
    fn import_transcript_srt_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.srt");
        std::fs::write(
            &path,
            "1\n00:00:00,000 --> 00:00:03,000\nHello world\n",
        )
        .unwrap();

        let transcript = import_transcript(&path, "src-001").unwrap();
        assert_eq!(transcript.segments.len(), 1);
        assert_eq!(transcript.word_count, 2);
    }

    #[test]
    fn import_transcript_vtt_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.vtt");
        std::fs::write(
            &path,
            "WEBVTT\n\n00:00:00.000 --> 00:00:03.000\nHello world\n",
        )
        .unwrap();

        let transcript = import_transcript(&path, "src-001").unwrap();
        assert_eq!(transcript.segments.len(), 1);
        assert_eq!(transcript.word_count, 2);
    }

    #[test]
    fn import_transcript_json_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.json");
        std::fs::write(
            &path,
            r#"{"result":{"language":"en"},"transcription":[{"offsets":{"from":0,"to":3000},"text":" Hello world","tokens":[{"text":" Hello","offsets":{"from":0,"to":1500},"p":0.9},{"text":" world","offsets":{"from":1500,"to":3000},"p":0.85}]}]}"#,
        )
        .unwrap();

        let transcript = import_transcript(&path, "src-001").unwrap();
        assert_eq!(transcript.model, "imported");
        assert_eq!(transcript.language, "en");
        assert_eq!(transcript.word_count, 2);
        // JSON preserves original confidence
        assert_eq!(transcript.segments[0].words[0].confidence, 0.9);
    }

    #[test]
    fn import_transcript_unsupported_format() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.ass");
        std::fs::write(&path, "some content").unwrap();

        let result = import_transcript(&path, "src-001");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("unsupported"));
    }

    // -- SRT/VTT round-trip through serde ------------------------------------

    #[test]
    fn srt_transcript_roundtrips_as_serde() {
        let srt = "\
1
00:00:00,000 --> 00:00:05,230
Welcome to the interview

2
00:00:05,230 --> 00:00:11,800
This is the second segment";

        let transcript = parse_srt(srt, "src-001").unwrap();

        let serialized = serde_json::to_string(&transcript).unwrap();
        let deserialized: crate::models::Transcript = serde_json::from_str(&serialized).unwrap();
        assert_eq!(transcript, deserialized);
    }

    #[test]
    fn vtt_transcript_roundtrips_as_serde() {
        let vtt = "\
WEBVTT

00:00:00.000 --> 00:00:05.230
Welcome to the interview

00:00:05.230 --> 00:00:11.800
This is the second segment";

        let transcript = parse_vtt(vtt, "src-001").unwrap();

        let serialized = serde_json::to_string(&transcript).unwrap();
        let deserialized: crate::models::Transcript = serde_json::from_str(&serialized).unwrap();
        assert_eq!(transcript, deserialized);
    }
}
