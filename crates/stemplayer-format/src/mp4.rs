use crate::BoundedReader;
use crate::mp4_metadata::{
    MAX_METADATA_BYTES, parse_native_roles, read_handler, read_metadata, read_track_name,
};
use crate::mp4_tables::{
    Mp4PacketIndex, Mp4TableBuilder, read_chunk_offsets, read_sample_sizes, read_sample_to_chunk,
    read_timing,
};
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use stemplayer_domain::{ErrorCode, PlayerError};

pub const MAX_MP4_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_BOXES: usize = 1_000_000;
const MAX_DEPTH: usize = 32;
const MAX_TRACKS: usize = 16;
const MAX_FTYP_BYTES: usize = 4096;
const MAX_EDITS: usize = 100_000;
const MAX_PACKET_TABLE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Mp4Edit {
    pub segment_duration: u64,
    pub media_time: i64,
    pub media_rate_integer: i16,
    pub media_rate_fraction: i16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Mp4TrackStructure {
    pub track_id: u32,
    pub enabled: bool,
    pub is_audio: bool,
    pub media_timescale: Option<u32>,
    pub media_duration: Option<u64>,
    pub sample_count: Option<u32>,
    pub encoded_bytes: Option<u64>,
    pub edits: Vec<Mp4Edit>,
    pub packet_index: Option<Mp4PacketIndex>,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Mp4Structure {
    pub major_brand: String,
    pub compatible_brands: Vec<String>,
    pub file_bytes: u64,
    pub metadata_bytes_read: u64,
    pub box_count: usize,
    pub track_count: usize,
    pub movie_timescale: u32,
    pub tracks: Vec<Mp4TrackStructure>,
    pub native_roles: Option<Vec<String>>,
}

pub fn inspect_mp4_structure(
    mut reader: BoundedReader,
    cancelled: &AtomicBool,
) -> Result<Mp4Structure, PlayerError> {
    let file_bytes = reader.identity().len();
    if !(8..=MAX_MP4_BYTES).contains(&file_bytes) {
        return Err(malformed("MP4 file size is outside the supported limit"));
    }
    let mut state = ScanState::default();
    scan_boxes(&mut reader, cancelled, 0, file_bytes, 0, &mut state, None)?;
    let metadata_bytes_read = reader.bytes_read();
    let (major_brand, compatible_brands) = state
        .file_type
        .ok_or_else(|| not_mp4("File has no ISO BMFF file type box"))?;
    if !state.has_movie {
        return Err(malformed("MP4 file has no movie box"));
    }
    if state.track_count > MAX_TRACKS {
        return Err(malformed("MP4 track count exceeds the supported limit"));
    }
    let movie_timescale = state
        .movie_timescale
        .filter(|value| *value > 0)
        .ok_or_else(|| malformed("MP4 movie timescale is missing or invalid"))?;
    Ok(Mp4Structure {
        major_brand,
        compatible_brands,
        file_bytes,
        metadata_bytes_read,
        box_count: state.box_count,
        track_count: state.track_count,
        movie_timescale,
        tracks: state.tracks,
        native_roles: state.native_roles,
    })
}

#[derive(Default)]
struct ScanState {
    box_count: usize,
    track_count: usize,
    has_movie: bool,
    file_type: Option<(String, Vec<String>)>,
    movie_timescale: Option<u32>,
    tracks: Vec<Mp4TrackStructure>,
    edit_count: usize,
    packet_table_bytes: usize,
    metadata_bytes: usize,
    native_roles: Option<Vec<String>>,
}

#[derive(Default)]
struct TrackBuilder {
    track_id: Option<u32>,
    enabled: bool,
    handler: Option<[u8; 4]>,
    handler_name: Option<String>,
    media_timescale: Option<u32>,
    media_duration: Option<u64>,
    edits: Vec<Mp4Edit>,
    tables: Mp4TableBuilder,
    name: Option<String>,
}

fn scan_boxes(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    mut offset: u64,
    end: u64,
    depth: usize,
    state: &mut ScanState,
    mut track: Option<&mut TrackBuilder>,
) -> Result<(), PlayerError> {
    if depth > MAX_DEPTH {
        return Err(malformed("MP4 box nesting exceeds the supported limit"));
    }
    while offset < end {
        let remaining = end - offset;
        if remaining < 8 {
            return Err(malformed("MP4 box header is truncated"));
        }
        state.box_count = state
            .box_count
            .checked_add(1)
            .ok_or_else(|| malformed("MP4 box count overflowed"))?;
        if state.box_count > MAX_BOXES {
            return Err(malformed("MP4 box count exceeds the supported limit"));
        }

        let mut header = [0_u8; 16];
        reader.read_exact_at(offset, &mut header[..8], cancelled)?;
        let kind: [u8; 4] = header[4..8].try_into().unwrap();
        let size32 = u32::from_be_bytes(header[..4].try_into().unwrap());
        let (size, header_bytes) = match size32 {
            0 => (remaining, 8_u64),
            1 => {
                if remaining < 16 {
                    return Err(malformed("Extended MP4 box header is truncated"));
                }
                reader.read_exact_at(offset + 8, &mut header[8..16], cancelled)?;
                (u64::from_be_bytes(header[8..16].try_into().unwrap()), 16)
            }
            value => (u64::from(value), 8),
        };
        let box_end = offset
            .checked_add(size)
            .ok_or_else(|| malformed("MP4 box range overflowed"))?;
        if size < header_bytes || box_end > end {
            return Err(malformed("MP4 box range is invalid"));
        }
        let body = offset + header_bytes;

        match &kind {
            b"ftyp" if depth == 0 => {
                if state.file_type.is_some() {
                    return Err(malformed("MP4 contains multiple file type boxes"));
                }
                state.file_type = Some(read_file_type(reader, cancelled, body, box_end)?);
            }
            b"moov" if depth == 0 => {
                if state.has_movie {
                    return Err(malformed("MP4 contains multiple movie boxes"));
                }
                state.has_movie = true;
                scan_boxes(reader, cancelled, body, box_end, depth + 1, state, None)?;
            }
            b"mvhd" if track.is_none() && depth == 1 => {
                if state.movie_timescale.is_some() {
                    return Err(malformed("MP4 contains multiple movie headers"));
                }
                state.movie_timescale = Some(read_timescale(reader, cancelled, body, box_end)?.0);
            }
            b"trak" if track.is_none() && depth == 1 => {
                state.track_count = state
                    .track_count
                    .checked_add(1)
                    .ok_or_else(|| malformed("MP4 track count overflowed"))?;
                if state.track_count > MAX_TRACKS {
                    return Err(malformed("MP4 track count exceeds the supported limit"));
                }
                let mut builder = TrackBuilder::default();
                scan_boxes(
                    reader,
                    cancelled,
                    body,
                    box_end,
                    depth + 1,
                    state,
                    Some(&mut builder),
                )?;
                let structure = builder.finish(reader.identity().len())?;
                state.packet_table_bytes = state
                    .packet_table_bytes
                    .checked_add(
                        structure
                            .packet_index
                            .as_ref()
                            .map_or(0, Mp4PacketIndex::retained_bytes),
                    )
                    .ok_or_else(|| malformed("MP4 packet-table allocation overflowed"))?;
                if state.packet_table_bytes > MAX_PACKET_TABLE_BYTES {
                    return Err(malformed(
                        "MP4 packet tables exceed the supported memory limit",
                    ));
                }
                state.tracks.push(structure);
            }
            b"mdia" | b"minf" | b"stbl" | b"edts" | b"udta" => {
                scan_boxes(
                    reader,
                    cancelled,
                    body,
                    box_end,
                    depth + 1,
                    state,
                    track.as_deref_mut(),
                )?;
            }
            b"tkhd" if track.is_some() => {
                let (track_id, enabled) = read_track_header(reader, cancelled, body, box_end)?;
                let track = track.as_deref_mut().unwrap();
                if track.track_id.replace(track_id).is_some() {
                    return Err(malformed("MP4 track contains multiple track headers"));
                }
                track.enabled = enabled;
            }
            b"mdhd" if track.is_some() => {
                let (timescale, duration) = read_timescale(reader, cancelled, body, box_end)?;
                let track = track.as_deref_mut().unwrap();
                if track.media_timescale.replace(timescale).is_some() {
                    return Err(malformed("MP4 track contains multiple media headers"));
                }
                track.media_duration = Some(duration);
            }
            b"hdlr" if track.is_some() => {
                let (handler, name) = read_handler(reader, cancelled, body, box_end)?;
                let track = track.as_deref_mut().unwrap();
                if track.handler.replace(handler).is_some() {
                    return Err(malformed("MP4 track contains multiple media handlers"));
                }
                track.handler_name = name;
            }
            b"elst" if track.is_some() => {
                let edits = read_edits(reader, cancelled, body, box_end)?;
                state.edit_count = state
                    .edit_count
                    .checked_add(edits.len())
                    .ok_or_else(|| malformed("MP4 edit count overflowed"))?;
                if state.edit_count > MAX_EDITS {
                    return Err(malformed("MP4 edit count exceeds the supported limit"));
                }
                let track = track.as_deref_mut().unwrap();
                if !track.edits.is_empty() {
                    return Err(malformed("MP4 track contains multiple edit lists"));
                }
                track.edits = edits;
            }
            b"stsz" if track.is_some() => {
                let sizes = read_sample_sizes(reader, cancelled, body, box_end)?;
                track
                    .as_deref_mut()
                    .unwrap()
                    .tables
                    .set_sample_sizes(sizes)?;
            }
            b"stco" if track.is_some() => {
                let offsets = read_chunk_offsets(reader, cancelled, body, box_end, false)?;
                track
                    .as_deref_mut()
                    .unwrap()
                    .tables
                    .set_chunk_offsets(offsets)?;
            }
            b"co64" if track.is_some() => {
                let offsets = read_chunk_offsets(reader, cancelled, body, box_end, true)?;
                track
                    .as_deref_mut()
                    .unwrap()
                    .tables
                    .set_chunk_offsets(offsets)?;
            }
            b"stsc" if track.is_some() => {
                let entries = read_sample_to_chunk(reader, cancelled, body, box_end)?;
                track
                    .as_deref_mut()
                    .unwrap()
                    .tables
                    .set_sample_to_chunk(entries)?;
            }
            b"stts" if track.is_some() => {
                let entries = read_timing(reader, cancelled, body, box_end)?;
                track.as_deref_mut().unwrap().tables.set_timing(entries)?;
            }
            b"stem" if track.is_none() && depth == 2 => {
                if state.native_roles.is_some() {
                    return Err(malformed(
                        "MP4 contains multiple Native Instruments metadata boxes",
                    ));
                }
                let bytes = read_metadata(reader, cancelled, body, box_end)?;
                state.metadata_bytes = state
                    .metadata_bytes
                    .checked_add(bytes.len())
                    .ok_or_else(|| malformed("MP4 metadata byte count overflowed"))?;
                if state.metadata_bytes > MAX_METADATA_BYTES {
                    return Err(malformed("MP4 metadata exceeds the supported size"));
                }
                state.native_roles = Some(parse_native_roles(&bytes)?);
            }
            b"name" if track.is_some() => {
                let value = read_track_name(reader, cancelled, body, box_end)?;
                if track.as_deref_mut().unwrap().name.replace(value).is_some() {
                    return Err(malformed("MP4 track contains multiple names"));
                }
            }
            _ => {}
        }
        offset = box_end;
    }
    Ok(())
}

impl TrackBuilder {
    fn finish(self, file_bytes: u64) -> Result<Mp4TrackStructure, PlayerError> {
        let track_id = self
            .track_id
            .filter(|value| *value > 0)
            .ok_or_else(|| malformed("MP4 track has no track header"))?;
        let is_audio = self.handler == Some(*b"soun");
        if is_audio && self.media_timescale.is_none_or(|value| value == 0) {
            return Err(malformed("MP4 audio track has incomplete media tables"));
        }
        let packet_index = is_audio
            .then(|| self.tables.finish(file_bytes))
            .transpose()?;
        if packet_index
            .as_ref()
            .is_some_and(|index| Some(index.duration) != self.media_duration)
        {
            return Err(malformed("MP4 media duration and sample timing disagree"));
        }
        let sample_count = packet_index.as_ref().map(|index| index.sample_count);
        let encoded_bytes = packet_index.as_ref().map(|index| index.encoded_bytes);
        Ok(Mp4TrackStructure {
            track_id,
            enabled: self.enabled,
            is_audio,
            media_timescale: self.media_timescale,
            media_duration: self.media_duration,
            sample_count,
            encoded_bytes,
            edits: self.edits,
            packet_index,
            name: self.name.or(self.handler_name),
        })
    }
}

fn read_track_header(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    body: u64,
    end: u64,
) -> Result<(u32, bool), PlayerError> {
    let version_and_flags = read_array::<4>(
        reader,
        cancelled,
        body,
        end,
        "MP4 track header is truncated",
    )?;
    let flags = u32::from_be_bytes([
        0,
        version_and_flags[1],
        version_and_flags[2],
        version_and_flags[3],
    ]);
    let id_offset = match version_and_flags[0] {
        0 => 12,
        1 => 20,
        _ => return Err(malformed("MP4 track header version is unsupported")),
    };
    let id = read_array::<4>(
        reader,
        cancelled,
        body + id_offset,
        end,
        "MP4 track header is truncated",
    )?;
    Ok((u32::from_be_bytes(id), flags & 1 != 0))
}

fn read_timescale(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    body: u64,
    end: u64,
) -> Result<(u32, u64), PlayerError> {
    let version = read_array::<1>(reader, cancelled, body, end, "MP4 time header is truncated")?[0];
    match version {
        0 => {
            let bytes =
                read_array::<20>(reader, cancelled, body, end, "MP4 time header is truncated")?;
            Ok((
                u32::from_be_bytes(bytes[12..16].try_into().unwrap()),
                u64::from(u32::from_be_bytes(bytes[16..20].try_into().unwrap())),
            ))
        }
        1 => {
            let bytes =
                read_array::<32>(reader, cancelled, body, end, "MP4 time header is truncated")?;
            Ok((
                u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
                u64::from_be_bytes(bytes[24..32].try_into().unwrap()),
            ))
        }
        _ => Err(malformed("MP4 time header version is unsupported")),
    }
}

fn read_edits(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    body: u64,
    end: u64,
) -> Result<Vec<Mp4Edit>, PlayerError> {
    let header = read_array::<8>(reader, cancelled, body, end, "MP4 edit list is truncated")?;
    let entry_bytes = match header[0] {
        0 => 12_u64,
        1 => 20,
        _ => return Err(malformed("MP4 edit-list version is unsupported")),
    };
    let count = u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize;
    if count > MAX_EDITS
        || body
            .checked_add(8)
            .and_then(|value| value.checked_add(entry_bytes * count as u64))
            != Some(end)
    {
        return Err(malformed("MP4 edit-list range is invalid"));
    }
    let mut edits = Vec::with_capacity(count);
    for index in 0..count {
        let offset = body + 8 + entry_bytes * index as u64;
        let mut bytes = [0_u8; 20];
        reader.read_exact_at(offset, &mut bytes[..entry_bytes as usize], cancelled)?;
        let (segment_duration, media_time, rate_offset) = if header[0] == 0 {
            (
                u64::from(u32::from_be_bytes(bytes[..4].try_into().unwrap())),
                i64::from(i32::from_be_bytes(bytes[4..8].try_into().unwrap())),
                8,
            )
        } else {
            (
                u64::from_be_bytes(bytes[..8].try_into().unwrap()),
                i64::from_be_bytes(bytes[8..16].try_into().unwrap()),
                16,
            )
        };
        edits.push(Mp4Edit {
            segment_duration,
            media_time,
            media_rate_integer: i16::from_be_bytes(
                bytes[rate_offset..rate_offset + 2].try_into().unwrap(),
            ),
            media_rate_fraction: i16::from_be_bytes(
                bytes[rate_offset + 2..rate_offset + 4].try_into().unwrap(),
            ),
        });
    }
    Ok(edits)
}

fn read_array<const N: usize>(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    offset: u64,
    end: u64,
    summary: &str,
) -> Result<[u8; N], PlayerError> {
    if offset.checked_add(N as u64).is_none_or(|value| value > end) {
        return Err(malformed(summary));
    }
    let mut bytes = [0_u8; N];
    reader.read_exact_at(offset, &mut bytes, cancelled)?;
    Ok(bytes)
}

fn read_file_type(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    body: u64,
    end: u64,
) -> Result<(String, Vec<String>), PlayerError> {
    let length =
        usize::try_from(end - body).map_err(|_| malformed("File type box is too large"))?;
    if !(8..=MAX_FTYP_BYTES).contains(&length) || (length - 8) % 4 != 0 {
        return Err(malformed("MP4 file type box is invalid"));
    }
    let mut bytes = vec![0_u8; length];
    reader.read_exact_at(body, &mut bytes, cancelled)?;
    let major = brand(&bytes[..4])?;
    let compatible = bytes[8..]
        .chunks_exact(4)
        .map(brand)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((major, compatible))
}

fn brand(bytes: &[u8]) -> Result<String, PlayerError> {
    if !bytes
        .iter()
        .all(|byte| byte.is_ascii() && !byte.is_ascii_control())
    {
        return Err(malformed("MP4 brand contains invalid bytes"));
    }
    Ok(String::from_utf8(bytes.to_vec()).unwrap())
}

fn not_mp4(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::ContainerNotMp4, summary)
        .with_user_action("Choose a valid .stem.m4a or .stem.mp4 file")
}

fn malformed(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::ContainerMalformed, summary)
        .with_user_action("Choose a valid .stem.m4a or .stem.mp4 file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::time::{SystemTime, UNIX_EPOCH};

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
        let path = temporary_path();
        fs::write(&path, bytes).unwrap();
        let result =
            inspect_mp4_structure(BoundedReader::open(&path).unwrap(), &AtomicBool::new(false));
        fs::remove_file(path).unwrap();
        result
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
        let mut track_header = vec![0_u8; 24];
        track_header[3] = 1;
        track_header[12..16].copy_from_slice(&7_u32.to_be_bytes());

        let mut media_header = vec![0_u8; 20];
        media_header[12..16].copy_from_slice(&48_000_u32.to_be_bytes());
        media_header[16..20].copy_from_slice(&144_000_u32.to_be_bytes());

        let mut handler = vec![0_u8; 24];
        handler[8..12].copy_from_slice(b"soun");

        let mut edit = vec![0_u8; 20];
        edit[4..8].copy_from_slice(&1_u32.to_be_bytes());
        edit[8..12].copy_from_slice(&3_000_u32.to_be_bytes());
        edit[12..16].copy_from_slice(&1_024_i32.to_be_bytes());
        edit[16..18].copy_from_slice(&1_i16.to_be_bytes());

        let mut chunk_offsets = vec![0_u8; 8];
        chunk_offsets[4..8].copy_from_slice(&1_u32.to_be_bytes());
        chunk_offsets.extend_from_slice(&0_u32.to_be_bytes());

        let mut sample_to_chunk = vec![0_u8; 8];
        sample_to_chunk[4..8].copy_from_slice(&1_u32.to_be_bytes());
        sample_to_chunk.extend_from_slice(&1_u32.to_be_bytes());
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
}
