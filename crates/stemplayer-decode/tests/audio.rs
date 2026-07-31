use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use stemplayer_decode::{CompressedTrackDecoder, inspect_audio_file};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn supported_audio_decodes_deterministically_and_seeks() {
    for name in [
        "mono.wav",
        "stereo-float.wav",
        "stereo.mp3",
        "stereo.flac",
        "stereo.ogg",
        "stereo-aac.m4a",
        "stereo-alac.m4a",
    ] {
        let path = fixture(name);
        let info = inspect_audio_file(&path).unwrap();
        let cancelled = AtomicBool::new(false);
        let mut decoder = CompressedTrackDecoder::open_audio(&path, info.track_id).unwrap();
        let first = decoder
            .decode_window(0, 256, &cancelled)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        let adjacent = decoder.decode_window(256, 128, &cancelled).unwrap();
        let fresh = CompressedTrackDecoder::open_audio(&path, info.track_id)
            .unwrap()
            .decode_window(256, 128, &cancelled)
            .unwrap();
        assert_eq!(adjacent.left, fresh.left, "{name}");
        assert_eq!(adjacent.right, fresh.right, "{name}");
        let seek_frame = info.declared_frames.unwrap() / 2;
        decoder.decode_window(seek_frame, 128, &cancelled).unwrap();
        decoder.decode_window(seek_frame, 128, &cancelled).unwrap();
        decoder
            .decode_window(info.declared_frames.unwrap() - 128, 128, &cancelled)
            .unwrap();
        assert!(
            first
                .left
                .iter()
                .chain(&first.right)
                .all(|value| value.is_finite())
        );
    }
}

#[test]
fn mono_is_duplicated_and_stereo_is_preserved() {
    let cancelled = AtomicBool::new(false);
    let mono = fixture("mono.wav");
    let mono_info = inspect_audio_file(&mono).unwrap();
    let mono_window = CompressedTrackDecoder::open_audio(&mono, mono_info.track_id)
        .unwrap()
        .decode_window(0, 512, &cancelled)
        .unwrap();
    assert_eq!(mono_window.left, mono_window.right);

    let stereo = fixture("stereo-float.wav");
    let stereo_info = inspect_audio_file(&stereo).unwrap();
    let stereo_window = CompressedTrackDecoder::open_audio(&stereo, stereo_info.track_id)
        .unwrap()
        .decode_window(0, 512, &cancelled)
        .unwrap();
    assert!(
        stereo_window
            .left
            .iter()
            .zip(&stereo_window.right)
            .any(|(left, right)| left != right)
    );
}

#[test]
fn invalid_tracks_duration_cancellation_and_replacement_are_rejected() {
    for name in ["corrupt.mp3", "multichannel.wav", "multi-track.m4a"] {
        assert!(inspect_audio_file(&fixture(name)).is_err(), "{name}");
    }

    let source = fixture("stereo.flac");
    let info = inspect_audio_file(&source).unwrap();
    let mut decoder = CompressedTrackDecoder::open_audio(&source, info.track_id).unwrap();
    assert!(
        decoder
            .decode_window(0, 64, &AtomicBool::new(true))
            .is_err()
    );

    let target = std::env::temp_dir().join(format!(
        "stemplayer-audio-replacement-{}-{}.flac",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::copy(source, &target).unwrap();
    let info = inspect_audio_file(&target).unwrap();
    let mut decoder = CompressedTrackDecoder::open_audio(&target, info.track_id).unwrap();
    let mut bytes = std::fs::read(&target).unwrap();
    bytes.push(0);
    std::fs::write(&target, bytes).unwrap();
    assert!(
        decoder
            .decode_window(0, 64, &AtomicBool::new(false))
            .is_err()
    );
    std::fs::remove_file(target).unwrap();
}
