use serde::Serialize;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use stemplayer_domain::{ErrorCode, PlayerError};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia::core::codecs::audio::well_known::profiles::CODEC_PROFILE_AAC_LC;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

const MAX_PACKET_BYTES: usize = 1024 * 1024;
const MAX_DECODED_FRAMES: usize = 2_000_000;
const AAC_PACKET_FRAMES: usize = 1024;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DecodedSegment {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub packets: u32,
}

pub fn decode_aac_segment(
    bytes: Vec<u8>,
    sample_rate: u32,
    trim_start_frames: usize,
    expected_frames: usize,
    cancelled: &AtomicBool,
) -> Result<DecodedSegment, PlayerError> {
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("m4a");
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|_| {
            error(
                ErrorCode::HlsCodecUnsupported,
                "HLS segment is not a supported M4A file",
            )
        })?;
    let tracks = format
        .tracks()
        .iter()
        .filter(|track| track.track_type() == Some(TrackType::Audio))
        .collect::<Vec<_>>();
    if tracks.len() != 1 {
        return Err(error(
            ErrorCode::HlsCodecUnsupported,
            "HLS segment must contain one audio track",
        ));
    }
    let track = tracks[0];
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|value| value.audio())
        .cloned()
        .ok_or_else(|| {
            error(
                ErrorCode::HlsCodecUnsupported,
                "HLS segment has no audio codec configuration",
            )
        })?;
    if parameters.codec != CODEC_ID_AAC
        || parameters.profile != Some(CODEC_PROFILE_AAC_LC)
        || parameters.sample_rate != Some(sample_rate)
        || parameters.channels.as_ref().map(|value| value.count()) != Some(2)
    {
        return Err(error(
            ErrorCode::HlsCodecUnsupported,
            "HLS segment must use stereo AAC-LC at the declared sample rate",
        ));
    }
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&parameters, &AudioDecoderOptions::default())
        .map_err(|_| {
            error(
                ErrorCode::HlsCodecUnsupported,
                "HLS AAC decoder cannot be created",
            )
        })?;
    let maximum = expected_frames
        .checked_add(trim_start_frames)
        .and_then(|value| value.checked_add(16_384))
        .filter(|value| *value <= MAX_DECODED_FRAMES)
        .ok_or_else(|| {
            error(
                ErrorCode::HlsResponseTooLarge,
                "HLS decoded segment exceeds its frame limit",
            )
        })?;
    let mut left = Vec::with_capacity(maximum.min(expected_frames + trim_start_frames));
    let mut right = Vec::with_capacity(left.capacity());
    let mut packets = 0_u32;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(error(
                ErrorCode::BundleAborted,
                "HLS segment decode was cancelled",
            ));
        }
        let Some(packet) = format
            .next_packet()
            .map_err(|_| error(ErrorCode::DecodeFailed, "HLS segment packet read failed"))?
        else {
            break;
        };
        if packet.track_id != track_id {
            continue;
        }
        if packet.data.len() > MAX_PACKET_BYTES {
            return Err(error(
                ErrorCode::HlsResponseTooLarge,
                "HLS AAC packet exceeds its byte limit",
            ));
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|_| error(ErrorCode::DecodeFailed, "HLS AAC packet decode failed"))?;
        let spec = decoded.spec();
        if spec.rate() != sample_rate || spec.channels().count() != 2 {
            return Err(error(
                ErrorCode::HlsCodecUnsupported,
                "HLS decoded audio changed format",
            ));
        }
        let mut interleaved = vec![0.0_f32; decoded.frames() * 2];
        decoded.copy_to_slice_interleaved(&mut interleaved);
        let trim_start = usize::try_from(packet.trim_start.get()).unwrap();
        let presentation_frames = usize::try_from(packet.dur.get()).unwrap();
        let packet_end = trim_start
            .checked_add(presentation_frames)
            .filter(|end| *end <= decoded.frames())
            .ok_or_else(|| {
                error(
                    ErrorCode::AudioDataInvalid,
                    "HLS AAC packet trim exceeds decoded audio",
                )
            })?;
        for frame in interleaved[trim_start * 2..packet_end * 2].chunks_exact(2) {
            if !frame[0].is_finite() || !frame[1].is_finite() {
                return Err(error(
                    ErrorCode::AudioDataInvalid,
                    "HLS segment contains non-finite audio",
                ));
            }
            left.push(frame[0]);
            right.push(frame[1]);
        }
        packets = packets.saturating_add(1);
        if left.len() > maximum {
            return Err(error(
                ErrorCode::HlsResponseTooLarge,
                "HLS decoded segment exceeds its frame limit",
            ));
        }
    }
    if left.len() < trim_start_frames || right.len() < trim_start_frames {
        return Err(error(
            ErrorCode::HlsSegmentBoundaryMismatch,
            "HLS segment does not contain declared preroll",
        ));
    }
    left.drain(..trim_start_frames);
    right.drain(..trim_start_frames);
    let terminal_padding = left.len().saturating_sub(expected_frames);
    if terminal_padding <= AAC_PACKET_FRAMES {
        left.truncate(expected_frames);
        right.truncate(expected_frames);
    }
    if left.len() != expected_frames || right.len() != expected_frames {
        return Err(error(
            ErrorCode::HlsSegmentBoundaryMismatch,
            &format!(
                "HLS segment decoded {} frames but its timeline requires {expected_frames}",
                left.len()
            ),
        ));
    }
    Ok(DecodedSegment {
        left,
        right,
        packets,
    })
}

fn error(code: ErrorCode, summary: &str) -> PlayerError {
    PlayerError::new(code, summary).with_user_action("Open a qualified stem-hls-v1 source")
}
