use super::*;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn demux_preflight_accepts_ordinary_aac_and_alac_without_stem_roles() {
    for bytes in [
        include_bytes!("../../stemplayer-decode/tests/fixtures/stereo-aac.m4a").as_slice(),
        include_bytes!("../../stemplayer-decode/tests/fixtures/stereo-alac.m4a").as_slice(),
    ] {
        let cancelled = AtomicBool::new(false);
        validate_mp4_bytes_for_demux(bytes, &cancelled).unwrap();
        let structure = inspect(bytes).unwrap();
        assert_eq!(structure.track_count, 1);
        assert_eq!(
            structure
                .tracks
                .iter()
                .filter(|track| track.is_audio)
                .count(),
            1
        );
    }
}

#[test]
fn demux_preflight_rejects_oversized_packets_on_unrouted_tracks() {
    for fixed in [true, false] {
        let header = [
            0_u32.to_be_bytes(),
            (if fixed { u32::MAX } else { 0 }).to_be_bytes(),
            1_u32.to_be_bytes(),
        ]
        .concat();
        let sizes = if fixed {
            header
        } else {
            [header, u32::MAX.to_be_bytes().to_vec()].concat()
        };
        let track = atom(
            *b"trak",
            &[
                track_header(),
                atom(
                    *b"mdia",
                    &atom(*b"minf", &atom(*b"stbl", &atom(*b"stsz", &sizes))),
                ),
            ]
            .concat(),
        );
        assert_eq!(
            validate_mp4_bytes_for_demux(&file_with_tracks(&track), &AtomicBool::new(false))
                .unwrap_err()
                .code,
            ErrorCode::EncodedSampleInvalid
        );
    }
}

#[test]
fn demux_preflight_validates_unrouted_chunk_tables_and_ranges() {
    for (first_chunk, chunk_offset) in [(1, 0), (2, 0), (1, u32::MAX)] {
        let sizes = atom(
            *b"stsz",
            &[
                0_u32.to_be_bytes(),
                20_u32.to_be_bytes(),
                3_u32.to_be_bytes(),
            ]
            .concat(),
        );
        let track = track_with_layout(sizes, *b"vide", first_chunk, chunk_offset);
        assert_eq!(
            validate_mp4_bytes_for_demux(&file_with_tracks(&track), &AtomicBool::new(false))
                .is_ok(),
            first_chunk == 1 && chunk_offset == 0
        );
    }
}

#[test]
fn demux_preflight_bounds_sample_descriptions_before_reading_their_payloads() {
    let sizes = atom(
        *b"stsz",
        &[
            0_u32.to_be_bytes(),
            20_u32.to_be_bytes(),
            3_u32.to_be_bytes(),
        ]
        .concat(),
    );
    for length in [
        MAX_SAMPLE_DESCRIPTION_BYTES,
        MAX_SAMPLE_DESCRIPTION_BYTES + 1,
    ] {
        let padding = atom(*b"free", &vec![0_u8; length as usize - 52]);
        let description = sample_description(*b"mp4a", 0, &padding);
        let track = track_with_sample_sizes([sizes.clone(), description].concat());
        let bytes = file_with_tracks(&track);
        assert_eq!(
            validate_mp4_bytes_for_demux(&bytes, &AtomicBool::new(false)).is_ok(),
            length == MAX_SAMPLE_DESCRIPTION_BYTES
        );
    }
    let description = sample_description(*b"mp4a", 0, &[]);
    let duplicate = track_with_sample_sizes([sizes, description.clone(), description].concat());
    assert!(
        validate_mp4_bytes_for_demux(&file_with_tracks(&duplicate), &AtomicBool::new(false))
            .is_err()
    );
}

