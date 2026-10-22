#![deny(unsafe_code)]

mod compressed;
mod media;
mod presentation;
mod segment;

pub use compressed::{
    CompressedCodec, CompressedContainer, CompressedContainerTrack, CompressedDecodeDiagnostics,
    CompressedTrackDecoder, CompressedTrackInfo, CompressedWindow, inspect_audio_file,
    inspect_audio_file_cancelled, inspect_compressed_container, inspect_compressed_tracks,
};
pub use segment::{DecodedSegment, decode_aac_segment};

use serde::Serialize;
use std::sync::atomic::AtomicBool;
use stemplayer_domain::{ErrorCode, PlayerError};
use stemplayer_format::{BoundedReader, SampleEncoding, StudioBundle, StudioRole, WaveInfo};

pub const MAX_DECODE_FRAMES: u32 = 4096;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DecodedWindow {
    pub role: StudioRole,
    pub start_frame: u64,
    pub frames: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub bytes_read: u64,
}

pub struct StudioTrackDecoder {
    role: StudioRole,
    info: WaveInfo,
    reader: BoundedReader,
    bytes: Vec<u8>,
    left: Vec<f32>,
    right: Vec<f32>,
}

pub struct StudioWindow<'a> {
    pub role: StudioRole,
    pub start_frame: u64,
    pub frames: u32,
    pub left: &'a [f32],
    pub right: &'a [f32],
    pub bytes_read: u64,
}

impl StudioTrackDecoder {
    pub fn open(bundle: &StudioBundle, role: StudioRole) -> Result<Self, PlayerError> {
        let info = bundle
            .track(role)
            .ok_or_else(|| {
                PlayerError::new(
                    ErrorCode::ManifestRoleMissing,
                    "Requested Studio role is unavailable",
                )
            })?
            .info()
            .clone();
        Ok(Self {
            role,
            info,
            reader: bundle.open_track(role)?,
            bytes: Vec::new(),
            left: Vec::new(),
            right: Vec::new(),
        })
    }

    pub fn decode_window(
        &mut self,
        start_frame: u64,
        requested_frames: u32,
        cancelled: &AtomicBool,
    ) -> Result<StudioWindow<'_>, PlayerError> {
        let (frames, byte_offset, byte_count) =
            decode_range(&self.info, start_frame, requested_frames)?;
        self.bytes.resize(byte_count, 0);
        let before = self.reader.bytes_read();
        self.reader
            .read_exact_at(byte_offset, &mut self.bytes, cancelled)?;
        decode_bytes_into(
            self.info.encoding,
            &self.bytes,
            &mut self.left,
            &mut self.right,
        )?;
        Ok(StudioWindow {
            role: self.role,
            start_frame,
            frames,
            left: &self.left,
            right: &self.right,
            bytes_read: self.reader.bytes_read().saturating_sub(before),
        })
    }

    pub fn samples(&self) -> (&[f32], &[f32]) {
        (&self.left, &self.right)
    }
}

pub fn decode_window(
    bundle: &StudioBundle,
    role: StudioRole,
    start_frame: u64,
    requested_frames: u32,
    cancelled: &AtomicBool,
) -> Result<DecodedWindow, PlayerError> {
    let mut decoder = StudioTrackDecoder::open(bundle, role)?;
    let (role, start_frame, frames, bytes_read) = {
        let window = decoder.decode_window(start_frame, requested_frames, cancelled)?;
        (
            window.role,
            window.start_frame,
            window.frames,
            window.bytes_read,
        )
    };
    Ok(DecodedWindow {
        role,
        start_frame,
        frames,
        left: decoder.left,
        right: decoder.right,
        bytes_read,
    })
}

fn decode_range(
    info: &WaveInfo,
    start_frame: u64,
    requested_frames: u32,
) -> Result<(u32, u64, usize), PlayerError> {
    if requested_frames > MAX_DECODE_FRAMES {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Decode window exceeds 4096 frames",
        ));
    }
    if start_frame > info.frames {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Decode start frame is outside the source",
        ));
    }
    let frames = u64::from(requested_frames).min(info.frames.saturating_sub(start_frame)) as u32;
    let bytes_per_frame = u64::from(info.encoding.bytes_per_frame());
    let byte_offset = start_frame
        .checked_mul(bytes_per_frame)
        .and_then(|value| info.data_offset.checked_add(value))
        .ok_or_else(|| PlayerError::new(ErrorCode::WavMalformed, "Decode offset overflowed"))?;
    let byte_count = usize::try_from(u64::from(frames) * bytes_per_frame).unwrap();
    Ok((frames, byte_offset, byte_count))
}

#[cfg(test)]
fn decode_bytes(
    encoding: SampleEncoding,
    bytes: &[u8],
) -> Result<(Vec<f32>, Vec<f32>), PlayerError> {
    let sample_bytes = usize::from(encoding.bits_per_sample() / 8);
    let frames = bytes.len() / (sample_bytes * 2);
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);
    decode_bytes_into(encoding, bytes, &mut left, &mut right)?;
    Ok((left, right))
}

fn decode_bytes_into(
    encoding: SampleEncoding,
    bytes: &[u8],
    left: &mut Vec<f32>,
    right: &mut Vec<f32>,
) -> Result<(), PlayerError> {
    let sample_bytes = usize::from(encoding.bits_per_sample() / 8);
    let frames = bytes.len() / (sample_bytes * 2);
    left.clear();
    right.clear();
    left.reserve(frames);
    right.reserve(frames);
    for frame in bytes.chunks_exact(sample_bytes * 2) {
        let first = decode_sample(encoding, &frame[..sample_bytes])?;
        let second = decode_sample(encoding, &frame[sample_bytes..])?;
        left.push(first);
        right.push(second);
    }
    Ok(())
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
