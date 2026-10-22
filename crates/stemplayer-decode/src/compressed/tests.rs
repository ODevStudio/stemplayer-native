use super::*;
use std::fs;
use symphonia::core::audio::{Channels, Position};

#[test]
fn mp3_terminal_padding_is_one_bounded_final_frame() {
    assert!(accepts_mp3_terminal_padding(
        CompressedCodec::Mp3,
        Some(10_000),
        8_848,
        10_000
    ));
    assert!(!accepts_mp3_terminal_padding(
        CompressedCodec::Mp3,
        Some(10_000),
        8_847,
        10_000
    ));
    assert!(!accepts_mp3_terminal_padding(
        CompressedCodec::Flac,
        Some(10_000),
        8_848,
        10_000
    ));
    assert!(!accepts_mp3_terminal_padding(
        CompressedCodec::Mp3,
        Some(9_999),
        8_848,
        10_000
    ));
}

#[test]
fn mp3_decode_concealment_is_bounded_to_two_seconds() {
    assert!(accepts_mp3_decode_concealment(
        CompressedCodec::Mp3,
        44_100,
        87_048,
        1_152
    ));
    assert!(!accepts_mp3_decode_concealment(
        CompressedCodec::Mp3,
        44_100,
        87_049,
        1_152
    ));
    assert!(!accepts_mp3_decode_concealment(
        CompressedCodec::Flac,
        44_100,
        0,
        1_152
    ));
    assert!(!accepts_mp3_decode_concealment(
        CompressedCodec::Mp3,
        44_100,
        0,
        MAX_DECODED_PACKET_FRAMES + 1
    ));
}

#[test]
fn codec_profile_rate_channels_and_bounds_are_enforced() {
    let mut aac = AudioCodecParameters::new();
    aac.codec = CODEC_ID_AAC;
    aac.profile = Some(CODEC_PROFILE_AAC_LC);
    aac.sample_rate = Some(48_000);
    aac.channels = Some(Channels::Positioned(
        Position::FRONT_LEFT | Position::FRONT_RIGHT,
    ));
    assert_eq!(
        validate_parameters(7, Some(48_000), &aac, false)
            .unwrap()
            .codec,
        CompressedCodec::AacLc
    );

    aac.profile = None;
    assert_eq!(
        validate_parameters(7, None, &aac, false).unwrap_err().code,
        ErrorCode::CodecUnsupported
    );
    aac.profile = Some(CODEC_PROFILE_AAC_LC);
    aac.sample_rate = Some(96_000);
    assert_eq!(
        validate_parameters(7, None, &aac, false).unwrap_err().code,
        ErrorCode::CodecConfigInvalid
    );

    let mut alac = AudioCodecParameters::new();
    alac.codec = CODEC_ID_ALAC;
    alac.sample_rate = Some(48_000);
    alac.channels = aac.channels.clone();
    alac.bits_per_sample = Some(24);
    alac.extra_data = Some(vec![0_u8; 24].into_boxed_slice());
    assert_eq!(
        validate_parameters(8, None, &alac, false).unwrap().codec,
        CompressedCodec::Alac
    );
    alac.bits_per_sample = None;
    alac.extra_data.as_mut().unwrap()[5] = 20;
    assert_eq!(
        validate_parameters(8, None, &alac, false)
            .unwrap()
            .bits_per_sample,
        Some(20)
    );
    alac.bits_per_sample = Some(12);
    assert_eq!(
        validate_parameters(8, None, &alac, false).unwrap_err().code,
        ErrorCode::CodecConfigInvalid
    );

    let mut flac = AudioCodecParameters::new();
    flac.codec = CODEC_ID_FLAC;
    flac.sample_rate = Some(192_000);
    flac.channels = aac.channels;
    assert_eq!(
        validate_parameters(9, None, &flac, true)
            .unwrap()
            .sample_rate,
        192_000
    );
    flac.sample_rate = Some(384_001);
    assert_eq!(
        validate_parameters(9, None, &flac, true).unwrap_err().code,
        ErrorCode::CodecConfigInvalid
    );
}

#[test]
fn ordinary_audio_requires_a_known_bounded_duration() {
    assert_eq!(
        bounded_audio_frames(None).unwrap_err().code,
        ErrorCode::TimelineInvalid
    );
    assert_eq!(
        bounded_audio_frames(Some(i64::MAX as u64 + 1))
            .unwrap_err()
            .code,
        ErrorCode::TimelineInvalid
    );
}

#[test]
fn a_failed_window_invalidates_the_sequential_cursor_before_retry() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stereo.flac");
    let info = inspect_audio_file(&path).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut decoder =
        CompressedTrackDecoder::open_audio_inspected(&path, &info, Arc::clone(&cancelled)).unwrap();
    decoder.decode_window(0, 64, &cancelled).unwrap();
    decoder.pending.as_mut().unwrap().trim_start = u64::MAX;
    assert!(decoder.decode_window(64, 64, &cancelled).is_err());
    assert_eq!(decoder.next_frame, None);
    let retried = decoder.decode_window(64, 64, &cancelled).unwrap();
    let expected =
        CompressedTrackDecoder::open_audio_inspected(&path, &info, Arc::clone(&cancelled))
            .unwrap()
            .decode_window(64, 64, &cancelled)
            .unwrap();
    assert_eq!(retried.left, expected.left);
    assert_eq!(retried.right, expected.right);
}