#[test]
fn demux_preflight_rejects_nested_esds_allocation_lengths() {
    let hostile = atom(
        *b"esds",
        &[
            0, 0, 0, 0, 0x03, 0x17, 0, 1, 0, 0x04, 0x12, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0x05, 0xff, 0xff, 0xff, 0x7f,
        ],
    );
    let valid = esds(&[0x11, 0x90]);
    for (kind, version) in [
        (*b"mp4a", 0),
        (*b"mp4a", 1),
        (*b"mp4a", 2),
        (*b"alac", 0),
        (*b"Opus", 0),
        (*b"wave", 0),
        (*b"mp4v", 0),
    ] {
        for children in [
            hostile.clone(),
            atom(*b"wave", &hostile),
            [hostile.clone(), valid.clone()].concat(),
            [valid.clone(), hostile.clone()].concat(),
        ] {
            if kind == *b"mp4v" && children[4..8] == *b"wave" {
                continue;
            }
            let track = track_with_description(kind, version, &children);
            let bytes = file_with_tracks(&[audio_track(), track].concat());
            assert!(bytes.len() < 2048);
            assert_demux_rejected(&bytes);
        }
    }
}

#[test]
fn demux_preflight_bounds_each_esds_container_and_configuration() {
    for payload in [
        vec![0x03, 0xff, 0xff, 0xff, 0x7f],
        descriptor(3, &[0, 1, 0, 0x04, 0x7f]),
        descriptor(3, &[0, 1, 0x40, 0xff]),
        descriptor(
            3,
            &[
                0, 1, 0, 0x04, 0x12, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x05, 0xff, 0xff,
                0xff, 0xff,
            ],
        ),
    ] {
        let children = atom(*b"esds", &[[0; 4].to_vec(), payload].concat());
        assert_demux_rejected(&file_with_tracks(&track_with_description(
            *b"mp4a", 0, &children,
        )));
    }
    for length in [64 * 1024, 64 * 1024 + 1] {
        let bytes = file_with_tracks(&track_with_description(
            *b"mp4a",
            0,
            &esds(&vec![0; length]),
        ));
        if length == 64 * 1024 {
            validate_mp4_bytes_for_demux(&bytes, &AtomicBool::new(false)).unwrap();
            inspect(&bytes).unwrap();
        } else {
            assert_demux_rejected(&bytes);
        }
    }
}

#[test]
fn demux_preflight_validates_descriptors_after_legal_leading_boxes() {
    let valid = file_with_tracks(&track_with_description(*b"mp4a", 0, &esds(&[0x11, 0x90])));
    let invalid = file_with_tracks(&track_with_description(
        *b"mp4a",
        0,
        &atom(*b"esds", &[0, 0, 0, 0, 3, 0xff, 0xff, 0xff, 0x7f]),
    ));
    for leading in [
        atom(*b"free", &[0; 16]),
        atom(*b"skip", &[0; 16]),
        atom(*b"wide", &[]),
        atom(*b"mdat", &[0; 16]),
        [
            1_u32.to_be_bytes().to_vec(),
            b"free".to_vec(),
            16_u64.to_be_bytes().to_vec(),
        ]
        .concat(),
    ] {
        let bytes = [leading.clone(), valid.clone()].concat();
        validate_mp4_bytes_for_demux(&bytes, &AtomicBool::new(false)).unwrap();
        inspect(&bytes).unwrap();
        assert_demux_rejected(&[leading, invalid.clone()].concat());
    }
}

fn assert_demux_rejected(bytes: &[u8]) {
    let cancelled = AtomicBool::new(false);
    assert_eq!(
        validate_mp4_bytes_for_demux(bytes, &cancelled)
            .unwrap_err()
            .code,
        ErrorCode::CodecConfigInvalid
    );
    let path = write_temporary(bytes);
    let result = validate_mp4_for_demux(&mut BoundedReader::open(&path).unwrap(), &cancelled);
    fs::remove_file(path).unwrap();
    assert_eq!(result.unwrap_err().code, ErrorCode::CodecConfigInvalid);
}

fn track_with_description(kind: [u8; 4], version: u16, children: &[u8]) -> Vec<u8> {
    let sizes = atom(
        *b"stsz",
        &[
            0_u32.to_be_bytes(),
            20_u32.to_be_bytes(),
            3_u32.to_be_bytes(),
        ]
        .concat(),
    );
    track_with_layout(
        [sizes, sample_description(kind, version, children)].concat(),
        if kind == *b"mp4v" { *b"vide" } else { *b"soun" },
        1,
        0,
    )
}

