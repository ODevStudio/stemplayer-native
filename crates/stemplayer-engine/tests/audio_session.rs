use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use stemplayer_decode::{CompressedTrackDecoder, inspect_audio_file};
use stemplayer_domain::{ErrorCode, GenerationId};
use stemplayer_engine::{AnalysisDecoder, PlaybackSession};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "stemplayer-engine-audio-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn audio_session_retains_the_inspected_file_identity_until_decode() {
    let scratch = Scratch::new();
    let bytes = include_bytes!("../../stemplayer-decode/tests/fixtures/mono.wav");
    let path = scratch.write("audio.wav", bytes);
    let session = PlaybackSession::open_audio(&path, GenerationId::INITIAL).unwrap();
    fs::rename(&path, scratch.0.join("original.wav")).unwrap();
    scratch.write("audio.wav", bytes);
    let mut decoder = AnalysisDecoder::new(&session, Arc::new(AtomicBool::new(false)));
    assert_eq!(
        decoder.decode(0, 1).unwrap_err().code,
        ErrorCode::BundleInconsistent
    );
}

#[test]
fn audio_inspection_keeps_the_callers_cancellation() {
    let path = fixture("mono.wav");
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        PlaybackSession::open_audio_cancelled(&path, GenerationId::INITIAL, &cancelled)
            .err()
            .unwrap()
            .code,
        ErrorCode::BundleAborted
    );
    assert!(cancelled.load(Ordering::Acquire));
}

#[test]
fn ordinary_m4a_session_uses_presentation_duration_and_maps_pcm_once() {
    for name in ["stereo-aac.m4a", "stereo-alac.m4a"] {
        let path = fixture(name);
        let cancelled = Arc::new(AtomicBool::new(false));
        let info = inspect_audio_file(&path).unwrap();
        let session =
            PlaybackSession::open_audio_cancelled(&path, GenerationId::INITIAL, &cancelled)
                .unwrap();
        assert_eq!(
            session.metadata().end_frame_exclusive.get() as u64,
            info.declared_frames.unwrap()
        );
        assert_eq!(
            session.metadata().end_frame_exclusive,
            info.timeline.as_ref().unwrap().end_frame_exclusive
        );
        let mut expected =
            CompressedTrackDecoder::open_audio_inspected(&path, &info, Arc::clone(&cancelled))
                .unwrap();
        let expected = expected.decode_window(0, 128, &cancelled).unwrap();
        let mut decoder = AnalysisDecoder::new(&session, Arc::clone(&cancelled));
        let actual = decoder.decode(0, 128).unwrap();
        assert_eq!(actual.frames, 128);
        assert_eq!(actual.channels, 8);
        for (index, frame) in actual.samples.chunks_exact(8).enumerate() {
            assert_eq!(frame[0], expected.left[index]);
            assert_eq!(frame[1], expected.right[index]);
            assert_eq!(frame[2..], [0.0; 6]);
        }
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../stemplayer-decode/tests/fixtures")
        .join(name)
}
