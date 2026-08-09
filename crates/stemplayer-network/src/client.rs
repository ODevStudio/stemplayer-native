use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use stemplayer_domain::{ErrorCode, PlayerError, SourceLocator};

use crate::manifest::{
    HlsManifest, HlsRole, HlsWaveform, MAX_MANIFEST_BYTES, MAX_WAVEFORM_BYTES, invalid,
};
use crate::playlist::{MAX_PLAYLIST_BYTES, MediaPlaylist, validate_parity};
use crate::source::HlsPlaybackSource;
use crate::url_policy::entry_url;

#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
}

pub trait HttpTransport: Send + Sync {
    fn get(&self, url: &url::Url, maximum_bytes: usize) -> Result<HttpResponse, PlayerError>;
}

pub struct HlsClient {
    transport: Arc<dyn HttpTransport>,
    allow_loopback_http: bool,
}

impl HlsClient {
    pub fn new(transport: Arc<dyn HttpTransport>, allow_loopback_http: bool) -> Self {
        Self {
            transport,
            allow_loopback_http,
        }
    }

    pub fn open(
        &self,
        value: &str,
        cancelled: &AtomicBool,
    ) -> Result<Arc<HlsPlaybackSource>, PlayerError> {
        let started = Instant::now();
        let url = entry_url(value, self.allow_loopback_http)?;
        let manifest_response = self.transport.get(&url, MAX_MANIFEST_BYTES)?;
        check_cancelled(cancelled)?;
        let manifest = HlsManifest::parse(&manifest_response.bytes, &url)?;
        let mut playlists = Vec::with_capacity(manifest.routes.len());
        for route in &manifest.routes {
            let response = self
                .transport
                .get(&route.playlist_url, MAX_PLAYLIST_BYTES)?;
            check_cancelled(cancelled)?;
            playlists.push(MediaPlaylist::parse(
                &response.bytes,
                &route.playlist_url,
                manifest.sample_rate,
                manifest.preroll_frames,
            )?);
        }
        validate_parity(&playlists, manifest.duration_us, manifest.segment_count)?;
        let mut waveforms = [None, None, None, None];
        for route in manifest
            .routes
            .iter()
            .filter(|route| matches!(route.role, HlsRole::Stem(_)))
        {
            let response = self
                .transport
                .get(&route.waveform_url, MAX_WAVEFORM_BYTES)?;
            check_cancelled(cancelled)?;
            waveforms[route.role.index()] = Some(HlsWaveform::parse(&response.bytes)?);
        }
        if waveforms.iter().any(Option::is_none) {
            return Err(invalid(
                ErrorCode::HlsRoleMissing,
                "HLS source is missing a stem waveform",
            ));
        }
        let waveforms = waveforms.map(Option::unwrap);
        let samples_per_peak = waveforms[0].samples_per_peak;
        let peak_count = waveforms[0].minimum.len();
        if waveforms.iter().any(|waveform| {
            waveform.samples_per_peak != samples_per_peak || waveform.minimum.len() != peak_count
        }) {
            return Err(invalid(
                ErrorCode::HlsStemsetInvalid,
                "HLS stem waveforms are not aligned",
            ));
        }
        HlsPlaybackSource::new(
            Arc::clone(&self.transport),
            manifest,
            playlists,
            waveforms,
            started.elapsed(),
        )
        .map(Arc::new)
    }
}

