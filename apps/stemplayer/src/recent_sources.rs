use stemplayer_domain::{ErrorCode, PlayerError};
use stemplayer_platform::LocalSettings;

pub(crate) fn is_url(value: &str) -> bool {
    let value = value.trim_ascii_start();
    value.contains("://")
        || value.split_once(':').is_some_and(|(prefix, rest)| {
            !prefix.contains(['/', '\\'])
                && !(prefix.len() == 1
                    && prefix.as_bytes()[0].is_ascii_alphabetic()
                    && (rest.starts_with('/') || rest.starts_with('\\')))
        })
}

pub(crate) fn validated_url(value: &str) -> Result<String, PlayerError> {
    if value.len() > 2_048 {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "URL exceeds its configured limit",
        ));
    }
    #[cfg(feature = "hls")]
    {
        stemplayer_network::entry_url(value, cfg!(debug_assertions))
            .map(|url| url.as_str().to_owned())
    }
    #[cfg(not(feature = "hls"))]
    {
        let _ = value;
        Err(PlayerError::new(
            ErrorCode::HlsUrlUnsafe,
            "HLS support is unavailable",
        ))
    }
}

pub(crate) fn sanitize(settings: &LocalSettings) -> LocalSettings {
    LocalSettings {
        recent_sources: settings
            .recent_sources
            .iter()
            .filter(|value| {
                settings.recent_sources_enabled && (!is_url(value) || validated_url(value).is_ok())
            })
            .cloned()
            .collect(),
        ..settings.clone()
    }
}

#[cfg(any(test, all(feature = "local-wav", not(feature = "development"))))]
pub(crate) fn label(value: &str) -> String {
    if is_url(value) {
        #[cfg(feature = "hls")]
        if value.len() <= 2_048
            && let Ok(url) = stemplayer_network::entry_url(value, cfg!(debug_assertions))
        {
            return format!("HLS ({})", url.origin().ascii_serialization())
                .chars()
                .take(96)
                .collect();
        }
        return "HLS (unavailable)".into();
    }
    let path = std::path::Path::new(value);
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Local source");
    let parent = path
        .parent()
        .and_then(std::path::Path::file_name)
        .and_then(|value| value.to_str());
    parent
        .map_or_else(|| name.to_owned(), |parent| format!("{name} ({parent})"))
        .chars()
        .take(96)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_recent_urls_are_removed_and_never_expose_secrets_in_labels() {
        let unsafe_urls = [
            "https://user:synthetic-secret@example.test/stemset.json",
            "https://example.test/stemset.json?token=synthetic-secret",
            "https://example.test/stemset.json#synthetic-secret",
            "HTTPS://user:synthetic-secret@example.test/stemset.json",
            "https:user:synthetic-secret@example.test/stemset.json",
            "ftp:user:synthetic-secret@example.test/stemset.json",
            "x:user:synthetic-secret@example.test/stemset.json",
        ];
        let settings = LocalSettings {
            recent_sources: unsafe_urls
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            ..LocalSettings::default()
        };
        assert!(sanitize(&settings).recent_sources.is_empty());
        for url in unsafe_urls {
            assert!(validated_url(url).is_err());
            assert!(!label(url).contains("synthetic-secret"));
        }
    }

    #[test]
    fn oversized_urls_are_rejected_before_url_policy_parsing() {
        assert_eq!(
            validated_url(&"x".repeat(2_049)).unwrap_err().code,
            ErrorCode::ValueOutOfRange
        );
    }

    #[test]
    fn local_paths_are_not_mistaken_for_uri_schemes() {
        for value in [
            "C:\\music\\track.mp3",
            "C:/music/track.mp3",
            "\\\\?\\C:\\music\\track.mp3",
            "\\\\server\\share\\track.mp3",
            "./artist:track.mp3",
            "track.mp3",
        ] {
            assert!(!is_url(value));
        }
    }

    #[cfg(feature = "hls")]
    #[test]
    fn recent_urls_reuse_entry_policy_and_labels_contain_only_the_origin() {
        let value = "https://example.test:8443/private/stemset.json";
        assert_eq!(validated_url(value).unwrap(), value);
        assert_eq!(label(value), "HLS (https://example.test:8443)");
        assert!(validated_url("http://example.test/stemset.json").is_err());
        assert_eq!(
            validated_url("http://127.0.0.1/stemset.json").is_ok(),
            cfg!(debug_assertions)
        );
    }

    #[test]
    fn disabling_history_clears_legacy_entries() {
        let settings = LocalSettings {
            recent_sources_enabled: false,
            recent_sources: vec!["track.mp3".into()],
            ..LocalSettings::default()
        };
        assert!(sanitize(&settings).recent_sources.is_empty());
    }
}
