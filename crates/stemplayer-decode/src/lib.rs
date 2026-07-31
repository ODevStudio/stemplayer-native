#![deny(unsafe_code)]

mod compressed;
mod segment;

pub use compressed::{
    CompressedCodec, CompressedContainer, CompressedContainerTrack, CompressedDecodeDiagnostics,
    CompressedTrackDecoder, CompressedTrackInfo, CompressedWindow, inspect_audio_file,
    inspect_compressed_container, inspect_compressed_tracks,
};
pub use segment::{DecodedSegment, decode_aac_segment};

use serde::Serialize;
use std::sync::atomic::AtomicBool;
use stemplayer_domain::{ErrorCode, GenerationId, PlayerError};
use stemplayer_format::{SampleEncoding, StudioBundle, StudioRole};

pub const MAX_DECODE_FRAMES: u32 = 4096;

pub trait Decoder: Send {
    fn cancel(&mut self, generation: GenerationId) -> Result<(), PlayerError>;
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DecodedWindow {
    pub role: StudioRole,
    pub start_frame: u64,
    pub frames: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub bytes_read: u64,
}

pub fn decode_window(
    bundle: &StudioBundle,
    role: StudioRole,
    start_frame: u64,
    requested_frames: u32,
    cancelled: &AtomicBool,
) -> Result<DecodedWindow, PlayerError> {
    if requested_frames > MAX_DECODE_FRAMES {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Decode window exceeds 4096 frames",
        ));
    }
    let track = bundle.track(role).ok_or_else(|| {
        PlayerError::new(
            ErrorCode::ManifestRoleMissing,
            "Requested Studio role is unavailable",
        )
    })?;
    if start_frame > track.info().frames {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Decode start frame is outside the source",
        ));
    }
    let frames =
        u64::from(requested_frames).min(track.info().frames.saturating_sub(start_frame)) as u32;
    let bytes_per_frame = u64::from(track.info().encoding.bytes_per_frame());
    let byte_offset = start_frame
        .checked_mul(bytes_per_frame)
        .and_then(|value| track.info().data_offset.checked_add(value))
        .ok_or_else(|| PlayerError::new(ErrorCode::WavMalformed, "Decode offset overflowed"))?;
    let byte_count = usize::try_from(u64::from(frames) * bytes_per_frame).unwrap();
    let mut bytes = vec![0_u8; byte_count];
    let mut reader = bundle.open_track(role)?;
    reader.read_exact_at(byte_offset, &mut bytes, cancelled)?;
    let (left, right) = decode_bytes(track.info().encoding, &bytes)?;
    Ok(DecodedWindow {
        role,
        start_frame,
        frames,
        left,
        right,
        bytes_read: reader.bytes_read(),
    })
}

fn decode_bytes(
    encoding: SampleEncoding,
    bytes: &[u8],
) -> Result<(Vec<f32>, Vec<f32>), PlayerError> {
    let sample_bytes = usize::from(encoding.bits_per_sample() / 8);
    let frames = bytes.len() / (sample_bytes * 2);
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);
    for frame in bytes.chunks_exact(sample_bytes * 2) {
        let first = decode_sample(encoding, &frame[..sample_bytes])?;
        let second = decode_sample(encoding, &frame[sample_bytes..])?;
        left.push(first);
        right.push(second);
    }
    Ok((left, right))
}

fn decode_sample(encoding: SampleEncoding, bytes: &[u8]) -> Result<f32, PlayerError> {
    let value = match encoding {
        SampleEncoding::Pcm16 => {
            f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32_768.0
        }
        SampleEncoding::Pcm24 => {
            let raw =
                i32::from(bytes[0]) | (i32::from(bytes[1]) << 8) | (i32::from(bytes[2]) << 16);
            ((raw << 8) >> 8) as f32 / 8_388_608.0
        }
        SampleEncoding::Pcm32 => {
            i32::from_le_bytes(bytes.try_into().unwrap()) as f32 / 2_147_483_648.0
        }
        SampleEncoding::Float32 => {
            let value = f32::from_le_bytes(bytes.try_into().unwrap());
            if !value.is_finite() {
                return Err(PlayerError::new(
                    ErrorCode::WavPcmInvalid,
                    "Float32 WAV contains NaN or Infinity",
                ));
            }
            value
        }
    };
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(encoding: SampleEncoding, bytes: &[u8]) -> Vec<f32> {
        decode_bytes(encoding, bytes).unwrap().0
    }

    #[test]
    fn integer_boundaries_convert_exactly() {
        let pcm16 = [
            i16::MIN.to_le_bytes(),
            [0, 0],
            i16::MAX.to_le_bytes(),
            [0, 0],
        ]
        .concat();
        assert_eq!(
            samples(SampleEncoding::Pcm16, &pcm16),
            [-1.0, 32767.0 / 32768.0]
        );

        let pcm24 = [0, 0, 0x80, 0, 0, 0, 0xff, 0xff, 0x7f, 0, 0, 0];
        assert_eq!(
            samples(SampleEncoding::Pcm24, &pcm24),
            [-1.0, 8388607.0 / 8388608.0]
        );

        let pcm32 = [
            i32::MIN.to_le_bytes(),
            [0, 0, 0, 0],
            i32::MAX.to_le_bytes(),
            [0, 0, 0, 0],
        ]
        .concat();
        assert_eq!(
            samples(SampleEncoding::Pcm32, &pcm32),
            [-1.0, 2147483647.0 / 2147483648.0]
        );
    }

    #[test]
    fn float_values_are_preserved_and_non_finite_rejected() {
        let bytes = [2.5_f32.to_le_bytes(), (-3.0_f32).to_le_bytes()].concat();
        let (left, right) = decode_bytes(SampleEncoding::Float32, &bytes).unwrap();
        assert_eq!(left, [2.5]);
        assert_eq!(right, [-3.0]);
        let invalid = [f32::NAN.to_le_bytes(), 0_f32.to_le_bytes()].concat();
        assert_eq!(
            decode_bytes(SampleEncoding::Float32, &invalid)
                .unwrap_err()
                .code,
            ErrorCode::WavPcmInvalid
        );
    }
}
