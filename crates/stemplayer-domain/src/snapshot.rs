use crate::{
    BoundedText, ChannelCount, DisplayName, ErrorCode, GainDb, GenerationId, OutputMode,
    OutputPreference, PlaybackPosition, PlayerError, PublicDiagnostic, SampleRate, SourceFrame,
    SourceKind, StemRole, TransportState,
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
#[serde(rename_all = "snake_case")]
pub enum AutomationReason {
    Gain,
    Mute,
    Solo,
    Reset,
    OutputPreference,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MixerSnapshot {
    pub stems: [StemSnapshot; 4],
    pub output_preference: OutputPreference,
    pub output_mode: OutputMode,
    pub automation_count: u64,
    pub crossfade_count: u64,
    pub current_linear_gains: [f32; 4],
    pub target_linear_gains: [f32; 4],
    pub remaining_gain_frames: [u32; 4],
    pub current_master_gain: f32,
    pub target_master_gain: f32,
    pub remaining_crossfade_frames: u32,
    pub last_automation_reason: Option<AutomationReason>,
    pub source_peak: [f32; 2],
    pub output_peak: [f32; 2],
    pub non_finite_samples: u64,
}

impl Default for MixerSnapshot {
    fn default() -> Self {
        Self {
            stems: StemRole::ALL.map(StemSnapshot::neutral),
            output_preference: OutputPreference::Automatic,
            output_mode: OutputMode::StemMix,
            automation_count: 0,
            crossfade_count: 0,
            current_linear_gains: [1.0; 4],
            target_linear_gains: [1.0; 4],
            remaining_gain_frames: [0; 4],
            current_master_gain: 0.0,
            target_master_gain: 0.0,
            remaining_crossfade_frames: 0,
            last_automation_reason: None,
            source_peak: [0.0; 2],
            output_peak: [0.0; 2],
            non_finite_samples: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ResamplerSnapshot {
    pub input_rate: u32,
    pub output_rate: u32,
    pub delay_frames: u32,
    pub input_frames: u64,
    pub output_frames: u64,
    pub phase_numerator: u32,
    pub phase_denominator: u32,
    pub buffer_occupancy_frames: u32,
    pub reset_count: u64,
    pub discontinuity_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceDiagnostics {
    pub container: BoundedText<32>,
    pub tracks: BoundedText<1024>,
    pub metadata_read_bytes: u64,
    pub media_read_bytes: u64,
    pub maximum_pending_blocks: u8,
    pub seek_packet_count: u32,
    pub seek_discarded_frames: u64,
    pub seek_latency_us: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NetworkDiagnostics {
    pub redacted_source: BoundedText<256>,
    pub manifest_latency_us: u64,
    pub group_ready_us: u64,
    pub fetch_count: u64,
    pub fetch_failures: u64,
    pub buffering_count: u64,
    pub recovery_us: u64,
    pub current_group: Option<u32>,
    pub cache_bytes: u64,
    pub peak_cache_bytes: u64,
    pub packets_decoded: u64,
    pub discarded_preroll_frames: u64,
    pub inter_stem_offset_frames: i64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveformState {
    #[default]
    Unavailable,
    Analyzing,
    Ready,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveformCacheState {
    #[default]
    Disabled,
    Miss,
    Hit,
    Written,
    Error,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SnapshotDetails {
    pub mixer: MixerSnapshot,
    pub device_name: Option<DisplayName>,
    pub buffering: bool,
    pub buffered_source_frames: u64,
    pub device_queue_frames: u64,
    pub audio_stream_active: bool,
    pub underrun_count: u64,
    pub overflow_count: u64,
    pub resampler: Option<ResamplerSnapshot>,
    pub source_diagnostics: Option<SourceDiagnostics>,
    pub network_diagnostics: Option<NetworkDiagnostics>,
    pub waveform: WaveformState,
    pub waveform_progress_percent: u8,
    pub waveform_cache: WaveformCacheState,
    pub waveform_throttled: bool,
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
