use crate::{
    ChannelCount, DisplayName, ErrorCode, GainDb, GenerationId, OutputMode, PlaybackPosition,
    PlayerError, PublicDiagnostic, SampleRate, SourceFrame, SourceKind, StemRole, TransportState,
};
use serde::Serialize;
use std::sync::Arc;

pub const MAX_SNAPSHOT_DIAGNOSTICS: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceMetadata {
    pub display_name: DisplayName,
    pub kind: SourceKind,
    pub has_master: bool,
    pub sample_rate: SampleRate,
    pub channel_count: ChannelCount,
    pub start_frame: SourceFrame,
    pub end_frame_exclusive: SourceFrame,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct StemSnapshot {
    pub role: StemRole,
    pub gain_db: GainDb,
    pub muted: bool,
    pub solo: bool,
}

impl StemSnapshot {
    pub const fn neutral(role: StemRole) -> Self {
        Self {
            role,
            gain_db: GainDb::UNITY,
            muted: false,
            solo: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MixerSnapshot {
    pub stems: [StemSnapshot; 4],
    pub output_mode: OutputMode,
}

impl Default for MixerSnapshot {
    fn default() -> Self {
        Self {
            stems: StemRole::ALL.map(StemSnapshot::neutral),
            output_mode: OutputMode::StemMix,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveformState {
    #[default]
    Unavailable,
    Analyzing,
    Ready,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SnapshotDetails {
    pub mixer: MixerSnapshot,
    pub device_name: Option<DisplayName>,
    pub buffering: bool,
    pub waveform: WaveformState,
    pub error: Option<PlayerError>,
}

#[derive(Clone, Debug, Serialize)]
struct PlayerSnapshotData {
    transport_state: TransportState,
    generation: GenerationId,
    position: Option<PlaybackPosition>,
    source: Option<SourceMetadata>,
    diagnostics: Arc<[PublicDiagnostic]>,
    details: SnapshotDetails,
}

#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub struct PlayerSnapshot(Arc<PlayerSnapshotData>);

impl PlayerSnapshot {
    pub fn new(
        transport_state: TransportState,
        generation: GenerationId,
        position: Option<PlaybackPosition>,
        source: Option<SourceMetadata>,
        diagnostics: Vec<PublicDiagnostic>,
    ) -> Result<Self, PlayerError> {
        if diagnostics.len() > MAX_SNAPSHOT_DIAGNOSTICS {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Snapshot diagnostic count exceeds its configured limit",
            ));
        }
        Ok(Self(Arc::new(PlayerSnapshotData {
            transport_state,
            generation,
            position,
            source,
            diagnostics: diagnostics.into(),
            details: SnapshotDetails::default(),
        })))
    }

    pub fn empty(transport_state: TransportState) -> Self {
        Self(Arc::new(PlayerSnapshotData {
            transport_state,
            generation: GenerationId::INITIAL,
            position: None,
            source: None,
            diagnostics: Arc::from([]),
            details: SnapshotDetails::default(),
        }))
    }

    pub fn transport_state(&self) -> TransportState {
        self.0.transport_state
    }

    pub fn diagnostics(&self) -> &[PublicDiagnostic] {
        &self.0.diagnostics
    }

    pub fn generation(&self) -> GenerationId {
        self.0.generation
    }

    pub fn position(&self) -> Option<PlaybackPosition> {
        self.0.position
    }

    pub fn source(&self) -> Option<&SourceMetadata> {
        self.0.source.as_ref()
    }

    pub fn details(&self) -> &SnapshotDetails {
        &self.0.details
    }

    pub fn with_details(&self, details: SnapshotDetails) -> Self {
        Self(Arc::new(PlayerSnapshotData {
            transport_state: self.0.transport_state,
            generation: self.0.generation,
            position: self.0.position,
            source: self.0.source.clone(),
            diagnostics: Arc::clone(&self.0.diagnostics),
            details,
        }))
    }

    pub fn with_position(&self, position: PlaybackPosition) -> Self {
        let mut data = (*self.0).clone();
        data.position = Some(position);
        Self(Arc::new(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Severity;

    #[test]
    fn snapshot_diagnostics_are_bounded() {
        let diagnostic = PublicDiagnostic::new("D_TEST", "test", Severity::Info, vec![]).unwrap();
        let diagnostics = vec![diagnostic; MAX_SNAPSHOT_DIAGNOSTICS + 1];
        assert!(
            PlayerSnapshot::new(
                TransportState::Empty,
                GenerationId::INITIAL,
                None,
                None,
                diagnostics
            )
            .is_err()
        );
    }

    #[test]
    fn snapshot_clone_shares_immutable_data() {
        let snapshot = PlayerSnapshot::empty(TransportState::Ready);
        let clone = snapshot.clone();
        assert!(Arc::ptr_eq(&snapshot.0, &clone.0));
    }
}