fn sample_description(kind: [u8; 4], version: u16, children: &[u8]) -> Vec<u8> {
    let length = if kind == *b"mp4v" {
        78
    } else {
        match version {
            0 => 28,
            1 => 44,
            2 => 64,
            _ => unreachable!(),
        }
    };
    let mut entry = vec![0; length];
    entry[6..8].copy_from_slice(&1_u16.to_be_bytes());
    if kind != *b"mp4v" {
        entry[8..10].copy_from_slice(&version.to_be_bytes());
        entry[16..18].copy_from_slice(&2_u16.to_be_bytes());
        entry[18..20].copy_from_slice(&16_u16.to_be_bytes());
        entry[24..28].copy_from_slice(&(48_000_u32 << 16).to_be_bytes());
        if version == 2 {
            entry[32..40].copy_from_slice(&48_000_f64.to_be_bytes());
            entry[40..44].copy_from_slice(&2_u32.to_be_bytes());
            entry[44..48].copy_from_slice(&0x7f00_0000_u32.to_be_bytes());
        }
    }
    atom(
        *b"stsd",
        &[
            [0_u32.to_be_bytes(), 1_u32.to_be_bytes()].concat(),
            atom(kind, &[entry, children.to_vec()].concat()),
        ]
        .concat(),
    )
}

