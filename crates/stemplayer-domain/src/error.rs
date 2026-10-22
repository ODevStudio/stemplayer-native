use crate::BoundedText;
use serde::Serialize;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;

const MAX_ERROR_DETAILS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum ErrorCode {
    #[serde(rename = "E_TRANSPORT_STATE_INVALID")]
    TransportStateInvalid,
    #[serde(rename = "E_PLAYBACK_SESSION_CLOSED")]
    PlaybackSessionClosed,
    #[serde(rename = "E_PLAYBACK_NOT_READY")]
    PlaybackNotReady,
    #[serde(rename = "E_PLAYBACK_ALREADY_ACTIVE")]
    PlaybackAlreadyActive,
    #[serde(rename = "E_PLAYBACK_FEATURE_UNSUPPORTED")]
    FeatureUnsupported,
    #[serde(rename = "E_MIXER_GAIN_INVALID")]
    MixerGainInvalid,
    #[serde(rename = "E_MIXER_MASTER_UNAVAILABLE")]
    MixerMasterUnavailable,
    #[serde(rename = "E_COMMAND_QUEUE_FULL")]
    CommandQueueFull,
    #[serde(rename = "E_TIMESCALE_INVALID")]
    TimelineInvalid,
    #[serde(rename = "E_CONTAINER_NOT_MP4")]
    ContainerNotMp4,
    #[serde(rename = "E_CONTAINER_MALFORMED")]
    ContainerMalformed,
    #[serde(rename = "E_NOT_A_STEM_FILE")]
    NotAStemFile,
    #[serde(rename = "E_TRACK_COUNT_UNSUPPORTED")]
    TrackCountUnsupported,
    #[serde(rename = "E_TRACK_ROLE_AMBIGUOUS")]
    TrackRoleAmbiguous,
    #[serde(rename = "E_METADATA_MALFORMED")]
    MetadataMalformed,
    #[serde(rename = "E_METADATA_TOO_LARGE")]
    MetadataTooLarge,
    #[serde(rename = "E_EDIT_LIST_UNSUPPORTED")]
    EditListUnsupported,
    #[serde(rename = "E_EDIT_LIST_CONFLICT")]
    EditListConflict,
    #[serde(rename = "E_PRESENTATION_RANGE_INVALID")]
    PresentationRangeInvalid,
    #[serde(rename = "E_CODEC_UNSUPPORTED")]
    CodecUnsupported,
    #[serde(rename = "E_CODEC_CONFIG_INVALID")]
    CodecConfigInvalid,
    #[serde(rename = "E_DEMUX_FAILED")]
    DemuxFailed,
    #[serde(rename = "E_ENCODED_SAMPLE_INVALID")]
    EncodedSampleInvalid,
    #[serde(rename = "E_DECODE_FAILED")]
    DecodeFailed,
    #[serde(rename = "E_AUDIO_DATA_INVALID")]
    AudioDataInvalid,
    #[serde(rename = "E_TRACK_CONFIGURATION_MISMATCH")]
    TrackConfigurationMismatch,
    #[serde(rename = "E_NO_AUDIO_TRACKS")]
    NoAudioTracks,
    #[serde(rename = "E_MANIFEST_INVALID")]
    ManifestInvalid,
    #[serde(rename = "E_MANIFEST_VERSION_UNSUPPORTED")]
    ManifestVersionUnsupported,
    #[serde(rename = "E_MANIFEST_TOO_LARGE")]
    ManifestTooLarge,
    #[serde(rename = "E_MANIFEST_ROLE_MISSING")]
    ManifestRoleMissing,
    #[serde(rename = "E_MANIFEST_ROLE_DUPLICATE")]
    ManifestRoleDuplicate,
    #[serde(rename = "E_MANIFEST_FILE_MISSING")]
    ManifestFileMissing,
    #[serde(rename = "E_MANIFEST_FILE_AMBIGUOUS")]
    ManifestFileAmbiguous,
    #[serde(rename = "E_MANIFEST_PATH_UNSAFE")]
    ManifestPathUnsafe,
    #[serde(rename = "E_MANIFEST_SHA256_MISMATCH")]
    ManifestSha256Mismatch,
    #[serde(rename = "E_WAV_NOT_RIFF")]
    WavNotRiff,
    #[serde(rename = "E_WAV_MALFORMED")]
    WavMalformed,
    #[serde(rename = "E_WAV_FORMAT_UNSUPPORTED")]
    WavFormatUnsupported,
    #[serde(rename = "E_WAV_CHANNELS_UNSUPPORTED")]
    WavChannelsUnsupported,
    #[serde(rename = "E_WAV_SAMPLE_RATE_MISMATCH")]
    WavSampleRateMismatch,
    #[serde(rename = "E_WAV_FRAME_COUNT_MISMATCH")]
    WavFrameCountMismatch,
    #[serde(rename = "E_WAV_BLOCK_ALIGN_INVALID")]
    WavBlockAlignInvalid,
    #[serde(rename = "E_WAV_DATA_TRUNCATED")]
    WavDataTruncated,
    #[serde(rename = "E_WAV_PCM_INVALID")]
    WavPcmInvalid,
    #[serde(rename = "E_BUNDLE_INCONSISTENT")]
    BundleInconsistent,
    #[serde(rename = "E_BUNDLE_ABORTED")]
    BundleAborted,
    #[serde(rename = "E_HLS_STEMSET_INVALID")]
    HlsStemsetInvalid,
    #[serde(rename = "E_HLS_ROLE_MISSING")]
    HlsRoleMissing,
    #[serde(rename = "E_HLS_TIMELINE_MISMATCH")]
    HlsTimelineMismatch,
    #[serde(rename = "E_HLS_SEGMENT_BOUNDARY_MISMATCH")]
    HlsSegmentBoundaryMismatch,
    #[serde(rename = "E_HLS_URL_UNSAFE")]
    HlsUrlUnsafe,
    #[serde(rename = "E_HLS_RESPONSE_TOO_LARGE")]
    HlsResponseTooLarge,
    #[serde(rename = "E_HLS_HTTP_FAILED")]
    HlsHttpFailed,
    #[serde(rename = "E_HLS_TLS_FAILED")]
    HlsTlsFailed,
    #[serde(rename = "E_HLS_REDIRECT_REJECTED")]
    HlsRedirectRejected,
    #[serde(rename = "E_HLS_GROUP_BUFFERING_TIMEOUT")]
    HlsGroupBufferingTimeout,
    #[serde(rename = "E_HLS_HASH_MISMATCH")]
    HlsHashMismatch,
    #[serde(rename = "E_HLS_CODEC_UNSUPPORTED")]
    HlsCodecUnsupported,
    #[serde(rename = "E_VALUE_OUT_OF_RANGE")]
    ValueOutOfRange,
    #[serde(rename = "E_INTERNAL")]
    Internal,
}

