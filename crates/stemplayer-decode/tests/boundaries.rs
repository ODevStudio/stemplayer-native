use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use stemplayer_decode::{
    CompressedTrackDecoder, decode_aac_segment, inspect_audio_file, inspect_audio_file_cancelled,
    inspect_compressed_tracks,
};
use stemplayer_domain::ErrorCode;
use stemplayer_format::{BoundedReader, inspect_mp4_structure};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "stemplayer-decode-boundary-{}-{}-{}",
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

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn oversized_mp4_packet() -> Vec<u8> {
    let mut bytes = include_bytes!("fixtures/stereo-aac.m4a").to_vec();
    let tag = bytes.windows(4).position(|value| value == b"stsz").unwrap();
    let fixed_size = u32::from_be_bytes(bytes[tag + 8..tag + 12].try_into().unwrap());
    let offset = if fixed_size == 0 { tag + 16 } else { tag + 8 };
    bytes[offset..offset + 4].copy_from_slice(&(1024_u32 * 1024 + 1).to_be_bytes());
    bytes
}

#[test]
fn file_and_segment_preflight_reject_oversized_packets_before_demux() {
    let scratch = Scratch::new();
    let bytes = oversized_mp4_packet();
    for extension in ["m4a", "mp3"] {
        let path = scratch.write(&format!("oversized.{extension}"), &bytes);
        assert!(inspect_audio_file(&path).is_err());
        assert!(CompressedTrackDecoder::open_audio(&path, 1).is_err());
    }
    let error = decode_aac_segment(bytes, 44_100, 0, 256, &AtomicBool::new(false)).unwrap_err();
    assert_eq!(error.code, ErrorCode::HlsCodecUnsupported);
}

#[test]
fn inspected_identity_and_configuration_are_required_at_decoder_open() {
    let scratch = Scratch::new();
    let bytes = include_bytes!("fixtures/mono.wav");
    let path = scratch.write("audio.wav", bytes);
    let info = inspect_audio_file(&path).unwrap();
    fs::rename(&path, scratch.0.join("original.wav")).unwrap();
    scratch.write("audio.wav", bytes);
    let error = CompressedTrackDecoder::open_audio_inspected(
        &path,
        &info,
        Arc::new(AtomicBool::new(false)),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, ErrorCode::BundleInconsistent);

    let mut altered = inspect_audio_file(&path).unwrap();
    altered.channels = 2;
    let error = CompressedTrackDecoder::open_audio_inspected(
        &path,
        &altered,
        Arc::new(AtomicBool::new(false)),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, ErrorCode::TrackConfigurationMismatch);
}

#[test]
fn cancellation_applies_to_inspection_and_the_opened_decoder() {
    let path = fixture("stereo.mp3");
    assert_eq!(
        inspect_audio_file_cancelled(&path, &AtomicBool::new(true))
            .unwrap_err()
            .code,
        ErrorCode::BundleAborted,
    );
    let info = inspect_audio_file(&path).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut decoder =
        CompressedTrackDecoder::open_audio_inspected(&path, &info, Arc::clone(&cancel)).unwrap();
    cancel.store(true, Ordering::Release);
    assert_eq!(
        decoder
            .decode_window(256, 64, &AtomicBool::new(false))
            .unwrap_err()
            .code,
        ErrorCode::BundleAborted
    );
}

#[test]
fn ordinary_mp4_duration_and_windows_follow_presentation_edits() {
    for name in ["stereo-aac.m4a", "stereo-alac.m4a"] {
        let path = fixture(name);
        let cancelled = Arc::new(AtomicBool::new(false));
        let structure =
            inspect_mp4_structure(BoundedReader::open(&path).unwrap(), &cancelled).unwrap();
        let info = inspect_audio_file(&path).unwrap();
        let raw = inspect_compressed_tracks(&path)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let descriptor = structure
            .tracks
            .iter()
            .find(|track| track.track_id == info.track_id)
            .unwrap();
        assert!(!descriptor.edits.is_empty());
        let expected_frames = descriptor
            .edits
            .iter()
            .map(|edit| {
                let scaled = u128::from(edit.segment_duration) * u128::from(info.sample_rate);
                assert_eq!(scaled % u128::from(structure.movie_timescale), 0);
                (scaled / u128::from(structure.movie_timescale)) as u64
            })
            .sum::<u64>();
        assert_eq!(info.declared_frames, Some(expected_frames));
        let first = info.timeline.as_ref().unwrap().segments[0];
        let media_start = first.media_start_frame.unwrap().get() as u64;
        let mut presentation =
            CompressedTrackDecoder::open_audio_inspected(&path, &info, Arc::clone(&cancelled))
                .unwrap();
        let mut media =
            CompressedTrackDecoder::open_inspected(&path, &raw, Arc::clone(&cancelled)).unwrap();
        let presented = presentation.decode_window(0, 256, &cancelled).unwrap();
        let expected = media.decode_window(media_start, 256, &cancelled).unwrap();
        assert_eq!(presented.left, expected.left, "{name}");
        assert_eq!(presented.right, expected.right, "{name}");
        assert!(
            presentation
                .decode_window(expected_frames - 128, 128, &cancelled)
                .is_ok()
        );
    }
}

fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [
        u32::try_from(body.len() + 8)
            .unwrap()
            .to_be_bytes()
            .as_slice(),
        kind,
        body,
    ]
    .concat()
}

fn full_atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    atom(kind, &[[0; 4].as_slice(), body].concat())
}

fn atoms(bytes: &[u8]) -> Vec<(usize, [u8; 4], &[u8])> {
    let mut offset = 0;
    let mut result = Vec::new();
    while offset < bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        assert!(length >= 8 && offset + length <= bytes.len());
        result.push((
            offset,
            bytes[offset + 4..offset + 8].try_into().unwrap(),
            &bytes[offset + 8..offset + length],
        ));
        offset += length;
    }
    result
}

fn metadata_track(sample_offset: u32, duration: u32) -> Vec<u8> {
    let tkhd = atom(
        b"tkhd",
        &[
            vec![0, 0, 0, 1],
            vec![0; 8],
            2_u32.to_be_bytes().to_vec(),
            vec![0; 4],
            1_u32.to_be_bytes().to_vec(),
            vec![0; 16],
            [0x0001_0000_u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000]
                .into_iter()
                .flat_map(u32::to_be_bytes)
                .collect(),
            vec![0; 8],
        ]
        .concat(),
    );
    let mdhd = full_atom(
        b"mdhd",
        &[
            vec![0; 8],
            44_100_u32.to_be_bytes().to_vec(),
            duration.to_be_bytes().to_vec(),
            vec![0; 4],
        ]
        .concat(),
    );
    let hdlr = full_atom(
        b"hdlr",
        &[vec![0; 4], b"meta".to_vec(), vec![0; 13]].concat(),
    );
    let entry = atom(
        b"mett",
        &[vec![0; 6], vec![0, 1, 0], b"text/plain\0".to_vec()].concat(),
    );
    let stsd = full_atom(b"stsd", &[1_u32.to_be_bytes().to_vec(), entry].concat());
    let stts = full_atom(
        b"stts",
        &[1_u32, 1, duration]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>(),
    );
    let stsc = full_atom(
        b"stsc",
        &[1_u32, 1, 1, 1]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>(),
    );
    let stsz = full_atom(
        b"stsz",
        &[1_u32, 1]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>(),
    );
    let stco = full_atom(
        b"stco",
        &[1_u32, sample_offset]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>(),
    );
    let stbl = atom(b"stbl", &[stsd, stts, stsc, stsz, stco].concat());
    let dref = full_atom(
        b"dref",
        &[1_u32.to_be_bytes().to_vec(), atom(b"url ", &[0, 0, 0, 1])].concat(),
    );
    let minf = atom(
        b"minf",
        &[full_atom(b"nmhd", &[]), atom(b"dinf", &dref), stbl].concat(),
    );
    atom(
        b"trak",
        &[tkhd, atom(b"mdia", &[mdhd, hdlr, minf].concat())].concat(),
    )
}

fn alac_with_metadata(duration: u32) -> Vec<u8> {
    let original = include_bytes!("fixtures/stereo-alac.m4a");
    let boxes = atoms(original);
    let (moov_offset, _, moov) = boxes.last().unwrap();
    assert_eq!(boxes.last().unwrap().1, *b"moov");
    let track_bytes = metadata_track(0, duration).len();
    let sample_offset = u32::try_from(original.len() + track_bytes + 8).unwrap();
    [
        original[..*moov_offset].to_vec(),
        atom(
            b"moov",
            &[moov.to_vec(), metadata_track(sample_offset, duration)].concat(),
        ),
        atom(b"mdat", &[0]),
    ]
    .concat()
}

