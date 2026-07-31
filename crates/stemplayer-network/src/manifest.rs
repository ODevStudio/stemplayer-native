use serde::Deserialize;
use std::collections::BTreeSet;
use stemplayer_domain::{ErrorCode, PlayerError, StemRole};
use unicode_normalization::UnicodeNormalization;
use url::Url;

use crate::url_policy::{child_url, redacted};

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;
pub const MAX_WAVEFORM_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SEGMENTS: usize = 100_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HlsRole {
    Stem(StemRole),
    Master,
}

impl HlsRole {
    pub const STEMS: [Self; 4] = [
        Self::Stem(StemRole::Drums),
        Self::Stem(StemRole::Bass),
        Self::Stem(StemRole::Other),
        Self::Stem(StemRole::Vocals),
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Stem(role) => role.index(),
            Self::Master => 4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HlsRoute {
    pub role: HlsRole,
    pub playlist_url: Url,
    pub waveform_url: Url,
}

#[derive(Clone, Debug)]
pub struct HlsManifest {
    pub redacted_url: String,
    pub title: String,
    pub duration_us: u64,
    pub sample_rate: u32,
    pub bitrate_bps: u64,
    pub segment_count: usize,
    pub preroll_frames: usize,
    pub routes: Vec<HlsRoute>,
}

#[derive(Clone, Debug)]
pub struct HlsWaveform {
    pub samples_per_peak: u64,
    pub minimum: Vec<f32>,
    pub maximum: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    schema: String,
    manifest_version: u32,
    format_version: String,
    title: String,
    duration_us: u64,
    sample_rate: u32,
    channels: u8,
    codec: RawCodec,
    segment_duration_us: u64,
    segment_count: usize,
    master_present: bool,
    master: Option<RawRoute>,
    stems: RawStems,
    technical: Option<RawTechnical>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCodec {
    name: String,
    codec_string: String,
    segment_format: String,
    bitrate_bps: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoute {
    playlist: String,
    waveform: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStems {
    drums: RawRoute,
    bass: RawRoute,
    other: RawRoute,
    vocals: RawRoute,
}

#[derive(Default, Deserialize)]
struct RawTechnical {
    #[serde(default)]
    segment_preroll_frames: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWaveform {
    schema: String,
    samples_per_peak: u64,
    min: Vec<f32>,
    max: Vec<f32>,
}

impl HlsManifest {
    pub fn parse(bytes: &[u8], base: &Url) -> Result<Self, PlayerError> {
        if bytes.len() > MAX_MANIFEST_BYTES {
            return Err(invalid(
                ErrorCode::HlsResponseTooLarge,
                "HLS stemset exceeds 256 KiB",
            ));
        }
        let raw: RawManifest = serde_json::from_slice(bytes)
            .map_err(|_| invalid(ErrorCode::HlsStemsetInvalid, "HLS stemset JSON is invalid"))?;
        if raw.schema != "stem-hls-v1"
            || raw.manifest_version != 1
            || raw.format_version != "stem-hls-v1"
            || raw.codec.name != "AAC-LC"
            || raw.codec.codec_string != "mp4a.40.2"
            || raw.codec.segment_format != "M4A"
        {
            return Err(invalid(
                ErrorCode::HlsCodecUnsupported,
                "HLS stemset profile is unsupported",
            ));
        }
        if raw.title.is_empty()
            || raw.title.chars().count() > 512
            || raw.title != raw.title.nfc().collect::<String>()
            || raw.title.chars().any(char::is_control)
            || raw.duration_us == 0
            || raw.duration_us > MAX_SAFE_INTEGER
            || !matches!(raw.sample_rate, 44_100 | 48_000)
            || raw.channels != 2
            || raw.codec.bitrate_bps == 0
            || raw.codec.bitrate_bps > MAX_SAFE_INTEGER
            || raw.segment_duration_us == 0
            || raw.segment_count == 0
            || raw.segment_count > MAX_SEGMENTS
            || raw.master_present != raw.master.is_some()
        {
            return Err(invalid(
                ErrorCode::HlsStemsetInvalid,
                "HLS stemset values are invalid",
            ));
        }
        let preroll_frames = raw.technical.unwrap_or_default().segment_preroll_frames;
        if !matches!(preroll_frames, 0 | 1024) {
            return Err(invalid(
                ErrorCode::HlsStemsetInvalid,
                "HLS segment preroll must be zero or 1024 frames",
            ));
        }
        let mut entries = vec![
            (HlsRole::Stem(StemRole::Drums), raw.stems.drums),
            (HlsRole::Stem(StemRole::Bass), raw.stems.bass),
            (HlsRole::Stem(StemRole::Other), raw.stems.other),
            (HlsRole::Stem(StemRole::Vocals), raw.stems.vocals),
        ];
        if let Some(master) = raw.master {
            entries.push((HlsRole::Master, master));
        }
        let routes = entries
            .into_iter()
            .map(|(role, route)| {
                Ok(HlsRoute {
                    role,
                    playlist_url: child_url(base, &route.playlist)?,
                    waveform_url: child_url(base, &route.waveform)?,
                })
            })
            .collect::<Result<Vec<_>, PlayerError>>()?;
        let references = routes
            .iter()
            .flat_map(|route| [route.playlist_url.as_str(), route.waveform_url.as_str()])
            .collect::<BTreeSet<_>>();
        if references.len() != routes.len() * 2 {
            return Err(invalid(
                ErrorCode::HlsUrlUnsafe,
                "HLS stemset contains duplicate normalized references",
            ));
        }
        Ok(Self {
            redacted_url: redacted(base),
            title: raw.title,
            duration_us: raw.duration_us,
            sample_rate: raw.sample_rate,
            bitrate_bps: raw.codec.bitrate_bps,
            segment_count: raw.segment_count,
            preroll_frames,
            routes,
        })
    }
}

impl HlsWaveform {
    pub fn parse(bytes: &[u8]) -> Result<Self, PlayerError> {
        if bytes.len() > MAX_WAVEFORM_BYTES {
            return Err(invalid(
                ErrorCode::HlsResponseTooLarge,
                "HLS waveform exceeds 2 MiB",
            ));
        }
        let raw: RawWaveform = serde_json::from_slice(bytes)
            .map_err(|_| invalid(ErrorCode::HlsStemsetInvalid, "HLS waveform JSON is invalid"))?;
        if raw.schema != "stem-waveform-v1"
            || raw.samples_per_peak == 0
            || raw.min.len() != raw.max.len()
            || raw.min.len() > 1_000_000
            || raw.min.iter().zip(&raw.max).any(|(minimum, maximum)| {
                !minimum.is_finite() || !maximum.is_finite() || minimum > maximum
            })
        {
            return Err(invalid(
                ErrorCode::HlsStemsetInvalid,
                "HLS waveform values are invalid",
            ));
        }
        Ok(Self {
            samples_per_peak: raw.samples_per_peak,
            minimum: raw.min,
            maximum: raw.max,
        })
    }
}

pub(crate) fn invalid(code: ErrorCode, summary: &str) -> PlayerError {
    PlayerError::new(code, summary).with_user_action("Open a qualified stem-hls-v1 source")
}