impl ErrorCode {
    pub const ALL: [Self; 61] = [
        Self::TransportStateInvalid,
        Self::PlaybackSessionClosed,
        Self::PlaybackNotReady,
        Self::PlaybackAlreadyActive,
        Self::FeatureUnsupported,
        Self::MixerGainInvalid,
        Self::MixerMasterUnavailable,
        Self::TimelineInvalid,
        Self::ContainerNotMp4,
        Self::ContainerMalformed,
        Self::NotAStemFile,
        Self::TrackCountUnsupported,
        Self::TrackRoleAmbiguous,
        Self::MetadataMalformed,
        Self::MetadataTooLarge,
        Self::EditListUnsupported,
        Self::EditListConflict,
        Self::PresentationRangeInvalid,
        Self::CodecUnsupported,
        Self::CodecConfigInvalid,
        Self::DemuxFailed,
        Self::EncodedSampleInvalid,
        Self::DecodeFailed,
        Self::AudioDataInvalid,
        Self::TrackConfigurationMismatch,
        Self::NoAudioTracks,
        Self::CommandQueueFull,
        Self::ManifestInvalid,
        Self::ManifestVersionUnsupported,
        Self::ManifestTooLarge,
        Self::ManifestRoleMissing,
        Self::ManifestRoleDuplicate,
        Self::ManifestFileMissing,
        Self::ManifestFileAmbiguous,
        Self::ManifestPathUnsafe,
        Self::ManifestSha256Mismatch,
        Self::WavNotRiff,
        Self::WavMalformed,
        Self::WavFormatUnsupported,
        Self::WavChannelsUnsupported,
        Self::WavSampleRateMismatch,
        Self::WavFrameCountMismatch,
        Self::WavBlockAlignInvalid,
        Self::WavDataTruncated,
        Self::WavPcmInvalid,
        Self::BundleInconsistent,
        Self::BundleAborted,
        Self::HlsStemsetInvalid,
        Self::HlsRoleMissing,
        Self::HlsTimelineMismatch,
        Self::HlsSegmentBoundaryMismatch,
        Self::HlsUrlUnsafe,
        Self::HlsResponseTooLarge,
        Self::HlsHttpFailed,
        Self::HlsTlsFailed,
        Self::HlsRedirectRejected,
        Self::HlsGroupBufferingTimeout,
        Self::HlsHashMismatch,
        Self::HlsCodecUnsupported,
        Self::ValueOutOfRange,
        Self::Internal,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TransportStateInvalid => "E_TRANSPORT_STATE_INVALID",
            Self::PlaybackSessionClosed => "E_PLAYBACK_SESSION_CLOSED",
            Self::PlaybackNotReady => "E_PLAYBACK_NOT_READY",
            Self::PlaybackAlreadyActive => "E_PLAYBACK_ALREADY_ACTIVE",
            Self::FeatureUnsupported => "E_PLAYBACK_FEATURE_UNSUPPORTED",
            Self::MixerGainInvalid => "E_MIXER_GAIN_INVALID",
            Self::MixerMasterUnavailable => "E_MIXER_MASTER_UNAVAILABLE",
            Self::CommandQueueFull => "E_COMMAND_QUEUE_FULL",
            Self::TimelineInvalid => "E_TIMESCALE_INVALID",
            Self::ContainerNotMp4 => "E_CONTAINER_NOT_MP4",
            Self::ContainerMalformed => "E_CONTAINER_MALFORMED",
            Self::NotAStemFile => "E_NOT_A_STEM_FILE",
            Self::TrackCountUnsupported => "E_TRACK_COUNT_UNSUPPORTED",
            Self::TrackRoleAmbiguous => "E_TRACK_ROLE_AMBIGUOUS",
            Self::MetadataMalformed => "E_METADATA_MALFORMED",
            Self::MetadataTooLarge => "E_METADATA_TOO_LARGE",
            Self::EditListUnsupported => "E_EDIT_LIST_UNSUPPORTED",
            Self::EditListConflict => "E_EDIT_LIST_CONFLICT",
            Self::PresentationRangeInvalid => "E_PRESENTATION_RANGE_INVALID",
            Self::CodecUnsupported => "E_CODEC_UNSUPPORTED",
            Self::CodecConfigInvalid => "E_CODEC_CONFIG_INVALID",
            Self::DemuxFailed => "E_DEMUX_FAILED",
            Self::EncodedSampleInvalid => "E_ENCODED_SAMPLE_INVALID",
            Self::DecodeFailed => "E_DECODE_FAILED",
            Self::AudioDataInvalid => "E_AUDIO_DATA_INVALID",
            Self::TrackConfigurationMismatch => "E_TRACK_CONFIGURATION_MISMATCH",
            Self::NoAudioTracks => "E_NO_AUDIO_TRACKS",
            Self::ManifestInvalid => "E_MANIFEST_INVALID",
            Self::ManifestVersionUnsupported => "E_MANIFEST_VERSION_UNSUPPORTED",
            Self::ManifestTooLarge => "E_MANIFEST_TOO_LARGE",
            Self::ManifestRoleMissing => "E_MANIFEST_ROLE_MISSING",
            Self::ManifestRoleDuplicate => "E_MANIFEST_ROLE_DUPLICATE",
            Self::ManifestFileMissing => "E_MANIFEST_FILE_MISSING",
            Self::ManifestFileAmbiguous => "E_MANIFEST_FILE_AMBIGUOUS",
            Self::ManifestPathUnsafe => "E_MANIFEST_PATH_UNSAFE",
            Self::ManifestSha256Mismatch => "E_MANIFEST_SHA256_MISMATCH",
            Self::WavNotRiff => "E_WAV_NOT_RIFF",
            Self::WavMalformed => "E_WAV_MALFORMED",
            Self::WavFormatUnsupported => "E_WAV_FORMAT_UNSUPPORTED",
            Self::WavChannelsUnsupported => "E_WAV_CHANNELS_UNSUPPORTED",
            Self::WavSampleRateMismatch => "E_WAV_SAMPLE_RATE_MISMATCH",
            Self::WavFrameCountMismatch => "E_WAV_FRAME_COUNT_MISMATCH",
            Self::WavBlockAlignInvalid => "E_WAV_BLOCK_ALIGN_INVALID",
            Self::WavDataTruncated => "E_WAV_DATA_TRUNCATED",
            Self::WavPcmInvalid => "E_WAV_PCM_INVALID",
            Self::BundleInconsistent => "E_BUNDLE_INCONSISTENT",
            Self::BundleAborted => "E_BUNDLE_ABORTED",
            Self::HlsStemsetInvalid => "E_HLS_STEMSET_INVALID",
            Self::HlsRoleMissing => "E_HLS_ROLE_MISSING",
            Self::HlsTimelineMismatch => "E_HLS_TIMELINE_MISMATCH",
            Self::HlsSegmentBoundaryMismatch => "E_HLS_SEGMENT_BOUNDARY_MISMATCH",
            Self::HlsUrlUnsafe => "E_HLS_URL_UNSAFE",
            Self::HlsResponseTooLarge => "E_HLS_RESPONSE_TOO_LARGE",
            Self::HlsHttpFailed => "E_HLS_HTTP_FAILED",
            Self::HlsTlsFailed => "E_HLS_TLS_FAILED",
            Self::HlsRedirectRejected => "E_HLS_REDIRECT_REJECTED",
            Self::HlsGroupBufferingTimeout => "E_HLS_GROUP_BUFFERING_TIMEOUT",
            Self::HlsHashMismatch => "E_HLS_HASH_MISMATCH",
            Self::HlsCodecUnsupported => "E_HLS_CODEC_UNSUPPORTED",
            Self::ValueOutOfRange => "E_VALUE_OUT_OF_RANGE",
            Self::Internal => "E_INTERNAL",
        }
    }

    pub const fn category(self) -> ErrorCategory {
        match self {
            Self::TransportStateInvalid
            | Self::PlaybackSessionClosed
            | Self::PlaybackNotReady
            | Self::PlaybackAlreadyActive
            | Self::FeatureUnsupported
            | Self::CommandQueueFull => ErrorCategory::Lifecycle,
            Self::MixerGainInvalid | Self::MixerMasterUnavailable => ErrorCategory::Mixer,
            Self::TimelineInvalid => ErrorCategory::Timeline,
            Self::ContainerNotMp4
            | Self::ContainerMalformed
            | Self::NotAStemFile
            | Self::TrackCountUnsupported
            | Self::TrackRoleAmbiguous
            | Self::MetadataMalformed
            | Self::MetadataTooLarge
            | Self::EditListUnsupported
            | Self::EditListConflict
            | Self::PresentationRangeInvalid
            | Self::CodecUnsupported
            | Self::CodecConfigInvalid
            | Self::DemuxFailed
            | Self::EncodedSampleInvalid
            | Self::DecodeFailed
            | Self::AudioDataInvalid
            | Self::TrackConfigurationMismatch
            | Self::NoAudioTracks
            | Self::ManifestInvalid
            | Self::ManifestVersionUnsupported
            | Self::ManifestTooLarge
            | Self::ManifestRoleMissing
            | Self::ManifestRoleDuplicate
            | Self::ManifestFileMissing
            | Self::ManifestFileAmbiguous
            | Self::ManifestPathUnsafe
            | Self::ManifestSha256Mismatch
            | Self::WavNotRiff
            | Self::WavMalformed
            | Self::WavFormatUnsupported
            | Self::WavChannelsUnsupported
            | Self::WavSampleRateMismatch
            | Self::WavFrameCountMismatch
            | Self::WavBlockAlignInvalid
            | Self::WavDataTruncated
            | Self::WavPcmInvalid
            | Self::BundleInconsistent
            | Self::BundleAborted
            | Self::HlsStemsetInvalid
            | Self::HlsRoleMissing
            | Self::HlsTimelineMismatch
            | Self::HlsSegmentBoundaryMismatch
            | Self::HlsUrlUnsafe
            | Self::HlsResponseTooLarge
            | Self::HlsHashMismatch
            | Self::HlsCodecUnsupported => ErrorCategory::Source,
            Self::HlsHttpFailed
            | Self::HlsTlsFailed
            | Self::HlsRedirectRejected
            | Self::HlsGroupBufferingTimeout => ErrorCategory::Network,
            Self::ValueOutOfRange | Self::Internal => ErrorCategory::Internal,
        }
    }

    pub const fn recoverability(self) -> Recoverability {
        match self {
            Self::TransportStateInvalid
            | Self::PlaybackSessionClosed
            | Self::PlaybackNotReady
            | Self::PlaybackAlreadyActive
            | Self::BundleAborted => Recoverability::ReloadSession,
            Self::FeatureUnsupported | Self::HlsRedirectRejected => {
                Recoverability::ChooseAnotherSource
            }
            Self::Internal => Recoverability::RestartApplication,
            _ => match self.category() {
                ErrorCategory::Source | ErrorCategory::Timeline => {
                    Recoverability::ChooseAnotherSource
                }
                _ => Recoverability::Retry,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCategory {
    Lifecycle,
    Mixer,
    Timeline,
    Source,
    Network,
    Internal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Fatal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Recoverability {
    Retry,
    ChooseAnotherSource,
    ReloadSession,
    RestartApplication,
    FatalRelease,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ErrorDetail {
    pub key: BoundedText<64>,
    pub value: BoundedText<128>,
}

#[derive(Clone, Serialize)]
pub struct PlayerError {
    pub code: ErrorCode,
    pub category: ErrorCategory,
    pub summary: BoundedText<256>,
    pub user_action: Option<BoundedText<256>>,
    pub details: Arc<[ErrorDetail]>,
    pub severity: Severity,
    pub recoverability: Recoverability,
    #[serde(skip)]
    source: Option<Arc<dyn Error + Send + Sync>>,
}

impl PlayerError {
    pub fn new(code: ErrorCode, summary: impl AsRef<str>) -> Self {
        Self {
            code,
            category: code.category(),
            summary: BoundedText::truncated(summary),
            user_action: None,
            details: Arc::from([]),
            severity: Severity::Error,
            recoverability: code.recoverability(),
            source: None,
        }
    }

    pub fn with_user_action(mut self, action: impl AsRef<str>) -> Self {
        self.user_action = Some(BoundedText::truncated(action));
        self
    }

    pub fn with_details(mut self, details: Vec<ErrorDetail>) -> Result<Self, Self> {
        if details.len() > MAX_ERROR_DETAILS {
            return Err(Self::new(
                ErrorCode::ValueOutOfRange,
                "Error detail count exceeds its configured limit",
            ));
        }
        self.details = details.into();
        Ok(self)
    }

    pub fn with_source(mut self, source: impl Error + Send + Sync + 'static) -> Self {
        self.source = Some(Arc::new(source));
        self
    }
}

impl Debug for PlayerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlayerError")
            .field("code", &self.code)
            .field("category", &self.category)
            .field("summary", &self.summary)
            .field("user_action", &self.user_action)
            .field("details", &self.details)
            .field("severity", &self.severity)
            .field("recoverability", &self.recoverability)
            .finish_non_exhaustive()
    }
}

impl Display for PlayerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.summary)
    }
}

