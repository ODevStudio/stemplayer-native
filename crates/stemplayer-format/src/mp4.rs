use crate::BoundedReader;
use crate::mp4_descriptors::validate_sample_description;
use crate::mp4_metadata::{
    MAX_METADATA_BYTES, parse_native_roles, read_handler, read_metadata, read_track_name,
};
use crate::mp4_reader::Mp4Reader;
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
const MAX_SAMPLE_DESCRIPTION_BYTES: u64 = 64 * 1024 + 256;

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
    inspect_mp4(&mut Mp4Reader::File(&mut reader), cancelled)
}

pub fn validate_mp4_for_demux(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
) -> Result<(), PlayerError> {
    inspect_mp4(&mut Mp4Reader::File(reader), cancelled).map(|_| ())
}

pub fn validate_mp4_bytes_for_demux(
    bytes: &[u8],
    cancelled: &AtomicBool,
) -> Result<(), PlayerError> {
    inspect_mp4_bytes(bytes, cancelled).map(|_| ())
}

pub fn inspect_mp4_bytes(
    bytes: &[u8],
    cancelled: &AtomicBool,
) -> Result<Mp4Structure, PlayerError> {
    inspect_mp4(
        &mut Mp4Reader::Bytes {
            bytes,
            bytes_read: 0,
        },
        cancelled,
    )
}

fn inspect_mp4(
    reader: &mut Mp4Reader<'_>,
    cancelled: &AtomicBool,
) -> Result<Mp4Structure, PlayerError> {
    let file_bytes = reader.len();
    if !(8..=MAX_MP4_BYTES).contains(&file_bytes) {
        return Err(malformed("MP4 file size is outside the supported limit"));
    }
    let mut state = ScanState::default();
    scan_boxes(reader, cancelled, 0, file_bytes, 0, &mut state, None)?;
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
    has_sample_description: bool,
}

fn scan_boxes(
    reader: &mut Mp4Reader<'_>,
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
                let structure = builder.finish(reader.len())?;
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
            b"moof" | b"mvex" => {
                return Err(malformed("Fragmented MP4 is outside the supported profile"));
            }
            b"meta" => {
                add_metadata_bytes(state, box_end - body)?;
            }
            b"stsd" if track.is_some() => {
                if box_end - body > MAX_SAMPLE_DESCRIPTION_BYTES {
                    return Err(malformed(
                        "MP4 sample description exceeds the supported size",
                    ));
                }
                let track = track.as_deref_mut().unwrap();
                if track.has_sample_description {
                    return Err(malformed("MP4 track contains multiple sample descriptions"));
                }
                let mut bytes = vec![0_u8; (box_end - body) as usize];
                for (index, chunk) in bytes.chunks_mut(64 * 1024).enumerate() {
                    reader.read_exact_at(body + (index * 64 * 1024) as u64, chunk, cancelled)?;
                }
                validate_sample_description(&bytes)?;
                track.has_sample_description = true;
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
                add_metadata_bytes(state, box_end - body)?;
                let bytes = read_metadata(reader, cancelled, body, box_end)?;
                state.native_roles = Some(parse_native_roles(&bytes)?);
            }
            b"name" if track.is_some() => {
                add_metadata_bytes(state, box_end - body)?;
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

fn add_metadata_bytes(state: &mut ScanState, bytes: u64) -> Result<(), PlayerError> {
    state.metadata_bytes = usize::try_from(bytes)
        .ok()
        .and_then(|bytes| state.metadata_bytes.checked_add(bytes))
        .filter(|bytes| *bytes <= MAX_METADATA_BYTES)
        .ok_or_else(|| malformed("MP4 metadata exceeds the supported size"))?;
    Ok(())
}

impl TrackBuilder {
    fn finish(self, file_bytes: u64) -> Result<Mp4TrackStructure, PlayerError> {
        let track_id = self
            .track_id
            .filter(|value| *value > 0)
            .ok_or_else(|| malformed("MP4 track has no track header"))?;
        let is_audio = self.handler == Some(*b"soun");
        if self.media_timescale.is_none_or(|value| value == 0) {
            return Err(malformed("MP4 track has incomplete media tables"));
        }
        let packet_index = self.tables.finish(file_bytes)?;
        if Some(packet_index.duration) != self.media_duration {
            return Err(malformed("MP4 media duration and sample timing disagree"));
        }
        Ok(Mp4TrackStructure {
            track_id,
            enabled: self.enabled,
            is_audio,
            media_timescale: self.media_timescale,
            media_duration: self.media_duration,
            sample_count: Some(packet_index.sample_count),
            encoded_bytes: Some(packet_index.encoded_bytes),
            edits: self.edits,
            packet_index: Some(packet_index),
            name: self.name.or(self.handler_name),
        })
    }
}

fn read_track_header(
    reader: &mut Mp4Reader<'_>,
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
    reader: &mut Mp4Reader<'_>,
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
    reader: &mut Mp4Reader<'_>,
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
    reader: &mut Mp4Reader<'_>,
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
    reader: &mut Mp4Reader<'_>,
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
#[path = "mp4_tests.rs"]
mod tests;
