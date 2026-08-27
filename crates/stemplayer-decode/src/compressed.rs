use crate::MAX_DECODE_FRAMES;
use serde::Serialize;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use stemplayer_domain::{ErrorCode, PlayerError};
use stemplayer_domain::{SourceFrame, TrackRole};
use stemplayer_format::{
    BoundedReader, Mp4Structure, RoleMetadataSource, inspect_mp4_structure,
    resolve_mp4_structure_roles,
};
use stemplayer_timeline::{Mp4EditEntry, Mp4TrackTimeline, common_mp4_range, map_mp4_timeline};
use symphonia::core::codecs::audio::well_known::profiles::CODEC_PROFILE_AAC_LC;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AAC, CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_PCM_F32LE,
    CODEC_ID_PCM_F64LE, CODEC_ID_PCM_S8, CODEC_ID_PCM_S16LE, CODEC_ID_PCM_S24LE,
    CODEC_ID_PCM_S32LE, CODEC_ID_PCM_U8, CODEC_ID_VORBIS,
};
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

#[derive(Clone, Debug, Eq, PartialEq)]
struct CompressedFileIdentity {
    length: u64,
    modified: Option<std::time::SystemTime>,
}

const MAX_ENCODED_PACKET_BYTES: usize = 1024 * 1024;
const MAX_DECODED_PACKET_FRAMES: u64 = 16_384;
const MAX_CODEC_CONFIG_BYTES: usize = 64 * 1024;
const AAC_SEEK_PREROLL_FRAMES: i64 = 2_048;
const MP3_TERMINAL_PADDING_FRAMES: i64 = 1_152;
const MAX_CONSECUTIVE_MP3_CONCEALMENT_SECONDS: u64 = 2;

fn accepts_mp3_terminal_padding(
    codec: CompressedCodec,
    declared_frames: Option<u64>,
    current: i64,
    end: i64,
) -> bool {
    codec == CompressedCodec::Mp3
        && declared_frames == u64::try_from(end).ok()
        && end
            .checked_sub(current)
            .is_some_and(|frames| (1..=MP3_TERMINAL_PADDING_FRAMES).contains(&frames))
}