#[test]
fn presentation_windows_cross_explicit_silence_and_media_edits() {
    use stemplayer_timeline::PresentationSegment;

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stereo.flac");
    let info = inspect_audio_file(&path).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut decoder =
        CompressedTrackDecoder::open_audio_inspected(&path, &info, Arc::clone(&cancelled)).unwrap();
    let mut reference =
        CompressedTrackDecoder::open_audio_inspected(&path, &info, Arc::clone(&cancelled)).unwrap();
    decoder.info.timeline = Some(Mp4TrackTimeline {
        start_frame: SourceFrame::new(0),
        end_frame_exclusive: SourceFrame::new(48),
        segments: vec![
            PresentationSegment {
                start_frame: SourceFrame::new(0),
                end_frame_exclusive: SourceFrame::new(16),
                media_start_frame: None,
            },
            PresentationSegment {
                start_frame: SourceFrame::new(16),
                end_frame_exclusive: SourceFrame::new(32),
                media_start_frame: Some(SourceFrame::new(32)),
            },
            PresentationSegment {
                start_frame: SourceFrame::new(32),
                end_frame_exclusive: SourceFrame::new(48),
                media_start_frame: Some(SourceFrame::new(96)),
            },
        ],
        priming_resolved: true,
        padding_resolved: true,
    });
    decoder.info.declared_frames = Some(48);
    let actual = decoder.decode_window(8, 40, &cancelled).unwrap();
    let first = reference.decode_window(32, 16, &cancelled).unwrap();
    let last = reference.decode_window(96, 16, &cancelled).unwrap();
    assert_eq!(&actual.left[..8], &[0.0; 8]);
    assert_eq!(&actual.right[..8], &[0.0; 8]);
    assert_eq!(&actual.left[8..24], &first.left);
    assert_eq!(&actual.right[8..24], &first.right);
    assert_eq!(&actual.left[24..], &last.left);
    assert_eq!(&actual.right[24..], &last.right);
}

#[test]
fn local_file_identity_detects_replacement() {
    let path = std::env::temp_dir().join(format!(
        "stemplayer-compressed-identity-{}",
        std::process::id()
    ));
    fs::write(&path, b"first").unwrap();
    let (_, identity) = FileIdentity::open(&path).unwrap();
    fs::write(&path, b"replacement").unwrap();
    assert_ne!(FileIdentity::open(&path).unwrap().1, identity);
    fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires STEMPLAYER_NATIVE_CONTAINER_FIXTURE"]
fn local_container_tracks_decode_exact_windows() {
    let path = std::env::var("STEMPLAYER_NATIVE_CONTAINER_FIXTURE").unwrap();
    let container =
        inspect_compressed_container(Path::new(&path), &AtomicBool::new(false)).unwrap();
    assert!(container.end_frame_exclusive > container.start_frame);
    let tracks = inspect_compressed_tracks(Path::new(&path)).unwrap();
    assert!(matches!(tracks.len(), 4 | 5));
    for track in tracks {
        let mut decoder = CompressedTrackDecoder::open(Path::new(&path), track.track_id).unwrap();
        let first = decoder
            .decode_window(0, MAX_DECODE_FRAMES, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(first.left.len(), MAX_DECODE_FRAMES as usize);
        let middle = track.declared_frames.unwrap() / 2;
        let window = decoder
            .decode_window(middle, MAX_DECODE_FRAMES, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(window.left.len(), MAX_DECODE_FRAMES as usize);
        assert_eq!(window.left.len(), window.right.len());
        let safe_end = track
            .declared_frames
            .unwrap()
            .saturating_sub(MAX_DECODE_FRAMES as u64);
        let near_end = decoder
            .decode_window(safe_end, MAX_DECODE_FRAMES, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(near_end.left.len(), MAX_DECODE_FRAMES as usize);
        let adjacent_start = MAX_DECODE_FRAMES as u64;
        let mut sequential =
            CompressedTrackDecoder::open(Path::new(&path), track.track_id).unwrap();
        sequential
            .decode_window(0, MAX_DECODE_FRAMES, &AtomicBool::new(false))
            .unwrap();
        let adjacent = sequential
            .decode_window(adjacent_start, MAX_DECODE_FRAMES, &AtomicBool::new(false))
            .unwrap();
        let mut seeked = CompressedTrackDecoder::open(Path::new(&path), track.track_id).unwrap();
        let repeated = seeked
            .decode_window(adjacent_start, MAX_DECODE_FRAMES, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(adjacent.left, repeated.left);
        assert_eq!(adjacent.right, repeated.right);
        assert_eq!(
            decoder
                .decode_window(0, 1, &AtomicBool::new(true))
                .unwrap_err()
                .code,
            ErrorCode::BundleAborted
        );
        let mut seed = 0x9e37_79b9_u64;
        for _ in 0..16 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let start = seed % safe_end.max(1);
            let first = decoder
                .decode_window(start, MAX_DECODE_FRAMES, &AtomicBool::new(false))
                .unwrap();
            let repeat = decoder
                .decode_window(start, MAX_DECODE_FRAMES, &AtomicBool::new(false))
                .unwrap();
            assert_eq!(first.left, repeat.left);
            assert_eq!(first.right, repeat.right);
        }
    }
}
