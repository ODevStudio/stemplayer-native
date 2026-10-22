use stemplayer_domain::{ErrorCode, PlayerError};

const MAX_CODEC_CONFIG_BYTES: usize = 64 * 1024;
type BoxSlices<'a> = ([u8; 4], &'a [u8], &'a [u8]);

pub(crate) fn validate_sample_description(bytes: &[u8]) -> Result<(), PlayerError> {
    let header = bytes.get(..8).ok_or_else(invalid)?;
    if header[..4] != [0; 4] || header[4..] != 1_u32.to_be_bytes() {
        return Err(invalid());
    }
    let (kind, entry, remaining) = split_box(&bytes[8..])?;
    if !remaining.is_empty() {
        return Err(invalid());
    }
    let audio = matches!(
        &kind,
        b"mp4a"
            | b"alac"
            | b"ac-3"
            | b"ec-3"
            | b"fLaC"
            | b"Opus"
            | b".mp3"
            | b"lpcm"
            | b"wave"
            | b"alaw"
            | b"ulaw"
            | b"raw "
            | b"sowt"
            | b"twos"
            | b"in24"
            | b"in32"
            | b"fl32"
            | b"fl64"
    );
    let children = if audio {
        let version = entry.get(8..10).ok_or_else(invalid)?;
        let offset = match u16::from_be_bytes(version.try_into().unwrap()) {
            0 => 28,
            1 => 44,
            2 => 64,
            _ => return Err(invalid()),
        };
        entry.get(offset..).ok_or_else(invalid)?
    } else if matches!(
        &kind,
        b"av01" | b"avc1" | b"dvh1" | b"dvhe" | b"hev1" | b"hvc1" | b"mp4v" | b"vp08" | b"vp09"
    ) {
        entry.get(78..).ok_or_else(invalid)?
    } else {
        return Ok(());
    };
    validate_codec_boxes(children, audio)
}

fn validate_codec_boxes(mut bytes: &[u8], audio: bool) -> Result<(), PlayerError> {
    while !bytes.is_empty() {
        let (kind, body, remaining) = split_box(bytes)?;
        match &kind {
            b"esds" => validate_esds(body)?,
            b"wave" if audio => validate_codec_boxes(body, false)?,
            _ => {}
        }
        bytes = remaining;
    }
    Ok(())
}

fn validate_esds(bytes: &[u8]) -> Result<(), PlayerError> {
    let descriptors = bytes.get(4..).ok_or_else(invalid)?;
    let (tag, body, _) = split_descriptor(descriptors)?;
    if tag != 3 {
        return Err(invalid());
    }
    let flags = *body.get(2).ok_or_else(invalid)?;
    let mut offset = 3;
    if flags & 0x80 != 0 {
        offset += 2;
    }
    if flags & 0x40 != 0 {
        offset += 1 + usize::from(*body.get(offset).ok_or_else(invalid)?);
    }
    if flags & 0x20 != 0 {
        offset += 2;
    }
    let mut children = body.get(offset..).ok_or_else(invalid)?;
    while !children.is_empty() {
        let (tag, body, remaining) = split_descriptor(children)?;
        if tag == 4 {
            validate_decoder_config(body)?;
        }
        children = remaining;
    }
    Ok(())
}

fn validate_decoder_config(bytes: &[u8]) -> Result<(), PlayerError> {
    let mut children = bytes.get(13..).ok_or_else(invalid)?;
    while !children.is_empty() {
        let (tag, body, remaining) = split_descriptor(children)?;
        if tag == 5 && body.len() > MAX_CODEC_CONFIG_BYTES {
            return Err(invalid());
        }
        children = remaining;
    }
    Ok(())
}

fn split_descriptor(bytes: &[u8]) -> Result<(u8, &[u8], &[u8]), PlayerError> {
    let tag = *bytes.first().ok_or_else(invalid)?;
    if matches!(tag, 0 | 255) {
        return Err(invalid());
    }
    let mut length = 0_usize;
    for index in 1..=4 {
        let byte = *bytes.get(index).ok_or_else(invalid)?;
        length = (length << 7) | usize::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            let payload = &bytes[index + 1..];
            let body = payload.get(..length).ok_or_else(invalid)?;
            return Ok((tag, body, &payload[length..]));
        }
    }
    Err(invalid())
}

fn split_box(bytes: &[u8]) -> Result<BoxSlices<'_>, PlayerError> {
    let header = bytes.get(..8).ok_or_else(invalid)?;
    let kind = header[4..8].try_into().unwrap();
    let (length, header_bytes) = match u32::from_be_bytes(header[..4].try_into().unwrap()) {
        0 => return Err(invalid()),
        1 => {
            let extended = bytes.get(8..16).ok_or_else(invalid)?;
            (
                usize::try_from(u64::from_be_bytes(extended.try_into().unwrap()))
                    .map_err(|_| invalid())?,
                16,
            )
        }
        length => (length as usize, 8),
    };
    let body = bytes.get(header_bytes..length).ok_or_else(invalid)?;
    Ok((kind, body, &bytes[length..]))
}

fn invalid() -> PlayerError {
    PlayerError::new(
        ErrorCode::CodecConfigInvalid,
        "MP4 codec descriptor range or configuration size is invalid",
    )
}
