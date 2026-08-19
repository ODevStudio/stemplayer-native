use stemplayer_domain::{
    ErrorCode, GainDb, Microseconds, OutputMode, OutputPreference, PlayerError, PlayerSnapshot,
    SourceKind, StemRole, TransportState, WaveformState,
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
    pub gain_position: f32,
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
    pub output_mode: String,
    pub output_preference: i32,
    pub has_master: bool,
    pub buffering_banner: Option<&'static str>,
    pub error_banner: Option<String>,
    pub error_action: String,
    pub error_code: String,
    pub error_status: String,
    pub error_details: String,
    pub file_name: String,
    pub source_summary: String,
    pub device_name: String,
    pub output_rate_text: String,
    pub waveform_text: String,
    pub diagnostics_count: usize,
    pub diagnostics_text: String,
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
        let waveform_cache = format!("{:?}", details.waveform_cache).to_ascii_lowercase();
        let waveform_text = match details.waveform {
            WaveformState::Unavailable => "Waveform unavailable".to_owned(),
            WaveformState::Analyzing => format!(
                "Analyzing waveform: {}%{}",
                details.waveform_progress_percent,
                if details.waveform_throttled {
                    " (paused for playback)"
                } else {
                    ""
                }
            ),
            WaveformState::Ready => "Waveform ready".to_owned(),
        };
        let source_diagnostics = details.network_diagnostics.as_ref().map_or_else(
            || {
                details.source_diagnostics.as_ref().map_or_else(
                    || {
                        format!(
                            "{} | {}",
                            state_text(snapshot.transport_state()),
                            output_mode(
                                details.mixer.output_preference,
                                details.mixer.output_mode,
                                source.map(|value| value.kind),
                            )
                        )
                    },
                    |diagnostics| {
                        format!(
                            "{} | {} | metadata {} B | media {} B | queue {} | seek {} packets, {} discarded frames, {} us",
                            diagnostics.container,
                            diagnostics.tracks,
                            diagnostics.metadata_read_bytes,
                            diagnostics.media_read_bytes,
                            diagnostics.maximum_pending_blocks,
                            diagnostics.seek_packet_count,
                            diagnostics.seek_discarded_frames,
                            diagnostics.seek_latency_us
                        )
                    },
                )
            },
            |diagnostics| {
                format!(
                    "{} | group {:?} | fetches {} ({} failed) | ready {} us | buffering {} | cache {}/{} B | preroll {} frames | offset {} frames",
                    diagnostics.redacted_source,
                    diagnostics.current_group,
                    diagnostics.fetch_count,
                    diagnostics.fetch_failures,
                    diagnostics.group_ready_us,
                    diagnostics.buffering_count,
                    diagnostics.cache_bytes,
                    diagnostics.peak_cache_bytes,
                    diagnostics.discarded_preroll_frames,
                    diagnostics.inter_stem_offset_frames
                )
            },
        );
        let diagnostics_text = format!(
            "{source_diagnostics} | waveform {}% | cache {waveform_cache}{}",
            details.waveform_progress_percent,
            if details.waveform_throttled {
                " | analysis paused for playback"
            } else {
                ""
            }
        );
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
                gain_position: gain_position_from_db(stem.gain_db),
                gain_text: gain_percent_text(stem.gain_db),
                muted: stem.muted,
                solo: stem.solo,
            }),
            output_mode: output_mode(
                details.mixer.output_preference,
                details.mixer.output_mode,
                source.map(|value| value.kind),
            ),
            output_preference: match details.mixer.output_preference {
                OutputPreference::Automatic => 0,
                OutputPreference::MasterReference => 1,
                OutputPreference::StemMix => 2,
            },
            has_master: source.is_some_and(|value| value.has_master),
            buffering_banner: details.buffering.then_some("Buffering"),
            error_banner: details
                .error
                .as_ref()
                .map(|error| error.summary.as_str().to_owned()),
            error_action: details
                .error
                .as_ref()
                .and_then(|error| error.user_action.as_ref())
                .map_or_else(
                    || "Try again or choose another source".to_owned(),
                    ToString::to_string,
                ),
            error_code: details
                .error
                .as_ref()
                .map_or_else(String::new, |error| error.code.as_str().to_owned()),
            error_status: details.error.as_ref().map_or_else(String::new, |_| {
                if source.is_some() {
                    "Playback stopped; source remains loaded".to_owned()
                } else {
                    "Playback stopped; no source is loaded".to_owned()
                }
            }),
            error_details: details.error.as_ref().map_or_else(String::new, |error| {
                error
                    .details
                    .iter()
                    .map(|detail| format!("{}={}", detail.key, detail.value))
                    .collect::<Vec<_>>()
                    .join(" | ")
            }),
            file_name: source
                .map(|value| value.display_name.as_str().to_owned())
                .unwrap_or_else(|| "No source loaded".into()),
            source_summary: source.map(source_summary).unwrap_or_default(),
            device_name: details
                .device_name
                .as_ref()
                .map(|value| value.as_str().to_owned())
                .unwrap_or_else(|| "System default".into()),
            output_rate_text: details.resampler.map_or_else(
                || {
                    source.map_or_else(
                        || "Output rate unavailable".to_owned(),
                        |source| format!("Output {} Hz", source.sample_rate.get()),
                    )
                },
                |resampler| format!("Output {} Hz", resampler.output_rate),
            ),
            waveform_text,
            diagnostics_count: snapshot.diagnostics().len(),
            diagnostics_text,
        }
    }
}

