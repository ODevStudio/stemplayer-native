use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::sync_channel;
use stemplayer_domain::ErrorCode;
use stemplayer_format::{
    HashStatus, MAX_MANIFEST_BYTES, SampleEncoding, SelectedFile, SourceGroup, StudioBundle,
    StudioRole,
};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "stemplayer-phase4-{}-{}",
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
fn qualified_encodings_rates_and_optional_master_inspect() {
    for (encoding, rate, master) in [
        (SampleEncoding::Pcm16, 44_100, false),
        (SampleEncoding::Pcm24, 48_000, true),
        (SampleEncoding::Pcm32, 44_100, false),
        (SampleEncoding::Float32, 48_000, true),
    ] {
        let dir = bundle(encoding, rate, master, false);
        let inspected =
            StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false)).unwrap();
        assert_eq!(inspected.report().sample_rate, rate);
        assert_eq!(inspected.report().frames, 4);
        assert_eq!(inspected.report().has_master, master);
        assert_eq!(
            inspected.report().maximum_open_files,
            if master { 5 } else { 4 }
        );
        assert_eq!(inspected.report().peak_pending_blocks, 1);
        assert_eq!(inspected.report().warnings.len(), 1);
    }
}

#[test]
fn permitted_manifest_sizes_are_read_in_bounded_ranges() {
    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    let path = dir.0.join("manifest.json");
    let manifest = fs::read(&path).unwrap();
    for length in [65_536, 65_537, MAX_MANIFEST_BYTES] {
        let padded = manifest
            .iter()
            .copied()
            .chain(std::iter::repeat(b' '))
            .take(length)
            .collect::<Vec<_>>();
        fs::write(&path, padded).unwrap();
        let inspected =
            StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false)).unwrap();
        assert_eq!(inspected.report().frames, 4);
        assert_eq!(
            inspected.report().bytes_read,
            length as u64
                + inspected
                    .report()
                    .tracks
                    .iter()
                    .map(|track| track.bytes_read)
                    .sum::<u64>()
        );
    }
    fs::write(&path, vec![b' '; MAX_MANIFEST_BYTES + 1]).unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::ManifestTooLarge
    );
}

#[test]
fn same_metadata_replacements_cannot_reopen_validated_tracks() {
    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    let inspected =
        StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false)).unwrap();
    let path = dir.0.join("drums.wav");
    let metadata = fs::metadata(&path).unwrap();
    let bytes = fs::read(&path).unwrap();
    fs::rename(&path, dir.0.join("original.wav")).unwrap();
    fs::write(&path, bytes).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(metadata.modified().unwrap())
        .unwrap();
    let replacement = fs::metadata(&path).unwrap();
    assert_eq!(replacement.len(), metadata.len());
    assert_eq!(
        replacement.modified().unwrap(),
        metadata.modified().unwrap()
    );
    assert_eq!(
        inspected.open_track(StudioRole::Drums).err().unwrap().code,
        ErrorCode::BundleInconsistent
    );
}

#[test]
fn hashes_stream_and_fail_closed() {
    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, true);
    let (sender, receiver) = sync_channel(1);
    let validated = StudioBundle::inspect_with_progress(
        selected(&dir.0),
        true,
        &AtomicBool::new(false),
        Some(&sender),
    )
    .unwrap();
    let progress = receiver.try_recv().unwrap();
    assert_eq!(validated.report().maximum_open_files, 5);
    assert_eq!(progress.file_index, 0);
    assert_eq!(progress.bytes_hashed, progress.total_bytes);
    assert!(
        validated
            .report()
            .tracks
            .iter()
            .all(|track| track.hash_status == HashStatus::Verified)
    );

    let path = dir.0.join("bass.wav");
    let mut bytes = fs::read(&path).unwrap();
    bytes[44] ^= 1;
    fs::write(path, bytes).unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), true, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::ManifestSha256Mismatch
    );
}

#[test]
fn cancellation_and_track_mismatch_are_deterministic() {
    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), true, &AtomicBool::new(true))
            .err()
            .unwrap()
            .code,
        ErrorCode::BundleAborted
    );

    let manifest_path = dir.0.join("manifest.json");
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    fs::write(
        &manifest_path,
        manifest.replace(",\"sample_rate\":48000", ""),
    )
    .unwrap();
    fs::write(
        dir.0.join("vocals.wav"),
        wave(SampleEncoding::Pcm16, 44_100, 4),
    )
    .unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::BundleInconsistent
    );
}

