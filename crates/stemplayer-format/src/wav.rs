use crate::source::{BoundedReader, FileIdentity, source_error};
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use stemplayer_domain::{ErrorCode, PlayerError};

const MAX_CHUNKS: usize = 4096;
const MAX_FMT_BYTES: u32 = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleEncoding {
    Pcm16,
    Pcm24,
    Pcm32,
    Float32,
}

impl SampleEncoding {
    pub const fn bits_per_sample(self) -> u16 {
        match self {
            Self::Pcm16 => 16,
            Self::Pcm24 => 24,
            Self::Pcm32 | Self::Float32 => 32,
        }
    }

    pub const fn bytes_per_frame(self) -> u16 {
        self.bits_per_sample() / 8 * 2
    }
}

#[derive(Clone, Debug)]
pub struct WaveInfo {
    pub sample_rate: u32,
    pub frames: u64,
    pub data_offset: u64,
    pub data_bytes: u64,
    pub encoding: SampleEncoding,
    pub identity: FileIdentity,
    pub inspection_bytes_read: u64,
}

pub fn inspect_wave(
    mut reader: BoundedReader,
    cancelled: &AtomicBool,
) -> Result<WaveInfo, PlayerError> {
    let mut header = [0_u8; 12];
    reader.read_exact_at(0, &mut header, cancelled)?;
    if &header[0..4] == b"RF64" || &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(source_error(
            ErrorCode::WavNotRiff,
            "Audio file is not a qualified RIFF/WAVE file",
        ));
    }
    let riff_end = u64::from(le_u32(&header[4..8]))
        .checked_add(8)
        .ok_or_else(|| source_error(ErrorCode::WavMalformed, "RIFF size overflowed"))?;
    if riff_end > reader.identity().len() || riff_end < 12 {
        return Err(source_error(
            ErrorCode::WavDataTruncated,
            "RIFF declaration exceeds the file",
        ));
    }

    let mut offset = 12_u64;
    let mut chunks = 0_usize;
    let mut format = None;
    let mut data = None;
    while offset < riff_end {
        chunks += 1;
        if chunks > MAX_CHUNKS {
            return Err(source_error(
                ErrorCode::WavMalformed,
                "WAV chunk count exceeds the limit",
            ));
        }
        let mut chunk_header = [0_u8; 8];
        reader.read_exact_at(offset, &mut chunk_header, cancelled)?;
        let length = u64::from(le_u32(&chunk_header[4..8]));
        let body = offset
            .checked_add(8)
            .ok_or_else(|| source_error(ErrorCode::WavMalformed, "WAV chunk offset overflowed"))?;
        let body_end = body
            .checked_add(length)
            .ok_or_else(|| source_error(ErrorCode::WavMalformed, "WAV chunk size overflowed"))?;
        let padded_end = body_end
            .checked_add(length & 1)
            .ok_or_else(|| source_error(ErrorCode::WavMalformed, "WAV padding overflowed"))?;
        if padded_end > riff_end {
            return Err(source_error(
                ErrorCode::WavDataTruncated,
                "WAV chunk is truncated",
            ));
        }
        match &chunk_header[0..4] {
            b"fmt " => {
                if format.is_some() || !(16..=u64::from(MAX_FMT_BYTES)).contains(&length) {
                    return Err(source_error(
                        ErrorCode::WavMalformed,
                        "WAV must contain one bounded fmt chunk",
                    ));
                }
                let mut bytes = vec![0_u8; length as usize];
                reader.read_exact_at(body, &mut bytes, cancelled)?;
                format = Some(parse_format(&bytes)?);
            }
            b"data" if data.is_some() => {
                return Err(source_error(
                    ErrorCode::WavMalformed,
                    "WAV contains multiple data chunks",
                ));
            }
            b"data" => data = Some((body, length)),
            _ => {}
        }
        offset = padded_end;
    }
    if offset != riff_end {
        return Err(source_error(
            ErrorCode::WavMalformed,
            "WAV chunks do not end at the RIFF boundary",
        ));
    }
    let (sample_rate, encoding) = format
        .ok_or_else(|| source_error(ErrorCode::WavMalformed, "WAV has no usable fmt chunk"))?;
    let (data_offset, data_bytes) =
        data.ok_or_else(|| source_error(ErrorCode::WavMalformed, "WAV has no data chunk"))?;
    let bytes_per_frame = u64::from(encoding.bytes_per_frame());
    if data_bytes % bytes_per_frame != 0 {
        return Err(source_error(
            ErrorCode::WavBlockAlignInvalid,
            "WAV data length is not frame aligned",
        ));
    }
    let frames = data_bytes / bytes_per_frame;
    Ok(WaveInfo {
        sample_rate,
        frames,
        data_offset,
        data_bytes,
        encoding,
        identity: reader.identity().clone(),
        inspection_bytes_read: reader.bytes_read(),
    })
}

fn parse_format(bytes: &[u8]) -> Result<(u32, SampleEncoding), PlayerError> {
    let format = le_u16(&bytes[0..2]);
    let channels = le_u16(&bytes[2..4]);
    let sample_rate = le_u32(&bytes[4..8]);
    let byte_rate = le_u32(&bytes[8..12]);
    let block_align = le_u16(&bytes[12..14]);
    let bits = le_u16(&bytes[14..16]);
    if channels != 2 {
        return Err(source_error(
            ErrorCode::WavChannelsUnsupported,
            "WAV must contain exactly two channels",
        ));
    }
    if !matches!(sample_rate, 44_100 | 48_000) {
        return Err(source_error(
            ErrorCode::WavFormatUnsupported,
            "WAV sample rate must be 44.1 or 48 kHz",
        ));
    }
    let encoding = match (format, bits) {
        (1, 16) => SampleEncoding::Pcm16,
        (1, 24) => SampleEncoding::Pcm24,
        (1, 32) => SampleEncoding::Pcm32,
        (3, 32) => SampleEncoding::Float32,
        _ => {
            return Err(source_error(
                ErrorCode::WavFormatUnsupported,
                "WAV encoding is outside the qualified profile",
            ));
        }
    };
    let expected_align = encoding.bytes_per_frame();
    let expected_rate = sample_rate
        .checked_mul(u32::from(expected_align))
        .ok_or_else(|| source_error(ErrorCode::WavMalformed, "WAV byte rate overflowed"))?;
    if block_align != expected_align || byte_rate != expected_rate {
        return Err(source_error(
            ErrorCode::WavBlockAlignInvalid,
            "WAV byte rate or block alignment is invalid",
        ));
    }
    Ok((sample_rate, encoding))
}

fn le_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes(bytes.try_into().unwrap())
}

fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings_have_qualified_frame_sizes() {
        assert_eq!(SampleEncoding::Pcm16.bytes_per_frame(), 4);
        assert_eq!(SampleEncoding::Pcm24.bytes_per_frame(), 6);
        assert_eq!(SampleEncoding::Pcm32.bytes_per_frame(), 8);
        assert_eq!(SampleEncoding::Float32.bytes_per_frame(), 8);
    }
}