fn esds(configuration: &[u8]) -> Vec<u8> {
    let decoder = descriptor(
        4,
        &[
            vec![0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            descriptor(5, configuration),
        ]
        .concat(),
    );
    atom(
        *b"esds",
        &[
            vec![0; 4],
            descriptor(3, &[vec![0, 1, 0], decoder, descriptor(6, &[2])].concat()),
        ]
        .concat(),
    )
}

fn descriptor(tag: u8, body: &[u8]) -> Vec<u8> {
    let length = body.len() as u32;
    [
        vec![
            tag,
            0x80 | ((length >> 21) & 0x7f) as u8,
            0x80 | ((length >> 14) & 0x7f) as u8,
            0x80 | ((length >> 7) & 0x7f) as u8,
            (length & 0x7f) as u8,
        ],
        body.to_vec(),
    ]
    .concat()
}

#[test]
fn demux_preflight_bounds_aggregate_ancillary_metadata() {
    for lengths in [
        vec![MAX_METADATA_BYTES],
        vec![MAX_METADATA_BYTES + 1],
        vec![MAX_METADATA_BYTES / 2, MAX_METADATA_BYTES / 2 + 1],
    ] {
        let metadata = lengths
            .iter()
            .map(|length| atom(*b"meta", &vec![0_u8; *length]))
            .collect::<Vec<_>>()
            .concat();
        let bytes = file_with_tracks(&atom(*b"udta", &metadata));
        assert_eq!(
            validate_mp4_bytes_for_demux(&bytes, &AtomicBool::new(false)).is_ok(),
            lengths.iter().sum::<usize>() <= MAX_METADATA_BYTES
        );
    }
}

#[test]
fn demux_preflight_rejects_fragment_tables_and_honors_cancellation() {
    for fragment in [atom(*b"moof", &[]), atom(*b"mvex", &[])] {
        let bytes = [file_with_tracks(&[]), fragment].concat();
        assert!(validate_mp4_bytes_for_demux(&bytes, &AtomicBool::new(false)).is_err());
    }
    assert_eq!(
        validate_mp4_bytes_for_demux(&file_with_tracks(&[]), &AtomicBool::new(true))
            .unwrap_err()
            .code,
        ErrorCode::BundleAborted
    );
}

fn file_with_tracks(tracks: &[u8]) -> Vec<u8> {
    [
        atom(*b"ftyp", b"M4A \0\0\0\0isom"),
        atom(*b"moov", &[movie_header(1_000), tracks.to_vec()].concat()),
    ]
    .concat()
}

fn track_header() -> Vec<u8> {
    let mut header = vec![0_u8; 24];
    header[12..16].copy_from_slice(&7_u32.to_be_bytes());
    atom(*b"tkhd", &header)
}

#[test]
fn bounded_structure_accepts_valid_boxes_and_rejects_bad_ranges() {
    let valid = [
        atom(*b"ftyp", b"M4A \0\0\0\0isomM4A "),
        atom(
            *b"moov",
            &[
                movie_header(1_000),
                audio_track(),
                atom(
                    *b"udta",
                    &atom(
                        *b"stem",
                        br#"{"stems":[{"name":"Drums"},{"name":"Bass"},{"name":"Other"},{"name":"Vocals"}]}"#,
                    ),
                ),
            ]
            .concat(),
        ),
    ]
    .concat();
    let structure = inspect(&valid).unwrap();
    assert_eq!(structure.major_brand, "M4A ");
    assert_eq!(structure.compatible_brands, ["isom", "M4A "]);
    assert_eq!(structure.track_count, 1);
    assert_eq!(structure.movie_timescale, 1_000);
    assert_eq!(structure.tracks[0].track_id, 7);
    assert!(structure.tracks[0].enabled && structure.tracks[0].is_audio);
    assert_eq!(structure.tracks[0].media_timescale, Some(48_000));
    assert_eq!(structure.tracks[0].media_duration, Some(144_000));
    assert_eq!(structure.tracks[0].name.as_deref(), Some("Drums"));
    assert_eq!(
        structure
            .native_roles
            .as_ref()
            .unwrap()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["Drums", "Bass", "Other", "Vocals"]
    );
    assert_eq!(structure.tracks[0].sample_count, Some(3));
    assert_eq!(structure.tracks[0].encoded_bytes, Some(60));
    let index = structure.tracks[0].packet_index.as_ref().unwrap();
    assert_eq!(index.packet_range(0).unwrap(), (0, 10));
    assert_eq!(index.packet_range(1).unwrap(), (10, 20));
    assert_eq!(index.packet_range(2).unwrap(), (30, 30));
    assert_eq!(index.sample_at_or_before(96_000), 2);
    assert_eq!(
        structure.tracks[0].edits,
        [Mp4Edit {
            segment_duration: 3_000,
            media_time: 1_024,
            media_rate_integer: 1,
            media_rate_fraction: 0,
        }]
    );

    let mut truncated = atom(*b"moov", &[]);
    truncated[..4].copy_from_slice(&64_u32.to_be_bytes());
    assert_eq!(
        inspect(&truncated).unwrap_err().code,
        ErrorCode::ContainerMalformed
    );
    assert_eq!(
        inspect(&atom(*b"free", &[])).unwrap_err().code,
        ErrorCode::ContainerNotMp4
    );
}

#[test]
fn version_one_times_and_invalid_sample_tables_are_checked() {
    let valid = [
        atom(*b"ftyp", b"M4A \0\0\0\0isom"),
        atom(
            *b"moov",
            &[movie_header_v1(2_000, 12_000), audio_track()].concat(),
        ),
    ]
    .concat();
    assert_eq!(inspect(&valid).unwrap().movie_timescale, 2_000);

    let mut invalid = vec![0_u8; 12];
    invalid[8..12].copy_from_slice(&1_000_001_u32.to_be_bytes());
    let file = [
        atom(*b"ftyp", b"M4A \0\0\0\0isom"),
        atom(
            *b"moov",
            &[
                movie_header(1_000),
                track_with_sample_sizes(atom(*b"stsz", &invalid)),
            ]
            .concat(),
        ),
    ]
    .concat();
    assert_eq!(
        inspect(&file).unwrap_err().code,
        ErrorCode::ContainerMalformed
    );
}

fn inspect(bytes: &[u8]) -> Result<Mp4Structure, PlayerError> {
    let path = write_temporary(bytes);
    let result =
        inspect_mp4_structure(BoundedReader::open(&path).unwrap(), &AtomicBool::new(false));
    if result.is_ok() {
        validate_mp4_for_demux(
            &mut BoundedReader::open(&path).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap();
    }
    fs::remove_file(path).unwrap();
    result
}

fn write_temporary(bytes: &[u8]) -> PathBuf {
    let path = temporary_path();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
    path
}

fn atom(kind: [u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + body.len());
    bytes.extend_from_slice(&(8_u32 + u32::try_from(body.len()).unwrap()).to_be_bytes());
    bytes.extend_from_slice(&kind);
    bytes.extend_from_slice(body);
    bytes
}

fn movie_header(timescale: u32) -> Vec<u8> {
    let mut body = vec![0_u8; 20];
    body[12..16].copy_from_slice(&timescale.to_be_bytes());
    body[16..20].copy_from_slice(&3_000_u32.to_be_bytes());
    atom(*b"mvhd", &body)
}

fn movie_header_v1(timescale: u32, duration: u64) -> Vec<u8> {
    let mut body = vec![0_u8; 32];
    body[0] = 1;
    body[20..24].copy_from_slice(&timescale.to_be_bytes());
    body[24..32].copy_from_slice(&duration.to_be_bytes());
    atom(*b"mvhd", &body)
}

fn audio_track() -> Vec<u8> {
    let mut sizes = vec![0_u8; 12];
    sizes[8..12].copy_from_slice(&3_u32.to_be_bytes());
    for size in [10_u32, 20, 30] {
        sizes.extend_from_slice(&size.to_be_bytes());
    }
    track_with_sample_sizes(atom(*b"stsz", &sizes))
}

fn track_with_sample_sizes(sample_sizes: Vec<u8>) -> Vec<u8> {
    track_with_layout(sample_sizes, *b"soun", 1, 0)
}

fn track_with_layout(
    sample_sizes: Vec<u8>,
    handler_kind: [u8; 4],
    first_chunk: u32,
    chunk_offset: u32,
) -> Vec<u8> {
    let mut track_header = vec![0_u8; 24];
    track_header[3] = 1;
    track_header[12..16].copy_from_slice(&7_u32.to_be_bytes());

    let mut media_header = vec![0_u8; 20];
    media_header[12..16].copy_from_slice(&48_000_u32.to_be_bytes());
    media_header[16..20].copy_from_slice(&144_000_u32.to_be_bytes());

    let mut handler = vec![0_u8; 24];
    handler[8..12].copy_from_slice(&handler_kind);

    let mut edit = vec![0_u8; 20];
    edit[4..8].copy_from_slice(&1_u32.to_be_bytes());
    edit[8..12].copy_from_slice(&3_000_u32.to_be_bytes());
    edit[12..16].copy_from_slice(&1_024_i32.to_be_bytes());
    edit[16..18].copy_from_slice(&1_i16.to_be_bytes());

    let mut chunk_offsets = vec![0_u8; 8];
    chunk_offsets[4..8].copy_from_slice(&1_u32.to_be_bytes());
    chunk_offsets.extend_from_slice(&chunk_offset.to_be_bytes());

    let mut sample_to_chunk = vec![0_u8; 8];
    sample_to_chunk[4..8].copy_from_slice(&1_u32.to_be_bytes());
    sample_to_chunk.extend_from_slice(&first_chunk.to_be_bytes());
    sample_to_chunk.extend_from_slice(&3_u32.to_be_bytes());
    sample_to_chunk.extend_from_slice(&1_u32.to_be_bytes());

    let mut timing = vec![0_u8; 8];
    timing[4..8].copy_from_slice(&1_u32.to_be_bytes());
    timing.extend_from_slice(&3_u32.to_be_bytes());
    timing.extend_from_slice(&48_000_u32.to_be_bytes());

    let sample_table = [
        sample_sizes,
        atom(*b"stco", &chunk_offsets),
        atom(*b"stsc", &sample_to_chunk),
        atom(*b"stts", &timing),
    ]
    .concat();

    atom(
        *b"trak",
        &[
            atom(*b"tkhd", &track_header),
            atom(*b"name", b"Drums"),
            atom(*b"edts", &atom(*b"elst", &edit)),
            atom(
                *b"mdia",
                &[
                    atom(*b"mdhd", &media_header),
                    atom(*b"hdlr", &handler),
                    atom(*b"minf", &atom(*b"stbl", &sample_table)),
                ]
                .concat(),
            ),
        ]
        .concat(),
    )
}

fn temporary_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("stemplayer-mp4-{}-{nonce}.mp4", std::process::id()))
}

#[test]
#[ignore = "requires STEMPLAYER_NATIVE_CONTAINER_FIXTURE"]
fn local_container_structure_is_bounded_and_content_identified() {
    let path = std::env::var("STEMPLAYER_NATIVE_CONTAINER_FIXTURE").unwrap();
    let structure = inspect_mp4_structure(
        BoundedReader::open(std::path::Path::new(&path)).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(matches!(structure.track_count, 4 | 5));
    assert!(!structure.major_brand.is_empty());
    assert!(structure.box_count > structure.track_count);
}
