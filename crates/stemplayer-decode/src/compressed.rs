use crate::MAX_DECODE_FRAMES;
use crate::media::{FileAccess, Mp4SeekGuard, check_cancelled, operation_error, probe_file};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use stemplayer_domain::{ErrorCode, PlayerError};
use stemplayer_domain::{SourceFrame, TrackRole};
use stemplayer_format::{
    FileIdentity, Mp4Structure, RoleMetadataSource, resolve_mp4_structure_roles,
};
use stemplayer_timeline::{Mp4TrackTimeline, common_mp4_range};
use symphonia::core::codecs::audio::well_known::profiles::CODEC_PROFILE_AAC_LC;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AAC, CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_PCM_F32LE,
    CODEC_ID_PCM_F64LE, CODEC_ID_PCM_S8, CODEC_ID_PCM_S16LE, CODEC_ID_PCM_S24LE,
    CODEC_ID_PCM_S32LE, CODEC_ID_PCM_U8, CODEC_ID_VORBIS,
};
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::units::Timestamp;

mod inspection;
mod parameters;

use parameters::validate_parameters;

#[cfg(test)]
use inspection::bounded_audio_frames;
use inspection::inspected_track;
pub use inspection::{
    inspect_audio_file, inspect_audio_file_cancelled, inspect_compressed_container,
    inspect_compressed_tracks,
};

const MAX_ENCODED_PACKET_BYTES: usize = stemplayer_format::MAX_MP4_PACKET_BYTES as usize;
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
    #[serde(skip)]
    pub timeline: Option<Mp4TrackTimeline>,
    #[serde(skip)]
    identity: Option<FileIdentity>,
    #[serde(skip)]
    media_frames: Option<u64>,
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

