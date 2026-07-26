use crate::BoundedReader;
use serde_json::Value;
use std::sync::atomic::AtomicBool;
use stemplayer_domain::{ErrorCode, PlayerError};

pub(crate) const MAX_METADATA_BYTES: usize = 1024 * 1024;
const MAX_METADATA_DEPTH: usize = 16;
const MAX_METADATA_NODES: usize = 1024;

pub(crate) fn read_metadata(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    body: u64,
    end: u64,
) -> Result<Vec<u8>, PlayerError> {
    let length = usize::try_from(end - body)
        .map_err(|_| malformed("MP4 metadata size exceeds the platform limit"))?;
    if length == 0 || length > MAX_METADATA_BYTES {
        return Err(malformed("MP4 metadata exceeds the supported size"));
    }
    let mut bytes = vec![0_u8; length];
    let mut offset = body;
    for chunk in bytes.chunks_mut(64 * 1024) {
        reader.read_exact_at(offset, chunk, cancelled)?;
        offset += chunk.len() as u64;
    }
    Ok(bytes)
}

pub(crate) fn read_track_name(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    body: u64,
    end: u64,
) -> Result<String, PlayerError> {
    let bytes = read_metadata(reader, cancelled, body, end)?;
    let name = std::str::from_utf8(&bytes)
        .map_err(|_| malformed("MP4 track name is not valid UTF-8"))?
        .trim_matches(char::from(0))
        .trim();
    if name.is_empty() || name.len() > 256 {
        return Err(malformed("MP4 track name exceeds the supported size"));
    }
    Ok(name.to_owned())
}

pub(crate) fn read_handler(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    body: u64,
    end: u64,
) -> Result<([u8; 4], Option<String>), PlayerError> {
    if body.checked_add(24).is_none_or(|value| value > end) {
        return Err(malformed("MP4 handler is truncated"));
    }
    let mut header = [0_u8; 24];
    reader.read_exact_at(body, &mut header, cancelled)?;
    let length = usize::try_from(end - body - 24)
        .map_err(|_| malformed("MP4 handler name exceeds the platform limit"))?;
    if length > 257 {
        return Err(malformed("MP4 handler name exceeds the supported size"));
    }
    let name = if length == 0 {
        None
    } else {
        let mut bytes = vec![0_u8; length];
        reader.read_exact_at(body + 24, &mut bytes, cancelled)?;
        let name = std::str::from_utf8(&bytes)
            .map_err(|_| malformed("MP4 handler name is not valid UTF-8"))?
            .trim_matches(char::from(0))
            .trim();
        (!name.is_empty()).then(|| name.to_owned())
    };
    Ok((header[8..12].try_into().unwrap(), name))
}

pub(crate) fn parse_native_roles(bytes: &[u8]) -> Result<Vec<String>, PlayerError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| malformed("Native Instruments metadata is malformed"))?;
    validate_metadata_tree(&value)?;
    let object = value
        .as_object()
        .ok_or_else(|| malformed("Native Instruments metadata must be an object"))?;
    let entries = object
        .get("stems")
        .or_else(|| object.get("tracks"))
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("Native Instruments metadata has no stem entries"))?;
    if !(4..=5).contains(&entries.len()) {
        return Err(malformed(
            "Native Instruments metadata stem count is unsupported",
        ));
    }
    entries
        .iter()
        .map(|entry| {
            let entry = entry
                .as_object()
                .ok_or_else(|| malformed("Native Instruments stem entry is malformed"))?;
            let role = entry
                .get("role")
                .and_then(Value::as_str)
                .or_else(|| entry.get("name").and_then(Value::as_str))
                .ok_or_else(|| malformed("Native Instruments stem role is missing"))?;
            if role.is_empty() || role.len() > 256 {
                return Err(malformed(
                    "Native Instruments stem role exceeds the supported size",
                ));
            }
            Ok(role.to_owned())
        })
        .collect()
}

fn validate_metadata_tree(root: &Value) -> Result<(), PlayerError> {
    let mut pending = vec![(root, 0_usize)];
    let mut nodes = 0_usize;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if nodes > MAX_METADATA_NODES || depth > MAX_METADATA_DEPTH {
            return Err(malformed(
                "Native Instruments metadata exceeds structural limits",
            ));
        }
        match value {
            Value::Object(object) => {
                if object
                    .keys()
                    .any(|key| matches!(key.as_str(), "__proto__" | "prototype" | "constructor"))
                {
                    return Err(malformed(
                        "Native Instruments metadata contains a forbidden key",
                    ));
                }
                pending.extend(object.values().map(|value| (value, depth + 1)));
            }
            Value::Array(values) => {
                pending.extend(values.iter().map(|value| (value, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(())
}

fn malformed(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::ContainerMalformed, summary)
        .with_user_action("Choose a valid .stem.m4a or .stem.mp4 file")
}
