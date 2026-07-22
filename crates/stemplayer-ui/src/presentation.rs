use stemplayer_domain::{
    Microseconds, OutputMode, PlayerSnapshot, SourceKind, StemRole, TransportState, WaveformState,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransportAvailability {
    pub open: bool,
    pub play: bool,
    pub pause: bool,
    pub stop: bool,
    pub seek: bool,
    pub mixer: bool,
}

impl TransportAvailability {
    fn from_state(state: TransportState, loaded: bool) -> Self {
        let closed = state == TransportState::Closed;
        Self {
            open: !closed && !matches!(state, TransportState::Inspecting | TransportState::Seeking),
            play: matches!(
                state,
                TransportState::Ready | TransportState::Paused | TransportState::Ended
            ),
            pause: matches!(state, TransportState::Priming | TransportState::Playing),
            stop: loaded
                && !matches!(
                    state,
                    TransportState::Inspecting | TransportState::Error | TransportState::Closed
                ),
            seek: loaded
                && !matches!(
                    state,
                    TransportState::Inspecting | TransportState::Error | TransportState::Closed
                ),
            mixer: loaded
                && !matches!(
                    state,
                    TransportState::Inspecting | TransportState::Error | TransportState::Closed
                ),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MixerRow {
    pub role: StemRole,
    pub label: &'static str,
    pub gain_db: f32,
    pub gain_text: String,
    pub muted: bool,
    pub solo: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresentationModel {
    pub transport: TransportAvailability,
    pub state_text: &'static str,
    pub current_time: String,
    pub total_time: String,
    pub mixer_rows: [MixerRow; 4],
    pub output_mode: &'static str,
    pub buffering_banner: Option<&'static str>,
    pub error_banner: Option<String>,
    pub file_name: String,
    pub source_summary: String,
    pub device_name: String,
    pub waveform_text: &'static str,
    pub diagnostics_count: usize,
}

impl PresentationModel {
    pub fn from_snapshot(snapshot: &PlayerSnapshot) -> Self {
        let source = snapshot.source();
        let position = snapshot
            .position()
            .map(|value| value.display_microseconds())
            .unwrap_or(Microseconds::new(0));
        let duration = source
            .map(|value| {
                let frames = value
                    .end_frame_exclusive
                    .get()
                    .saturating_sub(value.start_frame.get());
                Microseconds::new(
                    frames.saturating_mul(1_000_000) / i64::from(value.sample_rate.get()),
                )
            })
            .unwrap_or(Microseconds::new(0));
        let details = snapshot.details();
        Self {
            transport: TransportAvailability::from_state(
                snapshot.transport_state(),
                source.is_some(),
            ),
            state_text: state_text(snapshot.transport_state()),
            current_time: format_time(position),
            total_time: format_time(duration),
            mixer_rows: details.mixer.stems.map(|stem| MixerRow {
                role: stem.role,
                label: role_label(stem.role),
                gain_db: stem.gain_db.get(),
                gain_text: format!("{:+.1} dB", stem.gain_db.get()),
                muted: stem.muted,
                solo: stem.solo,
            }),
            output_mode: match details.mixer.output_mode {
                OutputMode::MasterReference => "Master reference",
                OutputMode::StemMix => "Stem mix",
            },
            buffering_banner: details.buffering.then_some("Buffering"),
            error_banner: details
                .error
                .as_ref()
                .map(|error| error.summary.as_str().to_owned()),
            file_name: source
                .map(|value| value.display_name.as_str().to_owned())
                .unwrap_or_else(|| "No source loaded".into()),
            source_summary: source.map(source_summary).unwrap_or_default(),
            device_name: details
                .device_name
                .as_ref()
                .map(|value| value.as_str().to_owned())
                .unwrap_or_else(|| "System default".into()),
            waveform_text: match details.waveform {
                WaveformState::Unavailable => "Waveform unavailable",
                WaveformState::Analyzing => "Analyzing waveform",
                WaveformState::Ready => "Waveform ready",
            },
            diagnostics_count: snapshot.diagnostics().len(),
        }
    }
}

fn format_time(value: Microseconds) -> String {
    let seconds = value.get().max(0) / 1_000_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn source_summary(source: &stemplayer_domain::SourceMetadata) -> String {
    let kind = match source.kind {
        SourceKind::StudioWavBundle => "Studio WAV",
        SourceKind::LocalStemContainer => "Stem container",
        SourceKind::HlsStemset => "HLS stemset",
    };
    format!(
        "{kind} | {} Hz | {} channels",
        source.sample_rate.get(),
        source.channel_count.get()
    )
}

const fn state_text(state: TransportState) -> &'static str {
    match state {
        TransportState::Empty => "Empty",
        TransportState::Inspecting => "Inspecting",
        TransportState::Ready => "Ready",
        TransportState::Priming => "Priming",
        TransportState::Playing => "Playing",
        TransportState::Paused => "Paused",
        TransportState::Seeking => "Seeking",
        TransportState::Ended => "Ended",
        TransportState::Error => "Error",
        TransportState::Closed => "Closed",
    }
}

const fn role_label(role: StemRole) -> &'static str {
    match role {
        StemRole::Drums => "Drums",
        StemRole::Bass => "Bass",
        StemRole::Other => "Other",
        StemRole::Vocals => "Vocals",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_transport_state_has_deterministic_enablement() {
        for state in TransportState::ALL {
            let model = PresentationModel::from_snapshot(&PlayerSnapshot::empty(state));
            assert_eq!(model.state_text, state_text(state));
            assert_eq!(
                model.transport.open,
                state != TransportState::Closed
                    && !matches!(state, TransportState::Inspecting | TransportState::Seeking)
            );
            assert!(!model.transport.mixer);
        }
    }

    #[test]
    fn loaded_transport_enablement_covers_every_state() {
        for state in TransportState::ALL {
            let snapshot = PlayerSnapshot::new(
                state,
                stemplayer_domain::GenerationId::INITIAL,
                None,
                Some(stemplayer_domain::SourceMetadata {
                    display_name: stemplayer_domain::DisplayName::new("demo").unwrap(),
                    kind: SourceKind::StudioWavBundle,
                    has_master: true,
                    sample_rate: stemplayer_domain::SampleRate::new(48_000).unwrap(),
                    channel_count: stemplayer_domain::ChannelCount::new(10).unwrap(),
                    start_frame: stemplayer_domain::SourceFrame::new(0),
                    end_frame_exclusive: stemplayer_domain::SourceFrame::new(28_800_000),
                }),
                vec![],
            )
            .unwrap();
            let transport = PresentationModel::from_snapshot(&snapshot).transport;
            assert_eq!(
                transport.play,
                matches!(
                    state,
                    TransportState::Ready | TransportState::Paused | TransportState::Ended
                )
            );
            assert_eq!(
                transport.mixer,
                !matches!(
                    state,
                    TransportState::Inspecting | TransportState::Error | TransportState::Closed
                )
            );
            assert_eq!(transport.seek, transport.mixer);
        }
    }

    #[test]
    fn time_format_is_stable() {
        assert_eq!(format_time(Microseconds::new(-1)), "0:00");
        assert_eq!(format_time(Microseconds::new(65_999_999)), "1:05");
    }
}