#[test]
fn manifest_rate_and_frame_declarations_must_match_wav() {
    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    let path = dir.0.join("manifest.json");
    let manifest = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        manifest.replacen(r#""frames":4"#, r#""frames":5"#, 1),
    )
    .unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::WavFrameCountMismatch
    );

    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    let path = dir.0.join("manifest.json");
    let manifest = fs::read_to_string(&path).unwrap();
    fs::write(
        path,
        manifest.replacen(r#""sample_rate":48000"#, r#""sample_rate":44100"#, 1),
    )
    .unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::WavSampleRateMismatch
    );
}
#[test]
fn unsafe_missing_truncated_and_malformed_hash_inputs_fail() {
    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    fs::remove_file(dir.0.join("bass.wav")).unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::ManifestFileMissing
    );

    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    let manifest = fs::read_to_string(dir.0.join("manifest.json")).unwrap();
    fs::write(
        dir.0.join("manifest.json"),
        manifest.replace("drums.wav", "../drums.wav"),
    )
    .unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::ManifestPathUnsafe
    );

    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    let path = dir.0.join("other.wav");
    let mut bytes = fs::read(&path).unwrap();
    bytes.truncate(bytes.len() - 1);
    fs::write(path, bytes).unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::WavDataTruncated
    );

    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    let manifest = fs::read_to_string(dir.0.join("manifest.json")).unwrap();
    fs::write(
        dir.0.join("manifest.json"),
        manifest.replace("\"drums.wav\"", "\"drums.wav\",\"sha256\":\"bad\""),
    )
    .unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), true, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::ManifestInvalid
    );
}

#[test]
fn every_structural_truncation_and_declared_overflow_fail() {
    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    let path = dir.0.join("drums.wav");
    let valid = fs::read(&path).unwrap();
    for end in 0..valid.len() {
        fs::write(&path, &valid[..end]).unwrap();
        assert!(StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false)).is_err());
    }
    let mut overflow = valid;
    overflow[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    fs::write(path, overflow).unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::WavDataTruncated
    );
}

#[test]
fn normalized_reference_collisions_fail_before_open() {
    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    let path = dir.0.join("manifest.json");
    let manifest = fs::read_to_string(&path).unwrap();
    fs::write(&path, manifest.replace("bass.wav", "DRUMS.WAV")).unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::ManifestFileAmbiguous
    );

    let manifest = fs::read_to_string(&path).unwrap();
    fs::write(path, manifest.replace("DRUMS.WAV", "e\u{301}.wav")).unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::ManifestPathUnsafe
    );
}
#[test]
fn wav_profile_rejects_rf64_channels_rates_alignment_and_compression() {
    for (offset, replacement, expected) in [
        (0, b"RF64".as_slice(), ErrorCode::WavNotRiff),
        (20, &6_u16.to_le_bytes(), ErrorCode::WavFormatUnsupported),
        (22, &1_u16.to_le_bytes(), ErrorCode::WavChannelsUnsupported),
        (22, &3_u16.to_le_bytes(), ErrorCode::WavChannelsUnsupported),
        (
            24,
            &96_000_u32.to_le_bytes(),
            ErrorCode::WavFormatUnsupported,
        ),
        (28, &1_u32.to_le_bytes(), ErrorCode::WavBlockAlignInvalid),
        (32, &1_u16.to_le_bytes(), ErrorCode::WavBlockAlignInvalid),
        (34, &64_u16.to_le_bytes(), ErrorCode::WavFormatUnsupported),
    ] {
        let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
        let path = dir.0.join("drums.wav");
        let mut bytes = fs::read(&path).unwrap();
        bytes[offset..offset + replacement.len()].copy_from_slice(replacement);
        fs::write(path, bytes).unwrap();
        assert_eq!(
            StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false))
                .err()
                .unwrap()
                .code,
            expected
        );
    }
}

