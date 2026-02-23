//! TEST-026: Render format options (REQ-026, CON-007)
//!
//! Verifies codec selection, resolution parsing, encoder mapping, and the
//! stream-copy-vs-re-encode decision logic. These are unit-level tests of
//! the render pipeline's format handling — actual ffmpeg invocation is not
//! tested here (requires real video files).

use ar_edit_core::render::{self, RenderOptions};

// ---------------------------------------------------------------------------
// Tests: Resolution parsing
// ---------------------------------------------------------------------------

#[test]
fn parse_resolution_1080p() {
    assert_eq!(render::parse_resolution("1920x1080").unwrap(), (1920, 1080));
}

#[test]
fn parse_resolution_4k() {
    assert_eq!(render::parse_resolution("3840x2160").unwrap(), (3840, 2160));
}

#[test]
fn parse_resolution_720p() {
    assert_eq!(render::parse_resolution("1280x720").unwrap(), (1280, 720));
}

#[test]
fn parse_resolution_square() {
    assert_eq!(render::parse_resolution("1080x1080").unwrap(), (1080, 1080));
}

#[test]
fn parse_resolution_small() {
    assert_eq!(render::parse_resolution("640x480").unwrap(), (640, 480));
}

#[test]
fn parse_resolution_rejects_colon_separator() {
    assert!(render::parse_resolution("1920:1080").is_err());
}

#[test]
fn parse_resolution_rejects_single_number() {
    assert!(render::parse_resolution("1920").is_err());
}

#[test]
fn parse_resolution_rejects_letters() {
    assert!(render::parse_resolution("widexhigh").is_err());
}

#[test]
fn parse_resolution_rejects_empty() {
    assert!(render::parse_resolution("").is_err());
}

#[test]
fn parse_resolution_rejects_zero_width() {
    assert!(render::parse_resolution("0x1080").is_err());
}

#[test]
fn parse_resolution_rejects_zero_height() {
    assert!(render::parse_resolution("1920x0").is_err());
}

#[test]
fn parse_resolution_rejects_negative() {
    assert!(render::parse_resolution("-1920x1080").is_err());
}

#[test]
fn parse_resolution_rejects_triple() {
    assert!(render::parse_resolution("1920x1080x60").is_err());
}

// ---------------------------------------------------------------------------
// Tests: Codec normalisation
// ---------------------------------------------------------------------------

#[test]
fn normalize_h264_aliases() {
    assert_eq!(render::normalize_codec("h264"), "h264");
    assert_eq!(render::normalize_codec("avc"), "h264");
    assert_eq!(render::normalize_codec("libx264"), "h264");
}

#[test]
fn normalize_h265_aliases() {
    assert_eq!(render::normalize_codec("h265"), "h265");
    assert_eq!(render::normalize_codec("hevc"), "h265");
    assert_eq!(render::normalize_codec("libx265"), "h265");
}

#[test]
fn normalize_vp9_aliases() {
    assert_eq!(render::normalize_codec("vp9"), "vp9");
    assert_eq!(render::normalize_codec("libvpx-vp9"), "vp9");
}

#[test]
fn normalize_av1_aliases() {
    assert_eq!(render::normalize_codec("av1"), "av1");
    assert_eq!(render::normalize_codec("libaom-av1"), "av1");
    assert_eq!(render::normalize_codec("libsvtav1"), "av1");
}

#[test]
fn normalize_unknown_codec_passthrough() {
    assert_eq!(render::normalize_codec("prores"), "prores");
    assert_eq!(render::normalize_codec("mjpeg"), "mjpeg");
    assert_eq!(render::normalize_codec("dnxhd"), "dnxhd");
}

// ---------------------------------------------------------------------------
// Tests: FFmpeg encoder name mapping
// ---------------------------------------------------------------------------

#[test]
fn encoder_h264() {
    assert_eq!(render::ffmpeg_video_encoder("h264"), "libx264");
}

#[test]
fn encoder_h264_from_alias() {
    assert_eq!(render::ffmpeg_video_encoder("avc"), "libx264");
}

#[test]
fn encoder_h265() {
    assert_eq!(render::ffmpeg_video_encoder("h265"), "libx265");
}

#[test]
fn encoder_h265_from_alias() {
    assert_eq!(render::ffmpeg_video_encoder("hevc"), "libx265");
}