impl crate::NetworkService for HlsClient {
    fn validate_source(&self, source: &SourceLocator) -> Result<(), PlayerError> {
        match source {
            SourceLocator::HlsUrl(value) => {
                entry_url(value.as_str(), self.allow_loopback_http).map(drop)
            }
            SourceLocator::LocalPath(_) => Err(PlayerError::new(
                ErrorCode::FeatureUnsupported,
                "Network service accepts only HLS URLs",
            )),
        }
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), PlayerError> {
    if cancelled.load(Ordering::Acquire) {
        Err(PlayerError::new(
            ErrorCode::BundleAborted,
            "HLS source operation was cancelled",
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct MemoryTransport(BTreeMap<String, Vec<u8>>);

    impl HttpTransport for MemoryTransport {
        fn get(&self, url: &url::Url, maximum_bytes: usize) -> Result<HttpResponse, PlayerError> {
            let bytes =
                self.0.get(url.as_str()).cloned().ok_or_else(|| {
                    PlayerError::new(ErrorCode::HlsHttpFailed, "missing response")
                })?;
            if bytes.len() > maximum_bytes {
                return Err(PlayerError::new(
                    ErrorCode::HlsResponseTooLarge,
                    "response too large",
                ));
            }
            Ok(HttpResponse {
                bytes,
                content_type: None,
            })
        }
    }

    fn responses(manifest_change: impl FnOnce(&mut serde_json::Value)) -> MemoryTransport {
        let route = |name: &str| {
            serde_json::json!({
                "playlist": format!("{name}.m3u8"),
                "waveform": format!("{name}.waveform.json")
            })
        };
        let mut manifest = serde_json::json!({
            "schema": "stem-hls-v1",
            "manifest_version": 1,
            "format_version": "stem-hls-v1",
            "title": "Synthetic HLS",
            "duration_us": 2_000_000,
            "sample_rate": 48_000,
            "channels": 2,
            "codec": { "name": "AAC-LC", "codec_string": "mp4a.40.2", "segment_format": "M4A", "bitrate_bps": 96_000 },
            "segment_duration_us": 2_000_000,
            "segment_count": 1,
            "master_present": true,
            "master": route("master"),
            "stems": {
                "drums": route("drums"),
                "bass": route("bass"),
                "other": route("other"),
                "vocals": route("vocals")
            }
        });
        manifest_change(&mut manifest);
        let playlist = b"#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:2.000000,\nsegment.m4a\n#EXT-X-ENDLIST\n".to_vec();
        let waveform = serde_json::to_vec(&serde_json::json!({
            "schema": "stem-waveform-v1",
            "samples_per_peak": 48_000,
            "min": [-0.5, -0.25],
            "max": [0.5, 0.25]
        }))
        .unwrap();
        let base = "https://example.test/media/";
        let mut responses = BTreeMap::from([(
            format!("{base}stemset.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )]);
        for role in ["drums", "bass", "other", "vocals", "master"] {
            responses.insert(format!("{base}{role}.m3u8"), playlist.clone());
            responses.insert(format!("{base}{role}.waveform.json"), waveform.clone());
        }
        MemoryTransport(responses)
    }

    #[test]
    fn opens_validated_stemset_without_fetching_segments() {
        let client = HlsClient::new(Arc::new(responses(|_| {})), false);
        let source = client
            .open(
                "https://example.test/media/stemset.json",
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(source.frames(), 96_000);
        assert!(source.has_master());
        assert_eq!(source.waveforms()[0].minimum, [-0.5, -0.25]);
        assert_eq!(source.diagnostics().fetch_count, 10);
        assert_eq!(
            source.diagnostics().source,
            "https://example.test/2 path component(s)"
        );
    }

    #[test]
    fn rejects_unsafe_references_and_timeline_mismatch() {
        let unsafe_client = HlsClient::new(
            Arc::new(responses(|manifest| {
                manifest["stems"]["drums"]["playlist"] = "../drums.m3u8".into();
            })),
            false,
        );
        assert_eq!(
            unsafe_client
                .open(
                    "https://example.test/media/stemset.json",
                    &AtomicBool::new(false),
                )
                .err()
                .unwrap()
                .code,
            ErrorCode::HlsUrlUnsafe
        );

        let mut transport = responses(|_| {});
        transport.0.insert(
            "https://example.test/media/vocals.m3u8".into(),
            b"#EXTM3U\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:1.999999,\nsegment.m4a\n#EXT-X-ENDLIST\n"
                .to_vec(),
        );
        let mismatch = HlsClient::new(Arc::new(transport), false);
        assert_eq!(
            mismatch
                .open(
                    "https://example.test/media/stemset.json",
                    &AtomicBool::new(false),
                )
                .err()
                .unwrap()
                .code,
            ErrorCode::HlsTimelineMismatch
        );
    }

    #[test]
    fn cancellation_stops_after_bounded_fetch() {
        let client = HlsClient::new(Arc::new(responses(|_| {})), false);
        assert_eq!(
            client
                .open(
                    "https://example.test/media/stemset.json",
                    &AtomicBool::new(true),
                )
                .err()
                .unwrap()
                .code,
            ErrorCode::BundleAborted
        );
    }

    #[test]
    fn rejects_misaligned_stem_waveforms() {
        for waveform in [
            serde_json::json!({
                "schema": "stem-waveform-v1",
                "samples_per_peak": 24_000,
                "min": [-0.5, -0.25],
                "max": [0.5, 0.25]
            }),
            serde_json::json!({
                "schema": "stem-waveform-v1",
                "samples_per_peak": 48_000,
                "min": [-0.5],
                "max": [0.5]
            }),
        ] {
            let mut transport = responses(|_| {});
            transport.0.insert(
                "https://example.test/media/vocals.waveform.json".into(),
                serde_json::to_vec(&waveform).unwrap(),
            );
            let client = HlsClient::new(Arc::new(transport), false);

            assert_eq!(
                client
                    .open(
                        "https://example.test/media/stemset.json",
                        &AtomicBool::new(false),
                    )
                    .err()
                    .unwrap()
                    .code,
                ErrorCode::HlsStemsetInvalid
            );
        }
    }
}
