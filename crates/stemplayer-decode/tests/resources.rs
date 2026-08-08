use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use stemplayer_decode::{StudioTrackDecoder, decode_window};
use stemplayer_domain::ErrorCode;
use stemplayer_format::{SelectedFile, SourceGroup, StudioBundle, StudioRole};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "stemplayer-phase4-resource-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn inspection_and_decode_reads_do_not_scale_with_duration() {
    let short = sparse_bundle(4096);
    let long_frames = 48_000_u64 * 60 * 60;
    let long = sparse_bundle(long_frames);
    let cancelled = AtomicBool::new(false);
    let short_bundle = StudioBundle::inspect(selected(&short.0), false, &cancelled).unwrap();
    let long_bundle = StudioBundle::inspect(selected(&long.0), false, &cancelled).unwrap();

    let short_track_reads = short_bundle
        .report()
        .tracks
        .iter()
        .map(|track| track.bytes_read)
        .collect::<Vec<_>>();
    let long_track_reads = long_bundle
        .report()
        .tracks
        .iter()
        .map(|track| track.bytes_read)
        .collect::<Vec<_>>();
    assert_eq!(short_track_reads, long_track_reads);

    let short_window =
        decode_window(&short_bundle, StudioRole::Drums, 0, 4096, &cancelled).unwrap();
    let long_window = decode_window(
        &long_bundle,
        StudioRole::Drums,
        long_frames - 4096,
        4096,
        &cancelled,
    )
    .unwrap();
    assert_eq!(short_window.bytes_read, 4096 * 4);
    assert_eq!(long_window.bytes_read, 4096 * 4);
    assert_eq!(short_window.left.len(), 4096);
    assert_eq!(long_window.left.len(), 4096);

    assert_eq!(
        decode_window(
            &short_bundle,
            StudioRole::Drums,
            0,
            1,
            &AtomicBool::new(true),
        )
        .err()
        .unwrap()
        .code,
        ErrorCode::BundleAborted
    );

    OpenOptions::new()
        .append(true)
        .open(long.0.join("drums.wav"))
        .unwrap()
        .write_all(&[0])
        .unwrap();
    assert_eq!(
        decode_window(&long_bundle, StudioRole::Drums, 0, 1, &cancelled,)
            .err()
            .unwrap()
            .code,
        ErrorCode::BundleInconsistent
    );
}

#[test]
fn studio_decoder_reuses_reader_across_bounded_windows() {
    let source = sparse_bundle(8192);
    let cancelled = AtomicBool::new(false);
    let bundle = StudioBundle::inspect(selected(&source.0), false, &cancelled).unwrap();
    let mut decoder = StudioTrackDecoder::open(&bundle, StudioRole::Drums).unwrap();
    let first = decoder.decode_window(0, 4096, &cancelled).unwrap();
    assert_eq!(first.left.len(), 4096);
    assert_eq!(first.bytes_read, 4096 * 4);
    let adjacent = decoder.decode_window(4096, 4096, &cancelled).unwrap();
    assert_eq!(adjacent.start_frame, 4096);
    assert_eq!(adjacent.bytes_read, 4096 * 4);

    OpenOptions::new()
        .append(true)
        .open(source.0.join("drums.wav"))
        .unwrap()
        .write_all(&[0])
        .unwrap();
    assert_eq!(
        decoder.decode_window(0, 1, &cancelled).err().unwrap().code,
        ErrorCode::BundleInconsistent
    );
}

fn selected(root: &PathBuf) -> SourceGroup {
    let files = fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            SelectedFile::new(entry.file_name().into_string().unwrap(), entry.path())
        })
        .collect();
    SourceGroup::new("resource bundle".to_owned(), files).unwrap()
}
fn sparse_bundle(frames: u64) -> TestDir {
    let dir = TestDir::new();
    for role in ["drums", "bass", "other", "vocals"] {
        sparse_wave(&dir.0.join(format!("{role}.wav")), frames);
    }
    fs::write(
        dir.0.join("manifest.json"),
        format!(
            "{{\"schema_version\":1,\"outputs\":[{}]}}",
            ["drums", "bass", "other", "vocals"]
                .map(|role| format!(
                    "{{\"logical_type\":\"{role}\",\"filename\":\"{role}.wav\",\"frames\":{frames}}}"
                ))
                .join(",")
        ),
    )
    .unwrap();
    dir
}

fn sparse_wave(path: &PathBuf, frames: u64) {
    let data_bytes = u32::try_from(frames * 4).unwrap();
    let mut header = Vec::new();
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36_u32 + data_bytes).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16_u32.to_le_bytes());
    header.extend_from_slice(&1_u16.to_le_bytes());
    header.extend_from_slice(&2_u16.to_le_bytes());
    header.extend_from_slice(&48_000_u32.to_le_bytes());
    header.extend_from_slice(&192_000_u32.to_le_bytes());
    header.extend_from_slice(&4_u16.to_le_bytes());
    header.extend_from_slice(&16_u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_bytes.to_le_bytes());
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&header).unwrap();
    file.set_len(44 + u64::from(data_bytes)).unwrap();
}
