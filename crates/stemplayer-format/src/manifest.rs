use crate::StudioRole;
use crate::source::{safe_basename, source_error};
use serde::Deserialize;
use serde_json::Value;
use stemplayer_domain::{ErrorCode, PlayerError};

pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_VALUES: usize = 10_000;
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

#[derive(Clone, Debug)]
pub struct ManifestTrack {
    pub role: StudioRole,
    pub filename: String,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub frames: Option<u64>,
    pub subtype: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StudioManifest {
    pub schema_version: u8,
    pub tracks: Vec<ManifestTrack>,
    pub ignored_outputs: Vec<String>,
    pub unknown_fields: Vec<String>,
}

#[derive(Deserialize)]
struct RawManifest {
    schema_version: u64,
    outputs: Vec<RawOutput>,
}

#[derive(Deserialize)]
struct RawOutput {
    logical_type: String,
    filename: Option<String>,
    sample_rate: Option<f64>,
    channels: Option<f64>,
    frames: Option<f64>,
    subtype: Option<String>,
    sha256: Option<String>,
}

pub fn parse_manifest(bytes: &[u8]) -> Result<StudioManifest, PlayerError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(source_error(
            ErrorCode::ManifestTooLarge,
            "Manifest exceeds the 1 MiB limit",
        ));
    }
    std::str::from_utf8(bytes)
        .map_err(|_| source_error(ErrorCode::ManifestInvalid, "Manifest is not valid UTF-8"))?;
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| source_error(ErrorCode::ManifestInvalid, "Manifest JSON is invalid"))?;
    let mut values = 0;
    validate_json(&value, 1, &mut values)?;
    let unknown_fields = value
        .as_object()
        .map(|object| {
            object
                .keys()
                .filter(|key| !matches!(key.as_str(), "schema_version" | "outputs"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let raw: RawManifest = serde_json::from_slice(bytes).map_err(|_| {
        source_error(
            ErrorCode::ManifestInvalid,
            "Manifest fields have invalid types or duplicates",
        )
    })?;
    if !matches!(raw.schema_version, 1 | 2) {
        return Err(source_error(
            ErrorCode::ManifestVersionUnsupported,
            "Manifest schema version is unsupported",
        ));
    }

    let mut tracks = Vec::with_capacity(5);
    let mut ignored_outputs = Vec::new();
    for output in raw.outputs {
        if matches!(output.logical_type.as_str(), "instrumental" | "acapella") {
            ignored_outputs.push(output.logical_type);
            continue;
        }
        let role = StudioRole::parse(&output.logical_type).ok_or_else(|| {
            source_error(
                ErrorCode::ManifestInvalid,
                "Manifest output has an unsupported logical_type",
            )
        })?;
        if tracks
            .iter()
            .any(|track: &ManifestTrack| track.role == role)
        {
            return Err(source_error(
                ErrorCode::ManifestRoleDuplicate,
                "Manifest declares a routed role more than once",
            ));
        }
        let filename = output.filename.ok_or_else(|| {
            source_error(
                ErrorCode::ManifestInvalid,
                "Routed manifest output has no filename",
            )
        })?;
        safe_basename(&filename)?;
        let sha256 = output.sha256.map(validate_hash).transpose()?;
        tracks.push(ManifestTrack {
            role,
            filename,
            sample_rate: output
                .sample_rate
                .map(|value| exact_u64(value, u32::MAX as u64, false, "sample_rate"))
                .transpose()?
                .map(|value| value as u32),
            channels: output
                .channels
                .map(|value| exact_u64(value, u8::MAX as u64, false, "channels"))
                .transpose()?
                .map(|value| value as u8),
            frames: output
                .frames
                .map(|value| exact_u64(value, MAX_SAFE_INTEGER as u64, true, "frames"))
                .transpose()?,
            subtype: output.subtype,
            sha256,
        });
    }

    for role in StudioRole::STEMS {
        if !tracks.iter().any(|track| track.role == role) {
            return Err(source_error(
                ErrorCode::ManifestRoleMissing,
                "Manifest is missing a required stem role",
            ));
        }
    }
    Ok(StudioManifest {
        schema_version: raw.schema_version as u8,
        tracks,
        ignored_outputs,
        unknown_fields,
    })
}

fn validate_json(value: &Value, depth: usize, values: &mut usize) -> Result<(), PlayerError> {
    *values = values.saturating_add(1);
    if depth > MAX_JSON_DEPTH || *values > MAX_JSON_VALUES {
        return Err(source_error(
            ErrorCode::ManifestInvalid,
            "Manifest exceeds JSON depth or value limits",
        ));
    }
    match value {
        Value::Array(items) => {
            for item in items {
                validate_json(item, depth + 1, values)?;
            }
        }
        Value::Object(object) => {
            for (key, child) in object {
                if matches!(key.as_str(), "__proto__" | "prototype" | "constructor") {
                    return Err(source_error(
                        ErrorCode::ManifestInvalid,
                        "Manifest contains a forbidden key",
                    ));
                }
                validate_json(child, depth + 1, values)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_hash(hash: String) -> Result<String, PlayerError> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(source_error(
            ErrorCode::ManifestInvalid,
            "SHA-256 declaration must contain exactly 64 hexadecimal characters",
        ));
    }
    Ok(hash.to_ascii_lowercase())
}

fn exact_u64(value: f64, max: u64, positive: bool, field: &str) -> Result<u64, PlayerError> {
    if !value.is_finite()
        || value.fract() != 0.0
        || value < if positive { 1.0 } else { 0.0 }
        || value > max as f64
    {
        return Err(source_error(
            ErrorCode::ManifestInvalid,
            &format!("Manifest {field} declaration is invalid"),
        ));
    }
    Ok(value as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_limits_and_roles_are_enforced() {
        let valid = br#"{"schema_version":1,"outputs":[
            {"logical_type":"drums","filename":"drums.wav"},
            {"logical_type":"bass","filename":"bass.wav"},
            {"logical_type":"other","filename":"other.wav"},
            {"logical_type":"vocals","filename":"vocals.wav"}]}"#;
        assert_eq!(parse_manifest(valid).unwrap().tracks.len(), 4);
        assert_eq!(
            parse_manifest(br#"{"schema_version":1,"outputs":[]}"#)
                .unwrap_err()
                .code,
            ErrorCode::ManifestRoleMissing
        );
        assert_eq!(
            parse_manifest(br#"{"schema_version":3,"outputs":[]}"#)
                .unwrap_err()
                .code,
            ErrorCode::ManifestVersionUnsupported
        );
    }

    #[test]
    fn manifest_byte_depth_and_value_limits_fail_closed() {
        let oversized = vec![b' '; MAX_MANIFEST_BYTES + 1];
        assert_eq!(
            parse_manifest(&oversized).unwrap_err().code,
            ErrorCode::ManifestTooLarge
        );

        let nested = format!(
            r#"{{"schema_version":1,"outputs":[],"nested":{}}}"#,
            "[".repeat(MAX_JSON_DEPTH) + &"]".repeat(MAX_JSON_DEPTH)
        );
        assert_eq!(
            parse_manifest(nested.as_bytes()).unwrap_err().code,
            ErrorCode::ManifestInvalid
        );

        let values = format!(
            r#"{{"schema_version":1,"outputs":[],"values":[{}]}}"#,
            vec!["0"; MAX_JSON_VALUES].join(",")
        );
        assert_eq!(
            parse_manifest(values.as_bytes()).unwrap_err().code,
            ErrorCode::ManifestInvalid
        );
    }
    #[test]
    fn forbidden_and_duplicate_semantic_fields_fail() {
        let forbidden = br#"{"schema_version":1,"outputs":[],"constructor":{}}"#;
        assert_eq!(
            parse_manifest(forbidden).unwrap_err().code,
            ErrorCode::ManifestInvalid
        );
        let duplicate = br#"{"schema_version":1,"schema_version":2,"outputs":[]}"#;
        assert_eq!(
            parse_manifest(duplicate).unwrap_err().code,
            ErrorCode::ManifestInvalid
        );
    }
}
