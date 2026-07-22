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
    #[serde(rename = "E_VALUE_OUT_OF_RANGE")]
    ValueOutOfRange,
    #[serde(rename = "E_INTERNAL")]
    Internal,
}

impl ErrorCode {
    pub const ALL: [Self; 11] = [
        Self::TransportStateInvalid,
        Self::PlaybackSessionClosed,
        Self::PlaybackNotReady,
        Self::PlaybackAlreadyActive,
        Self::FeatureUnsupported,
        Self::MixerGainInvalid,
        Self::MixerMasterUnavailable,
        Self::TimelineInvalid,
        Self::CommandQueueFull,
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
            Self::ValueOutOfRange => "E_VALUE_OUT_OF_RANGE",
            Self::Internal => "E_INTERNAL",
            Self::TimelineInvalid => "E_TIMESCALE_INVALID",
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
            Self::ValueOutOfRange | Self::Internal => ErrorCategory::Internal,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCategory {
    Lifecycle,
    Mixer,
    Timeline,
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
            recoverability: Recoverability::Retry,
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
}