#[test]
fn short_unrouted_tracks_are_rejected_before_local_mp4_seeks() {
    use std::io::Cursor;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{SeekMode, SeekTo, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::units::Timestamp;

    let scratch = Scratch::new();
    let bytes = alac_with_metadata(1);
    let path = scratch.write("short-metadata.m4a", &bytes);
    let cancelled = AtomicBool::new(false);
    let structure = inspect_mp4_structure(BoundedReader::open(&path).unwrap(), &cancelled).unwrap();
    assert_eq!(structure.tracks.len(), 2);
    assert!(
        structure
            .tracks
            .iter()
            .any(|track| !track.is_audio && track.media_duration == Some(1))
    );
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("m4a");
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, Default::default(), Default::default())
        .unwrap();
    assert_eq!(format.tracks().len(), 2);
    assert!(
        format
            .tracks()
            .iter()
            .any(|track| track.track_type() != Some(TrackType::Audio))
    );
    assert!(
        format
            .seek(
                SeekMode::Accurate,
                SeekTo::Timestamp {
                    ts: Timestamp::new(64),
                    track_id: 1
                }
            )
            .is_err()
    );
    assert_eq!(
        inspect_audio_file(&path).unwrap_err().code,
        ErrorCode::CodecUnsupported
    );
    assert_eq!(
        inspect_compressed_tracks(&path).unwrap_err().code,
        ErrorCode::CodecUnsupported
    );
    assert_eq!(
        CompressedTrackDecoder::open_audio(&path, 1)
            .err()
            .unwrap()
            .code,
        ErrorCode::CodecUnsupported
    );
    assert_eq!(
        CompressedTrackDecoder::open(&path, 1).err().unwrap().code,
        ErrorCode::CodecUnsupported
    );
}

#[test]
fn unrouted_tracks_with_sufficient_seek_coverage_remain_supported() {
    let scratch = Scratch::new();
    let path = scratch.write("full-metadata.m4a", &alac_with_metadata(22_050));
    assert!(inspect_audio_file(&path).is_ok());
    assert_eq!(inspect_compressed_tracks(&path).unwrap().len(), 1);
    for mut decoder in [
        CompressedTrackDecoder::open_audio(&path, 1).unwrap(),
        CompressedTrackDecoder::open(&path, 1).unwrap(),
    ] {
        assert_eq!(
            decoder
                .decode_window(64, 128, &AtomicBool::new(false))
                .unwrap()
                .frames,
            128
        );
    }
}

fn hostile_aac_descriptor() -> Vec<u8> {
    let original = include_bytes!("fixtures/stereo-aac.m4a");
    let mut body = original.as_slice();
    let mut offset = 0;
    for kind in [*b"moov", *b"trak", *b"mdia", *b"minf", *b"stbl", *b"stsd"] {
        let (start, _, child) = atoms(body)
            .into_iter()
            .find(|(_, value, _)| *value == kind)
            .unwrap();
        offset += start + 8;
        body = child;
    }
    let (entry, kind, sample) = atoms(&body[8..]).into_iter().next().unwrap();
    assert_eq!(kind, *b"mp4a");
    let (esds, _, _) = atoms(&sample[28..])
        .into_iter()
        .find(|(_, kind, _)| kind == b"esds")
        .unwrap();
    let length = offset + 8 + entry + 8 + 28 + esds + 8 + 5;
    [
        original[..length].to_vec(),
        vec![0xff, 0xff, 0xff, 0x7f],
        original[length + 4..].to_vec(),
    ]
    .concat()
}

#[test]
fn local_and_hls_preflight_reject_hostile_descriptors_after_legal_leading_boxes() {
    let scratch = Scratch::new();
    for (index, leading) in [
        atom(b"free", &[0; 16]),
        atom(b"skip", &[0; 16]),
        atom(b"wide", &[]),
        atom(b"mdat", &[0; 16]),
        [
            1_u32.to_be_bytes().to_vec(),
            b"free".to_vec(),
            16_u64.to_be_bytes().to_vec(),
        ]
        .concat(),
    ]
    .into_iter()
    .enumerate()
    {
        let bytes = [leading, hostile_aac_descriptor()].concat();
        let path = scratch.write(&format!("hostile-{index}.m4a"), &bytes);
        assert_eq!(
            inspect_mp4_structure(BoundedReader::open(&path).unwrap(), &AtomicBool::new(false))
                .unwrap_err()
                .code,
            ErrorCode::CodecConfigInvalid
        );
        assert!(inspect_audio_file(&path).is_err());
        assert!(inspect_compressed_tracks(&path).is_err());
        assert!(CompressedTrackDecoder::open_audio(&path, 1).is_err());
        assert_eq!(
            decode_aac_segment(bytes, 44_100, 0, 22_050, &AtomicBool::new(false))
                .unwrap_err()
                .code,
            ErrorCode::HlsCodecUnsupported
        );
    }
}