pub fn gain_position_from_db(gain: GainDb) -> f32 {
    let gain = gain.get();
    if gain <= 0.0 {
        800.0 * (1.0 - (-gain / 60.0).sqrt())
    } else {
        800.0 + gain / 6.0 * 200.0
    }
}

pub fn gain_db_from_position(position: f32) -> Result<GainDb, PlayerError> {
    if !position.is_finite() || !(0.0..=1_000.0).contains(&position) {
        return Err(PlayerError::new(
            ErrorCode::MixerGainInvalid,
            "Gain position must be between 0 and 1000",
        ));
    }
    let gain = if position <= 800.0 {
        -60.0 * (1.0 - position / 800.0).powi(2)
    } else {
        (position - 800.0) / 200.0 * 6.0
    };
    GainDb::new((gain * 10.0).round() / 10.0)
}

fn gain_percent_text(gain: GainDb) -> String {
    if gain == GainDb::SILENCE {
        return "0%".to_owned();
    }
    format!("{}%", (gain.linear() * 100.0).round().max(1.0) as u16)
}

fn output_mode(
    preference: OutputPreference,
    mode: OutputMode,
    source_kind: Option<SourceKind>,
) -> String {
    let selection = match preference {
        OutputPreference::Automatic => "Automatic",
        OutputPreference::MasterReference | OutputPreference::StemMix => "Manual",
    };
    let mode = match (source_kind, mode) {
        (Some(SourceKind::LocalAudioFile), OutputMode::StemMix) => "Stereo mix",
        (_, OutputMode::MasterReference) => "Master reference",
        (_, OutputMode::StemMix) => "Stem mix",
    };
    format!("{selection}: {mode}")
}

fn format_time(value: Microseconds) -> String {
    let seconds = value.get().max(0) / 1_000_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn source_summary(source: &stemplayer_domain::SourceMetadata) -> String {
    let kind = match source.kind {
        SourceKind::StudioWavBundle => "Studio WAV",
        SourceKind::LocalStemContainer => "Stem container",
        SourceKind::LocalAudioFile => "Audio file",
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

    #[test]
    fn ordinary_audio_presents_its_source_channels_and_stereo_output() {
        let snapshot = PlayerSnapshot::new(
            TransportState::Ready,
            stemplayer_domain::GenerationId::INITIAL,
            None,
            Some(stemplayer_domain::SourceMetadata {
                display_name: stemplayer_domain::DisplayName::new("demo.mp3").unwrap(),
                kind: SourceKind::LocalAudioFile,
                has_master: false,
                sample_rate: stemplayer_domain::SampleRate::new(48_000).unwrap(),
                channel_count: stemplayer_domain::ChannelCount::new(2).unwrap(),
                start_frame: stemplayer_domain::SourceFrame::new(0),
                end_frame_exclusive: stemplayer_domain::SourceFrame::new(48_000),
            }),
            vec![],
        )
        .unwrap();
        let model = PresentationModel::from_snapshot(&snapshot);

        assert_eq!(model.source_summary, "Audio file | 48000 Hz | 2 channels");
        assert_eq!(model.output_mode, "Automatic: Stereo mix");
    }

    #[test]
    fn gain_scale_matches_the_web_player_and_reports_percent() {
        for (gain, position, text) in [
            (GainDb::SILENCE, 0.0, "0%"),
            (GainDb::new(-15.0).unwrap(), 400.0, "18%"),
            (GainDb::UNITY, 800.0, "100%"),
            (GainDb::new(6.0).unwrap(), 1_000.0, "200%"),
        ] {
            assert_eq!(gain_position_from_db(gain), position);
            assert_eq!(gain_db_from_position(position).unwrap(), gain);
            assert_eq!(gain_percent_text(gain), text);
        }
        assert!(gain_db_from_position(f32::NAN).is_err());
        assert!(gain_db_from_position(1_001.0).is_err());
    }

    #[test]
    fn error_presentation_includes_action_code_and_redacted_details() {
        let error = PlayerError::new(ErrorCode::ManifestInvalid, "Manifest is invalid")
            .with_user_action("Choose another Studio bundle")
            .with_details(vec![stemplayer_domain::ErrorDetail {
                key: stemplayer_domain::BoundedText::new("role").unwrap(),
                value: stemplayer_domain::BoundedText::new("vocals").unwrap(),
            }])
            .unwrap();
        let snapshot = PlayerSnapshot::empty(TransportState::Error).with_details(
            stemplayer_domain::SnapshotDetails {
                error: Some(error),
                ..stemplayer_domain::SnapshotDetails::default()
            },
        );
        let model = PresentationModel::from_snapshot(&snapshot);
        assert_eq!(model.error_code, "E_MANIFEST_INVALID");
        assert_eq!(model.error_action, "Choose another Studio bundle");
        assert_eq!(model.error_details, "role=vocals");
        assert_eq!(model.error_status, "Playback stopped; no source is loaded");
    }
}