impl Error for PlayerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_public_code_has_a_stable_serialized_name() {
        for code in ErrorCode::ALL {
            let json = serde_json::to_string(&code).unwrap();
            assert_eq!(json, format!("\"{}\"", code.as_str()));
        }
    }

    #[test]
    fn public_serialization_redacts_internal_sources() {
        let error = PlayerError::new(ErrorCode::Internal, "Operation failed")
            .with_source(std::io::Error::other("C:\\private\\track.wav"));
        let json = serde_json::to_string(&error).unwrap();
        assert!(!json.contains("private"));
        assert!(!json.contains("track.wav"));
    }

    #[test]
    fn error_details_are_bounded() {
        let detail = ErrorDetail {
            key: BoundedText::new("role").unwrap(),
            value: BoundedText::new("vocals").unwrap(),
        };
        let details = vec![detail; MAX_ERROR_DETAILS + 1];
        assert!(
            PlayerError::new(ErrorCode::Internal, "failed")
                .with_details(details)
                .is_err()
        );
    }

    #[test]
    fn recovery_metadata_distinguishes_source_session_and_transient_failures() {
        for (code, recovery) in [
            (ErrorCode::ManifestInvalid, "choose-another-source"),
            (ErrorCode::ManifestPathUnsafe, "choose-another-source"),
            (ErrorCode::DecodeFailed, "choose-another-source"),
            (ErrorCode::HlsRedirectRejected, "choose-another-source"),
            (ErrorCode::HlsHttpFailed, "retry"),
            (ErrorCode::HlsTlsFailed, "retry"),
            (ErrorCode::CommandQueueFull, "retry"),
            (ErrorCode::BundleAborted, "reload-session"),
            (ErrorCode::PlaybackSessionClosed, "reload-session"),
            (ErrorCode::Internal, "restart-application"),
        ] {
            let json = serde_json::to_value(PlayerError::new(code, "Operation failed")).unwrap();
            assert_eq!(json["recoverability"], recovery, "{}", code.as_str());
        }
        for code in ErrorCode::ALL {
            if matches!(code.category(), ErrorCategory::Source) && code != ErrorCode::BundleAborted
            {
                assert_eq!(code.recoverability(), Recoverability::ChooseAnotherSource);
            }
        }
    }
}