#[test]
fn encoder_vp9() {
    assert_eq!(render::ffmpeg_video_encoder("vp9"), "libvpx-vp9");
}

#[test]
fn encoder_av1() {
    assert_eq!(render::ffmpeg_video_encoder("av1"), "libsvtav1");
}

#[test]
fn encoder_unknown_passthrough() {
    assert_eq!(render::ffmpeg_video_encoder("prores"), "prores");
}

// ---------------------------------------------------------------------------
// Tests: RenderOptions construction
// ---------------------------------------------------------------------------

#[test]
fn render_options_default_is_none() {
    let opts = RenderOptions::default();
    assert!(opts.video_codec.is_none());
    assert!(opts.resolution.is_none());
}

#[test]
fn render_options_with_codec() {
    let opts = RenderOptions {
        video_codec: Some("h265".into()),
        ..Default::default()
    };
    assert_eq!(opts.video_codec.as_deref(), Some("h265"));
    assert!(opts.resolution.is_none());
}

#[test]
fn render_options_with_resolution() {
    let opts = RenderOptions {
        resolution: Some((1280, 720)),
        ..Default::default()
    };
    assert!(opts.video_codec.is_none());
    assert_eq!(opts.resolution, Some((1280, 720)));
}

#[test]
fn render_options_with_both() {
    let opts = RenderOptions {
        video_codec: Some("h264".into()),
        resolution: Some((1920, 1080)),
        ..Default::default()
    };
    assert_eq!(opts.video_codec.as_deref(), Some("h264"));
    assert_eq!(opts.resolution, Some((1920, 1080)));
}

// ---------------------------------------------------------------------------
// Tests: Codec comparison (stream copy decision)
// ---------------------------------------------------------------------------

/// When source and target codecs normalise to the same value, stream copy
/// should be used (no re-encode needed).
#[test]
fn same_codec_detected_across_aliases() {
    // All these refer to the same codec
    let aliases = ["h264", "avc", "libx264"];
    for a in &aliases {
        for b in &aliases {
            assert_eq!(
                render::normalize_codec(a),
                render::normalize_codec(b),
                "{a} and {b} should normalise to the same codec"
            );
        }
    }
}

/// When source and target codecs differ, re-encoding is needed.
#[test]
fn different_codecs_detected() {
    assert_ne!(
        render::normalize_codec("h264"),
        render::normalize_codec("h265")
    );
    assert_ne!(
        render::normalize_codec("h264"),
        render::normalize_codec("vp9")
    );
    assert_ne!(
        render::normalize_codec("h265"),
        render::normalize_codec("av1")
    );
}

// ---------------------------------------------------------------------------
// Tests: render_to_file signature accepts RenderOptions
// ---------------------------------------------------------------------------

#[test]
fn render_to_file_with_options_empty_edit() {
    use ar_edit_core::models::EditDocument;
    use ar_edit_core::overlay::OverlayMode;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();

    let doc = EditDocument::create("test");
    let output = tmp.path().join("output.mp4");

    // With default options
    let result = render::render_to_file(
        &doc,
        tmp.path(),
        &output,
        OverlayMode::Clean,
        &RenderOptions::default(),
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no shots"));
}

#[test]
fn render_to_file_with_codec_option_empty_edit() {
    use ar_edit_core::models::EditDocument;
    use ar_edit_core::overlay::OverlayMode;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("transcripts")).unwrap();
    std::fs::create_dir_all(tmp.path().join("index")).unwrap();

    let doc = EditDocument::create("test");
    let output = tmp.path().join("output.mp4");

    let opts = RenderOptions {
        video_codec: Some("h265".into()),
        resolution: Some((1280, 720)),
        ..Default::default()
    };
    let result = render::render_to_file(&doc, tmp.path(), &output, OverlayMode::Clean, &opts);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no shots"));
}

// ---------------------------------------------------------------------------
// Tests: CLI integration (parse → options roundtrip)
// ---------------------------------------------------------------------------

/// Verify that resolution strings round-trip through parse_resolution.
#[test]
fn resolution_roundtrip() {
    let cases = [
        ("1920x1080", (1920u32, 1080u32)),
        ("1280x720", (1280, 720)),
        ("3840x2160", (3840, 2160)),
        ("640x480", (640, 480)),
    ];
    for (input, expected) in &cases {
        let parsed = render::parse_resolution(input).unwrap();
        assert_eq!(parsed, *expected, "failed for input: {input}");
    }
}
