use super::*;

pub(super) fn validate_parameters(
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
        timeline: None,
        identity: None,
        media_frames: declared_frames,
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