#[test]
fn multiple_data_chunks_are_rejected() {
    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    let path = dir.0.join("drums.wav");
    let mut bytes = fs::read(&path).unwrap();
    let riff_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) + 8;
    bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    fs::write(path, bytes).unwrap();
    assert_eq!(
        StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false))
            .err()
            .unwrap()
            .code,
        ErrorCode::WavMalformed
    );
}
#[test]
fn data_before_format_is_accepted() {
    let dir = bundle(SampleEncoding::Pcm16, 48_000, false, false);
    let path = dir.0.join("drums.wav");
    let bytes = fs::read(&path).unwrap();
    let mut reordered = bytes[..12].to_vec();
    reordered.extend_from_slice(&bytes[46..]);
    reordered.extend_from_slice(&bytes[12..46]);
    fs::write(path, reordered).unwrap();
    assert!(StudioBundle::inspect(selected(&dir.0), false, &AtomicBool::new(false)).is_ok());
}
fn selected(root: &PathBuf) -> SourceGroup {
    let files = fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            SelectedFile::new(entry.file_name().into_string().unwrap(), entry.path())
        })
        .collect();
    SourceGroup::new("test bundle".to_owned(), files).unwrap()
}
fn bundle(encoding: SampleEncoding, sample_rate: u32, master: bool, hashes: bool) -> TestDir {
    let dir = TestDir::new();
    let roles = ["drums", "bass", "other", "vocals"];
    let mut outputs = Vec::new();
    for role in roles.into_iter().chain(master.then_some("master")) {
        let filename = format!("{role}.wav");
        let bytes = wave(encoding, sample_rate, 4);
        fs::write(dir.0.join(&filename), &bytes).unwrap();
        outputs.push(output(
            role,
            &filename,
            sample_rate,
            hashes.then(|| hash(&bytes)),
        ));
    }
    fs::write(
        dir.0.join("manifest.json"),
        format!(
            "{{\"schema_version\":1,\"unknown_diagnostic\":true,\"outputs\":[{}]}}",
            outputs.join(",")
        ),
    )
    .unwrap();
    dir
}

fn output(role: &str, filename: &str, rate: u32, hash: Option<String>) -> String {
    format!(
        "{{\"logical_type\":\"{role}\",\"filename\":\"{filename}\",\"sample_rate\":{rate},\"channels\":2,\"frames\":4{}}}",
        hash.map(|value| format!(",\"sha256\":\"{value}\""))
            .unwrap_or_default()
    )
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn wave(encoding: SampleEncoding, sample_rate: u32, frames: u32) -> Vec<u8> {
    let format = if encoding == SampleEncoding::Float32 {
        3_u16
    } else {
        1
    };
    let bits = encoding.bits_per_sample();
    let align = bits / 8 * 2;
    let mut data = Vec::new();
    for frame in 0..frames {
        for channel in 0..2 {
            let value = i64::from(frame) * 2 + i64::from(channel);
            match encoding {
                SampleEncoding::Pcm16 => data.extend_from_slice(&(value as i16).to_le_bytes()),
                SampleEncoding::Pcm24 => {
                    let bytes = (value as i32).to_le_bytes();
                    data.extend_from_slice(&bytes[..3]);
                }
                SampleEncoding::Pcm32 => data.extend_from_slice(&(value as i32).to_le_bytes()),
                SampleEncoding::Float32 => {
                    data.extend_from_slice(&(value as f32 / 2.0).to_le_bytes())
                }
            }
        }
    }
    riff(format, sample_rate, align, bits, &data)
}

fn riff(format: u16, sample_rate: u32, align: u16, bits: u16, data: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16_u32.to_le_bytes());
    body.extend_from_slice(&format.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&sample_rate.to_le_bytes());
    body.extend_from_slice(&(sample_rate * u32::from(align)).to_le_bytes());
    body.extend_from_slice(&align.to_le_bytes());
    body.extend_from_slice(&bits.to_le_bytes());
    body.extend_from_slice(b"JUNK");
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&[7, 0]);
    body.extend_from_slice(b"data");
    body.extend_from_slice(&(data.len() as u32).to_le_bytes());
    body.extend_from_slice(data);
    if data.len() % 2 == 1 {
        body.push(0);
    }
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(&body);
    wav
}