pub struct CompressedTrackDecoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    info: CompressedTrackInfo,
    access: Arc<FileAccess>,
    cancelled: Arc<AtomicBool>,
    seek_guard: Option<Mp4SeekGuard>,
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
        let info = inspect_compressed_tracks(path)?
            .into_iter()
            .find(|track| track.track_id == track_id)
            .ok_or_else(|| configuration_error("Requested compressed track is unavailable"))?;
        Self::open_inspected(path, &info, Arc::new(AtomicBool::new(false)))
    }

    pub fn open_audio(path: &Path, track_id: u32) -> Result<Self, PlayerError> {
        let info = inspect_audio_file(path)?;
        if info.track_id != track_id {
            return Err(configuration_error("Requested audio track is unavailable"));
        }
        Self::open_audio_inspected(path, &info, Arc::new(AtomicBool::new(false)))
    }

    pub fn open_inspected(
        path: &Path,
        expected: &CompressedTrackInfo,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, PlayerError> {
        Self::open_validated(path, expected, false, cancelled)
    }

    pub fn open_audio_inspected(
        path: &Path,
        expected: &CompressedTrackInfo,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, PlayerError> {
        Self::open_validated(path, expected, true, cancelled)
    }

    fn open_validated(
        path: &Path,
        expected: &CompressedTrackInfo,
        ordinary: bool,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, PlayerError> {
        let identity = expected
            .identity
            .as_ref()
            .ok_or_else(|| configuration_error("Audio track has not been inspected"))?;
        let probed = probe_file(path, Some(identity), false, Arc::clone(&cancelled))?;
        let track = probed
            .format
            .tracks()
            .iter()
            .find(|track| {
                track.id == expected.track_id && track.track_type() == Some(TrackType::Audio)
            })
            .ok_or_else(|| configuration_error("Requested audio track is unavailable"))?;
        let info = inspected_track(&probed, track, ordinary)?;
        if &info != expected {
            return Err(configuration_error("Audio track changed after inspection"));
        }
        let parameters = track
            .codec_params
            .as_ref()
            .and_then(|value| value.audio())
            .cloned()
            .ok_or_else(|| codec_error("Audio track has no codec configuration"))?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&parameters, &AudioDecoderOptions::default())
            .map_err(|_| codec_error("Audio track decoder cannot be created"))?;
        let seek_guard = probed
            .structure
            .as_ref()
            .map(|structure| {
                Mp4SeekGuard::new(probed.format.as_ref(), structure, expected.track_id)
            })
            .transpose()?;
        Ok(Self {
            format: probed.format,
            decoder,
            info,
            access: probed.access,
            cancelled,
            seek_guard,
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
        check_cancelled(cancelled)?;
        check_cancelled(&self.cancelled)?;
        self.access.ensure_unchanged()?;
        if self.info.timeline.is_none() {
            return self.decode_media_window(start_frame, requested_frames, cancelled);
        }
        if requested_frames > MAX_DECODE_FRAMES {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Audio decode window exceeds 4096 frames",
            ));
        }
        let end = start_frame
            .checked_add(u64::from(requested_frames))
            .filter(|end| {
                self.info
                    .declared_frames
                    .is_some_and(|frames| *end <= frames)
            })
            .ok_or_else(|| {
                configuration_error("Audio decode range exceeds its presentation timeline")
            })?;
        let mut left = vec![0.0; requested_frames as usize];
        let mut right = vec![0.0; requested_frames as usize];
        let mut diagnostics = CompressedDecodeDiagnostics::default();
        let mut covered = 0_u64;
        for index in 0..self.info.timeline.as_ref().unwrap().segments.len() {
            check_cancelled(cancelled)?;
            check_cancelled(&self.cancelled)?;
            let segment = self.info.timeline.as_ref().unwrap().segments[index];
            let segment_start = u64::try_from(segment.start_frame.get()).unwrap();
            let segment_end = u64::try_from(segment.end_frame_exclusive.get()).unwrap();
            let first = start_frame.max(segment_start);
            let last = end.min(segment_end);
            if first >= last {
                continue;
            }
            if let Some(media_start) = segment.media_start_frame {
                let media_frame = u64::try_from(media_start.get()).unwrap() + first - segment_start;
                let window =
                    self.decode_media_window(media_frame, (last - first) as u32, cancelled)?;
                let offset = (first - start_frame) as usize;
                let window_end = offset + window.frames as usize;
                left[offset..window_end].copy_from_slice(&window.left);
                right[offset..window_end].copy_from_slice(&window.right);
                diagnostics.packets_decoded = diagnostics
                    .packets_decoded
                    .saturating_add(window.diagnostics.packets_decoded);
                diagnostics.encoded_bytes_read = diagnostics
                    .encoded_bytes_read
                    .saturating_add(window.diagnostics.encoded_bytes_read);
                diagnostics.discarded_frames = diagnostics
                    .discarded_frames
                    .saturating_add(window.diagnostics.discarded_frames);
                diagnostics.required_seek_frame = window.diagnostics.required_seek_frame;
                diagnostics.actual_seek_frame = window.diagnostics.actual_seek_frame;
            }
            covered += last - first;
        }
        if covered != u64::from(requested_frames) {
            return Err(configuration_error(
                "Audio presentation timeline has a frame gap",
            ));
        }
        self.access.ensure_unchanged()?;
        check_cancelled(cancelled)?;
        check_cancelled(&self.cancelled)?;
        Ok(CompressedWindow {
            start_frame,
            frames: requested_frames,
            left,
            right,
            diagnostics,
        })
    }

    fn decode_media_window(
        &mut self,
        start_frame: u64,
        requested_frames: u32,
        cancelled: &AtomicBool,
    ) -> Result<CompressedWindow, PlayerError> {
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
        self.next_frame = None;
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
            if let Some(guard) = &self.seek_guard {
                guard.check(seek_target)?;
            }
            let seek = self
                .format
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Timestamp {
                        ts: Timestamp::new(seek_target),
                        track_id: self.info.track_id,
                    },
                )
                .map_err(|_| {
                    operation_error(
                        &self.access,
                        &self.cancelled,
                        "Compressed track seek failed",
                    )
                })?;
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
            check_cancelled(&self.cancelled)?;
            if cancelled.load(Ordering::Acquire) {
                return Err(PlayerError::new(
                    ErrorCode::BundleAborted,
                    "Compressed decode generation was cancelled",
                ));
            }
            if self.pending.is_none() {
                self.pending = self.decode_next_packet(&mut diagnostics, cancelled)?;
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
        self.access.ensure_unchanged()?;
        check_cancelled(cancelled)?;
        check_cancelled(&self.cancelled)?;
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
        cancelled: &AtomicBool,
    ) -> Result<Option<PendingDecoded>, PlayerError> {
        loop {
            check_cancelled(cancelled)?;
            check_cancelled(&self.cancelled)?;
            let Some(packet) = self.format.next_packet().map_err(|_| {
                operation_error(
                    &self.access,
                    &self.cancelled,
                    "Compressed packet read failed",
                )
            })?
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

fn codec_error(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::CodecConfigInvalid, summary)
        .with_user_action("Choose a stem container using stereo AAC-LC or ALAC")
}

fn configuration_error(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::TrackConfigurationMismatch, summary)
        .with_user_action("Choose a stem container with compatible routed tracks")
}

#[cfg(test)]
mod tests;