fn accepts_mp3_decode_concealment(
    codec: CompressedCodec,
    sample_rate: u32,
    concealed_frames: u64,
    frames: u64,
) -> bool {
    codec == CompressedCodec::Mp3
        && (1..=MAX_DECODED_PACKET_FRAMES).contains(&frames)
        && concealed_frames.checked_add(frames).is_some_and(|total| {
            total <= u64::from(sample_rate).saturating_mul(MAX_CONSECUTIVE_MP3_CONCEALMENT_SECONDS)
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressedCodec {
    AacLc,
    Alac,
    Flac,
    Mp3,
    Pcm,
    Vorbis,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CompressedTrackInfo {
    pub track_id: u32,
    pub codec: CompressedCodec,
    pub sample_rate: u32,
    pub channels: u8,
    pub bits_per_sample: Option<u8>,
    pub declared_frames: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompressedContainerTrack {
    pub role: TrackRole,
    pub role_source: RoleMetadataSource,
    pub info: CompressedTrackInfo,
    pub timeline: Mp4TrackTimeline,
    pub edit_count: usize,
    pub encoded_bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompressedContainer {
    pub structure: Mp4Structure,
    pub tracks: Vec<CompressedContainerTrack>,
    pub sample_rate: u32,
    pub start_frame: SourceFrame,
    pub end_frame_exclusive: SourceFrame,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CompressedDecodeDiagnostics {
    pub packets_decoded: u32,
    pub encoded_bytes_read: u64,
    pub discarded_frames: u64,
    pub required_seek_frame: i64,
    pub actual_seek_frame: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CompressedWindow {
    pub start_frame: u64,
    pub frames: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub diagnostics: CompressedDecodeDiagnostics,
}

pub fn inspect_audio_file(path: &Path) -> Result<CompressedTrackInfo, PlayerError> {
    let format = probe_format(path)?;
    let tracks = format
        .tracks()
        .iter()
        .filter(|track| track.track_type() == Some(TrackType::Audio))
        .collect::<Vec<_>>();
    if tracks.len() != 1 {
        return Err(PlayerError::new(
            ErrorCode::TrackCountUnsupported,
            "Audio files must contain exactly one audio track",
        ));
    }
    let track = tracks[0];
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|value| value.audio())
        .ok_or_else(|| codec_error("Audio track has no codec configuration"))?;
    let info = validate_parameters(track.id, track.num_frames, parameters, true)?;
    let declared_frames = bounded_audio_frames(info.declared_frames)?;
    Ok(CompressedTrackInfo {
        declared_frames: Some(declared_frames),
        ..info
    })
}

fn bounded_audio_frames(frames: Option<u64>) -> Result<u64, PlayerError> {
    frames
        .filter(|frames| *frames > 0 && i64::try_from(*frames).is_ok())
        .ok_or_else(|| {
            PlayerError::new(
                ErrorCode::TimelineInvalid,
                "Audio file duration is unavailable or exceeds the supported timeline",
            )
        })
}

pub fn inspect_compressed_tracks(path: &Path) -> Result<Vec<CompressedTrackInfo>, PlayerError> {
    let format = probe_format(path)?;
    let tracks = format
        .tracks()
        .iter()
        .filter(|track| track.track_type() == Some(TrackType::Audio))
        .map(|track| {
            let parameters = track
                .codec_params
                .as_ref()
                .and_then(|value| value.audio())
                .ok_or_else(|| codec_error("Compressed track has no audio codec configuration"))?;
            validate_parameters(track.id, track.num_frames, parameters, false)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if tracks.is_empty() {
        return Err(PlayerError::new(
            ErrorCode::NoAudioTracks,
            "Stem container contains no audio tracks",
        ));
    }
    if tracks.len() > 16 {
        return Err(PlayerError::new(
            ErrorCode::TrackCountUnsupported,
            "Stem container contains more than 16 tracks",
        ));
    }
    Ok(tracks)
}

pub fn inspect_compressed_container(
    path: &Path,
    cancelled: &AtomicBool,
) -> Result<CompressedContainer, PlayerError> {
    let structure = inspect_mp4_structure(BoundedReader::open(path)?, cancelled)?;
    let roles = resolve_mp4_structure_roles(&structure)?;
    let codec_tracks = inspect_compressed_tracks(path)?;
    let mut tracks = Vec::with_capacity(roles.len());
    for resolved in roles {
        let descriptor = structure
            .tracks
            .iter()
            .find(|track| track.track_id == resolved.track_id)
            .ok_or_else(|| configuration_error("Resolved MP4 track is missing"))?;
        let info = codec_tracks
            .iter()
            .find(|track| track.track_id == resolved.track_id)
            .cloned()
            .ok_or_else(|| configuration_error("Resolved MP4 track has no qualified codec"))?;
        let decoded_frames = info
            .declared_frames
            .ok_or_else(|| configuration_error("Compressed track frame count is unavailable"))?;
        let media_timescale = descriptor
            .media_timescale
            .ok_or_else(|| configuration_error("Compressed track timescale is unavailable"))?;
        let edits = descriptor
            .edits
            .iter()
            .map(|edit| Mp4EditEntry {
                segment_duration: edit.segment_duration,
                media_time: edit.media_time,
                media_rate_integer: edit.media_rate_integer,
                media_rate_fraction: edit.media_rate_fraction,
            })
            .collect::<Vec<_>>();
        let timeline = map_mp4_timeline(
            structure.movie_timescale,
            media_timescale,
            info.sample_rate,
            decoded_frames,
            &edits,
        )?;
        tracks.push(CompressedContainerTrack {
            role: resolved.role,
            role_source: resolved.source,
            info,
            timeline,
            edit_count: edits.len(),
            encoded_bytes: descriptor.encoded_bytes.unwrap_or(0),
        });
    }
    if codec_tracks.iter().any(|codec| {
        !tracks
            .iter()
            .any(|track| track.info.track_id == codec.track_id)
    }) {
        return Err(configuration_error(
            "Stem container contains an unresolved audio track",
        ));
    }
    let sample_rate = tracks
        .first()
        .map(|track| track.info.sample_rate)
        .ok_or_else(|| configuration_error("Stem container has no routed tracks"))?;
    if tracks
        .iter()
        .any(|track| track.info.sample_rate != sample_rate)
    {
        return Err(configuration_error(
            "Stem container tracks use different sample rates",
        ));
    }
    let timelines = tracks
        .iter()
        .map(|track| track.timeline.clone())
        .collect::<Vec<_>>();
    let (start_frame, end_frame_exclusive) = common_mp4_range(&timelines)?;
    Ok(CompressedContainer {
        structure,
        tracks,
        sample_rate,
        start_frame,
        end_frame_exclusive,
    })
}

fn probe_format(path: &Path) -> Result<Box<dyn FormatReader>, PlayerError> {
    compressed_file_identity(path)?;
    let file = File::open(path).map_err(|_| demux_error("Stem container cannot be opened"))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|_| demux_error("Stem container cannot be demultiplexed"))
}
pub struct CompressedTrackDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    info: CompressedTrackInfo,
    path: PathBuf,
    identity: CompressedFileIdentity,
    next_frame: Option<u64>,
    pending: Option<PendingDecoded>,
    consecutive_concealed_frames: u64,
}

struct PendingDecoded {
    packet_start: i64,
    packet_end: i64,
    trim_start: u64,
    frames: u64,
    interleaved: Vec<f32>,
}

impl CompressedTrackDecoder {
    pub fn open(path: &Path, track_id: u32) -> Result<Self, PlayerError> {
        let identity = compressed_file_identity(path)?;
        let format = probe_format(path)?;
        let track = format
            .tracks()
            .iter()
            .find(|track| track.id == track_id && track.track_type() == Some(TrackType::Audio))
            .ok_or_else(|| {
                PlayerError::new(
                    ErrorCode::TrackConfigurationMismatch,
                    "Requested compressed audio track is unavailable",
                )
            })?;
        let parameters = track
            .codec_params
            .as_ref()
            .and_then(|value| value.audio())
            .cloned()
            .ok_or_else(|| codec_error("Compressed track has no audio codec configuration"))?;
        let info = validate_parameters(track_id, track.num_frames, &parameters, false)?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&parameters, &AudioDecoderOptions::default())
            .map_err(|_| codec_error("Compressed track decoder cannot be created"))?;
        Ok(Self {
            format,
            decoder,
            info,
            path: path.to_path_buf(),
            identity,
            next_frame: None,
            pending: None,
            consecutive_concealed_frames: 0,
        })
    }

    pub fn open_audio(path: &Path, track_id: u32) -> Result<Self, PlayerError> {
        let identity = compressed_file_identity(path)?;
        let format = probe_format(path)?;
        let track = format
            .tracks()
            .iter()
            .find(|track| track.id == track_id && track.track_type() == Some(TrackType::Audio))
            .ok_or_else(|| configuration_error("Requested audio track is unavailable"))?;
        let parameters = track
            .codec_params
            .as_ref()
            .and_then(|value| value.audio())
            .cloned()
            .ok_or_else(|| codec_error("Audio track has no codec configuration"))?;
        let info = validate_parameters(track_id, track.num_frames, &parameters, true)?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&parameters, &AudioDecoderOptions::default())
            .map_err(|_| codec_error("Audio track decoder cannot be created"))?;
        Ok(Self {
            format,
            decoder,
            info,
            path: path.to_path_buf(),
            identity,
            next_frame: None,
            pending: None,
            consecutive_concealed_frames: 0,
        })
    }

    pub const fn info(&self) -> &CompressedTrackInfo {
        &self.info
    }

    pub fn decode_window(
        &mut self,
        start_frame: u64,
        requested_frames: u32,
        cancelled: &AtomicBool,
    ) -> Result<CompressedWindow, PlayerError> {
        if compressed_file_identity(&self.path)? != self.identity {
            return Err(PlayerError::new(
                ErrorCode::BundleInconsistent,
                "Stem container changed after inspection",
            ));
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(PlayerError::new(
                ErrorCode::BundleAborted,
                "Compressed decode generation was cancelled",
            ));
        }
        if requested_frames > MAX_DECODE_FRAMES {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Compressed decode window exceeds 4096 frames",
            ));
        }
        if requested_frames == 0 {
            return Ok(CompressedWindow {
                start_frame,
                frames: 0,
                left: Vec::new(),
                right: Vec::new(),
                diagnostics: CompressedDecodeDiagnostics::default(),
            });
        }
        let target = i64::try_from(start_frame).map_err(|_| {
            PlayerError::new(
                ErrorCode::PresentationRangeInvalid,
                "Compressed decode start exceeds the signed timeline",
            )
        })?;
        let end = target
            .checked_add(i64::from(requested_frames))
            .ok_or_else(|| {
                PlayerError::new(
                    ErrorCode::PresentationRangeInvalid,
                    "Compressed decode window overflowed",
                )
            })?;
        let seeking = self.next_frame != Some(start_frame);
        let mut diagnostics = CompressedDecodeDiagnostics {
            required_seek_frame: target,
            actual_seek_frame: target,
            ..Default::default()
        };
        if seeking {
            let seek_target = match self.info.codec {
                CompressedCodec::AacLc => target.saturating_sub(AAC_SEEK_PREROLL_FRAMES).max(0),
                _ => target,
            };
            let seek = self
                .format
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Timestamp {
                        ts: Timestamp::new(seek_target),
                        track_id: self.info.track_id,
                    },
                )
                .map_err(|_| demux_error("Compressed track seek failed"))?;
            self.decoder.reset();
            self.pending = None;
            self.consecutive_concealed_frames = 0;
            diagnostics.required_seek_frame = seek.required_ts.get();
            diagnostics.actual_seek_frame = seek.actual_ts.get();
        }
        let mut current = target;
        let mut left = Vec::with_capacity(requested_frames as usize);
        let mut right = Vec::with_capacity(requested_frames as usize);
        while current < end {
            if cancelled.load(Ordering::Acquire) {
                return Err(PlayerError::new(
                    ErrorCode::BundleAborted,
                    "Compressed decode generation was cancelled",
                ));
            }
            if self.pending.is_none() {
                self.pending = self.decode_next_packet(&mut diagnostics)?;
                if self.pending.is_none() {
                    if accepts_mp3_terminal_padding(
                        self.info.codec,
                        self.info.declared_frames,
                        current,
                        end,
                    ) {
                        left.resize(requested_frames as usize, 0.0);
                        right.resize(requested_frames as usize, 0.0);
                        current = end;
                        continue;
                    }
                    return Err(PlayerError::new(
                        ErrorCode::EncodedSampleInvalid,
                        "Compressed track ended before the requested frame",
                    ));
                }
            }
            let pending = self.pending.as_ref().unwrap();
            if pending.packet_end <= current {
                if seeking {
                    diagnostics.discarded_frames = diagnostics.discarded_frames.saturating_add(
                        u64::try_from(
                            pending
                                .packet_end
                                .saturating_sub(pending.packet_start)
                                .max(0),
                        )
                        .unwrap(),
                    );
                }
                self.pending = None;
                continue;
            }
            if pending.packet_start > current {
                return Err(PlayerError::new(
                    ErrorCode::AudioDataInvalid,
                    "Compressed track contains an unexplained frame gap",
                ));
            }
            let overlap_end = pending.packet_end.min(end);
            let source_offset = pending
                .trim_start
                .checked_add(u64::try_from(current - pending.packet_start).unwrap())
                .ok_or_else(|| {
                    PlayerError::new(
                        ErrorCode::AudioDataInvalid,
                        "Decoded packet trim offset overflowed",
                    )
                })?;
            let copy_frames = u64::try_from(overlap_end - current).unwrap();
            if source_offset
                .checked_add(copy_frames)
                .is_none_or(|value| value > pending.frames)
            {
                return Err(PlayerError::new(
                    ErrorCode::AudioDataInvalid,
                    format!(
                        "Decoded packet does not cover its presentation range: offset {source_offset}, copy {copy_frames}, frames {}",
                        pending.frames
                    ),
                ));
            }

            for frame in source_offset..source_offset + copy_frames {
                let channels = usize::from(self.info.channels);
                let index = usize::try_from(frame).unwrap() * channels;
                left.push(pending.interleaved[index]);
                right.push(pending.interleaved[index + usize::from(channels == 2)]);
            }
            if seeking && current == target {
                diagnostics.discarded_frames =
                    diagnostics.discarded_frames.saturating_add(source_offset);
            }
            current = overlap_end;
            if current == pending.packet_end {
                self.pending = None;
            }
        }
        self.next_frame = Some(u64::try_from(end).unwrap());
        Ok(CompressedWindow {
            start_frame,
            frames: requested_frames,
            left,
            right,
            diagnostics,
        })
    }

    fn decode_next_packet(
        &mut self,
        diagnostics: &mut CompressedDecodeDiagnostics,
    ) -> Result<Option<PendingDecoded>, PlayerError> {
        loop {
            let Some(packet) = self
                .format
                .next_packet()
                .map_err(|_| demux_error("Compressed packet read failed"))?
            else {
                return Ok(None);
            };
            if packet.track_id != self.info.track_id {
                continue;
            }
            if packet.data.len() > MAX_ENCODED_PACKET_BYTES {
                return Err(PlayerError::new(
                    ErrorCode::EncodedSampleInvalid,
                    "Compressed packet exceeds the supported size",
                ));
            }
            let encoded_packet_start = packet.pts.get();
            let packet_end = encoded_packet_start
                .checked_add_unsigned(packet.dur.get())
                .ok_or_else(|| {
                    PlayerError::new(
                        ErrorCode::EncodedSampleInvalid,
                        "Compressed packet timestamp overflowed",
                    )
                })?;
            diagnostics.encoded_bytes_read = diagnostics
                .encoded_bytes_read
                .saturating_add(packet.data.len() as u64);
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(SymphoniaError::DecodeError(_))
                    if accepts_mp3_decode_concealment(
                        self.info.codec,
                        self.info.sample_rate,
                        self.consecutive_concealed_frames,
                        packet.dur.get(),
                    ) =>
                {
                    let frames = packet.dur.get();
                    self.consecutive_concealed_frames += frames;
                    let packet_start = encoded_packet_start
                        .checked_add_unsigned(packet.trim_start.get())
                        .ok_or_else(|| {
                            PlayerError::new(
                                ErrorCode::EncodedSampleInvalid,
                                "Audio packet trim timestamp overflowed",
                            )
                        })?;
                    return Ok(Some(PendingDecoded {
                        packet_start,
                        packet_end,
                        trim_start: 0,
                        frames,
                        interleaved: vec![
                            0.0;
                            usize::try_from(frames).unwrap()
                                * usize::from(self.info.channels)
                        ],
                    }));
                }
                Err(_) => {
                    return Err(PlayerError::new(
                        ErrorCode::DecodeFailed,
                        "Audio packet decode failed",
                    ));
                }
            };
            self.consecutive_concealed_frames = 0;
            diagnostics.packets_decoded = diagnostics.packets_decoded.saturating_add(1);
            let frames = u64::try_from(decoded.frames()).unwrap();
            if frames > MAX_DECODED_PACKET_FRAMES
                || matches!(
                    self.info.codec,
                    CompressedCodec::AacLc | CompressedCodec::Alac
                ) && frames < packet.block_dur().get()
                || decoded.spec().channels().count() != usize::from(self.info.channels)
                || decoded.spec().rate() != self.info.sample_rate
            {
                return Err(PlayerError::new(
                    ErrorCode::AudioDataInvalid,
                    "Decoded packet does not match the qualified audio track",
                ));
            }
            let mut interleaved = vec![0.0_f32; decoded.frames() * usize::from(self.info.channels)];
            decoded.copy_to_slice_interleaved(&mut interleaved);
            if interleaved.iter().any(|sample| !sample.is_finite()) {
                return Err(PlayerError::new(
                    ErrorCode::AudioDataInvalid,
                    "Decoded packet contains a non-finite sample",
                ));
            }
            let packet_trim = packet.trim_start.get();
            let (packet_start, trim_start) = if matches!(
                self.info.codec,
                CompressedCodec::AacLc | CompressedCodec::Alac
            ) {
                (encoded_packet_start, packet_trim)
            } else {
                (
                    encoded_packet_start
                        .checked_add_unsigned(packet_trim)
                        .ok_or_else(|| {
                            PlayerError::new(
                                ErrorCode::EncodedSampleInvalid,
                                "Audio packet trim timestamp overflowed",
                            )
                        })?,
                    0,
                )
            };
            return Ok(Some(PendingDecoded {
                packet_start,
                packet_end,
                trim_start,
                frames,
                interleaved,
            }));
        }
    }
}

fn validate_parameters(
    track_id: u32,
    declared_frames: Option<u64>,
    parameters: &AudioCodecParameters,
    ordinary: bool,
) -> Result<CompressedTrackInfo, PlayerError> {
    let codec = if parameters.codec == CODEC_ID_AAC {
        if parameters.profile != Some(CODEC_PROFILE_AAC_LC) {
            return Err(PlayerError::new(
                ErrorCode::CodecUnsupported,
                "AAC track must use the Low Complexity profile",
            ));
        }
        CompressedCodec::AacLc
    } else if parameters.codec == CODEC_ID_ALAC {
        if !matches!(alac_bit_depth(parameters), Some(16 | 20 | 24 | 32)) {
            return Err(codec_error(
                "ALAC track must use a qualified 16, 20, 24, or 32-bit configuration",
            ));
        }
        CompressedCodec::Alac
    } else if ordinary && parameters.codec == CODEC_ID_FLAC {
        CompressedCodec::Flac
    } else if ordinary && parameters.codec == CODEC_ID_MP3 {
        CompressedCodec::Mp3
    } else if ordinary && parameters.codec == CODEC_ID_VORBIS {
        CompressedCodec::Vorbis
    } else if ordinary
        && matches!(
            parameters.codec,
            CODEC_ID_PCM_S8
                | CODEC_ID_PCM_U8
                | CODEC_ID_PCM_S16LE
                | CODEC_ID_PCM_S24LE
                | CODEC_ID_PCM_S32LE
                | CODEC_ID_PCM_F32LE
                | CODEC_ID_PCM_F64LE
        )
    {
        CompressedCodec::Pcm
    } else {
        return Err(PlayerError::new(
            ErrorCode::CodecUnsupported,
            "Compressed track must use AAC-LC or ALAC",
        ));
    };
    let sample_rate = parameters
        .sample_rate
        .filter(|rate| {
            if ordinary {
                (8_000..=384_000).contains(rate)
            } else {
                matches!(rate, 44_100 | 48_000)
            }
        })
        .ok_or_else(|| codec_error("Audio track sample rate is unsupported"))?;
    let channels = parameters
        .channels
        .as_ref()
        .filter(|channels| {
            if ordinary {
                matches!(channels.count(), 1 | 2)
            } else {
                channels.count() == 2
            }
        })
        .ok_or_else(|| codec_error("Audio track must contain one or two channels"))?;
    if parameters
        .extra_data
        .as_ref()
        .is_some_and(|data| data.len() > MAX_CODEC_CONFIG_BYTES)
        || parameters
            .max_frames_per_packet
            .is_some_and(|frames| frames > MAX_DECODED_PACKET_FRAMES)
    {
        return Err(codec_error(
            "Compressed track codec configuration exceeds the supported limits",
        ));
    }
    Ok(CompressedTrackInfo {
        track_id,
        codec,
        sample_rate,
        channels: u8::try_from(channels.count()).unwrap(),
        bits_per_sample: if codec == CompressedCodec::Alac {
            alac_bit_depth(parameters)
        } else {
            parameters
                .bits_per_sample
                .and_then(|value| u8::try_from(value).ok())
        },
        declared_frames,
    })
}

fn alac_bit_depth(parameters: &AudioCodecParameters) -> Option<u8> {
    parameters
        .bits_per_sample
        .and_then(|value| u8::try_from(value).ok())
        .or_else(|| {
            let data = parameters.extra_data.as_deref()?;
            match data {
                [_, _, _, _, _, depth, ..] if data.len() == 24 => Some(*depth),
                [
                    _,
                    _,
                    _,
                    _,
                    b'a',
                    b'l',
                    b'a',
                    b'c',
                    _,
                    _,
                    _,
                    _,
                    _,
                    _,
                    _,
                    _,
                    _,
                    depth,
                    ..,
                ] if data.len() == 36 => Some(*depth),
                _ => None,
            }
        })
}

fn compressed_file_identity(path: &Path) -> Result<CompressedFileIdentity, PlayerError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| demux_error("Stem container metadata is unavailable"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(demux_error("Stem container is not a regular local file"));
    }
    Ok(CompressedFileIdentity {
        length: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn demux_error(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::DemuxFailed, summary)
        .with_user_action("Choose a valid local stem container")
}

fn codec_error(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::CodecConfigInvalid, summary)
        .with_user_action("Choose a stem container using stereo AAC-LC or ALAC")
}

fn configuration_error(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::TrackConfigurationMismatch, summary)
        .with_user_action("Choose a stem container with compatible routed tracks")
}

#[cfg(test)]
mod tests {
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
    fn local_file_identity_detects_replacement() {
        let path = std::env::temp_dir().join(format!(
            "stemplayer-compressed-identity-{}",
            std::process::id()
        ));
        fs::write(&path, b"first").unwrap();
        let identity = compressed_file_identity(&path).unwrap();
        fs::write(&path, b"replacement").unwrap();
        assert_ne!(compressed_file_identity(&path).unwrap(), identity);
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
            let mut decoder =
                CompressedTrackDecoder::open(Path::new(&path), track.track_id).unwrap();
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
            let mut seeked =
                CompressedTrackDecoder::open(Path::new(&path), track.track_id).unwrap();
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
}
