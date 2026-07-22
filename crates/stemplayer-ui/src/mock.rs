use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Duration;
use stemplayer_domain::{
    CacheCommand, ChannelCount, CommandId, DisplayName, ErrorCode, GenerationId, Microseconds,
    MixerSnapshot, PlaybackPosition, PlayerCommand, PlayerController, PlayerError, PlayerSnapshot,
    SnapshotDetails, SnapshotPublisher, SnapshotSubscription, SourceFrame, SourceKind,
    SourceLocator, SourceMetadata, StemRole, StemSnapshot, TransportState, WaveformState,
};
use stemplayer_mixer::MixerState;

const SAMPLE_RATE: u32 = 48_000;
const DURATION_SECONDS: i64 = 600;
const END_FRAME: i64 = SAMPLE_RATE as i64 * DURATION_SECONDS;

struct MockState {
    transport: TransportState,
    generation: GenerationId,
    frame: SourceFrame,
    source: Option<SourceMetadata>,
    mixer: MixerState,
    has_master: bool,
    device: Option<DisplayName>,
    buffering: bool,
    waveform: WaveformState,
    error: Option<PlayerError>,
}

impl Default for MockState {
    fn default() -> Self {
        Self {
            transport: TransportState::Empty,
            generation: GenerationId::INITIAL,
            frame: SourceFrame::new(0),
            source: None,
            mixer: MixerState::default(),
            has_master: true,
            device: Some(DisplayName::truncated("System default")),
            buffering: false,
            waveform: WaveformState::Unavailable,
            error: None,
        }
    }
}

pub struct MockController {
    state: Mutex<MockState>,
    snapshot: RwLock<PlayerSnapshot>,
    subscribers: Mutex<Vec<SnapshotPublisher>>,
    next_id: AtomicU64,
    stalled: AtomicBool,
}

impl Default for MockController {
    fn default() -> Self {
        Self::new()
    }
}

impl MockController {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(MockState::default()),
            snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
            subscribers: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            stalled: AtomicBool::new(false),
        }
    }

    pub fn set_stalled(&self, stalled: bool) {
        self.stalled.store(stalled, Ordering::Release);
    }

    pub fn set_master_present(&self, present: bool) {
        let mut state = self.lock_state();
        state.has_master = present;
        if let Some(source) = state.source.as_mut() {
            source.has_master = present;
            source.channel_count = ChannelCount::new(if present { 10 } else { 8 }).unwrap();
        }
        self.publish_locked(&state);
    }

    pub fn set_buffering(&self, buffering: bool) {
        let mut state = self.lock_state();
        state.buffering = buffering;
        self.publish_locked(&state);
    }

    pub fn set_waveform(&self, waveform: WaveformState) {
        let mut state = self.lock_state();
        state.waveform = waveform;
        self.publish_locked(&state);
    }

    pub fn fail(&self, error: PlayerError) {
        let mut state = self.lock_state();
        state.transport = TransportState::Error;
        state.error = Some(error);
        self.publish_locked(&state);
    }

    pub fn advance(&self, elapsed: Duration) {
        let mut state = self.lock_state();
        if state.transport == TransportState::Inspecting {
            state.transport = TransportState::Ready;
            state.waveform = WaveformState::Analyzing;
        } else if state.transport == TransportState::Playing {
            let frames = elapsed
                .as_nanos()
                .saturating_mul(u128::from(SAMPLE_RATE))
                .saturating_div(1_000_000_000)
                .min(i64::MAX as u128) as i64;
            let next = state.frame.get().saturating_add(frames);
            if next >= END_FRAME {
                state.frame = SourceFrame::new(END_FRAME);
                state.transport = TransportState::Ended;
            } else {
                state.frame = SourceFrame::new(next);
            }
        }
        self.publish_locked(&state);
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, MockState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn next_command_id(&self) -> Result<CommandId, PlayerError> {
        self.next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map(CommandId::new)
            .map_err(|_| {
                PlayerError::new(
                    ErrorCode::ValueOutOfRange,
                    "Command identifier is exhausted",
                )
            })
    }

    fn publish_locked(&self, state: &MockState) {
        let snapshot = snapshot(state);
        *self
            .snapshot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot.clone();
        self.subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|publisher| publisher.publish(snapshot.clone()));
    }

    fn apply(&self, command: PlayerCommand) -> Result<(), PlayerError> {
        let mut state = self.lock_state();
        match command {
            PlayerCommand::Open(locator) => {
                state.generation = state.generation.next()?;
                state.transport = TransportState::Inspecting;
                state.frame = SourceFrame::new(0);
                state.mixer = MixerState::default();
                state.error = None;
                state.waveform = WaveformState::Unavailable;
                state.source = Some(mock_source(locator, state.has_master));
            }
            PlayerCommand::Unload => *state = MockState::default(),
            PlayerCommand::Play => match state.transport {
                TransportState::Ready | TransportState::Paused | TransportState::Ended => {
                    if state.transport == TransportState::Ended {
                        state.frame = SourceFrame::new(0);
                    }
                    state.transport = TransportState::Playing;
                }
                _ => return Err(invalid_state()),
            },
            PlayerCommand::Pause if state.transport == TransportState::Playing => {
                state.transport = TransportState::Paused;
            }
            PlayerCommand::Pause => return Err(invalid_state()),
            PlayerCommand::Stop if state.source.is_some() => {
                state.transport = TransportState::Ready;
                state.frame = SourceFrame::new(0);
            }
            PlayerCommand::Stop => return Err(invalid_state()),
            PlayerCommand::SeekFrame(frame) if state.source.is_some() => {
                state.generation = state.generation.next()?;
                state.frame = SourceFrame::new(frame.get().clamp(0, END_FRAME));
                state.transport = if state.frame.get() == END_FRAME {
                    TransportState::Ended
                } else if state.transport == TransportState::Playing {
                    TransportState::Playing
                } else {
                    TransportState::Paused
                };
            }
            PlayerCommand::SeekTime(time) if state.source.is_some() => {
                state.generation = state.generation.next()?;
                let frame = time
                    .get()
                    .saturating_mul(i64::from(SAMPLE_RATE))
                    .saturating_div(1_000_000)
                    .clamp(0, END_FRAME);
                state.frame = SourceFrame::new(frame);
                state.transport = if frame == END_FRAME {
                    TransportState::Ended
                } else {
                    TransportState::Paused
                };
            }
            PlayerCommand::SeekFrame(_) | PlayerCommand::SeekTime(_) => return Err(invalid_state()),
            PlayerCommand::SetGain { role, gain_db } => {
                state.mixer = state.mixer.with_gain(role, gain_db);
            }
            PlayerCommand::SetMute { role, muted } => {
                state.mixer = state.mixer.with_mute(role, muted);
            }
            PlayerCommand::SetSolo { role, solo } => {
                state.mixer = state.mixer.with_solo(role, solo);
            }
            PlayerCommand::ResetMix => state.mixer = state.mixer.reset(),
            PlayerCommand::SetOutputPreference(preference) => {
                state.mixer = state.mixer.with_preference(preference, state.has_master)?;
            }
            PlayerCommand::SelectDevice(device) => {
                state.device = device.map(|value| DisplayName::truncated(value.as_str()));
            }
            PlayerCommand::Cache(
                CacheCommand::Clear | CacheCommand::Enable | CacheCommand::Disable,
            )
            | PlayerCommand::Cancel(_) => {}
            PlayerCommand::Shutdown => state.transport = TransportState::Closed,
        }
        self.publish_locked(&state);
        Ok(())
    }
}

impl PlayerController for MockController {
    fn submit(&self, command: PlayerCommand) -> Result<CommandId, PlayerError> {
        let id = self.next_command_id()?;
        if matches!(command, PlayerCommand::Shutdown) || !self.stalled.load(Ordering::Acquire) {
            self.apply(command)?;
        }
        Ok(id)
    }

    fn latest_snapshot(&self) -> PlayerSnapshot {
        self.snapshot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn subscribe(&self) -> SnapshotSubscription {
        let (publisher, subscription) = SnapshotSubscription::coalescing();
        self.subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(publisher);
        subscription
    }
}

fn snapshot(state: &MockState) -> PlayerSnapshot {
    let position = state.source.as_ref().map(|_| {
        PlaybackPosition::new(
            state.frame,
            Microseconds::new(state.frame.get().saturating_mul(1_000_000) / i64::from(SAMPLE_RATE)),
        )
    });
    let stems = StemRole::ALL.map(|role| {
        let control = state.mixer.control(role);
        StemSnapshot {
            role,
            gain_db: control.gain_db,
            muted: control.muted,
            solo: control.solo,
        }
    });
    let details = SnapshotDetails {
        mixer: MixerSnapshot {
            stems,
            output_mode: state
                .mixer
                .effective_output(state.has_master)
                .unwrap_or(stemplayer_domain::OutputMode::StemMix),
        },
        device_name: state.device.clone(),
        buffering: state.buffering,
        waveform: state.waveform,
        error: state.error.clone(),
    };
    PlayerSnapshot::new(
        state.transport,
        state.generation,
        position,
        state.source.clone(),
        vec![],
    )
    .unwrap()
    .with_details(details)
}

fn mock_source(locator: SourceLocator, has_master: bool) -> SourceMetadata {
    let value = match locator {
        SourceLocator::LocalPath(value) => value.to_string(),
        SourceLocator::HlsUrl(value) => value.to_string(),
    };
    let display = value.rsplit(['/', '\\']).next().unwrap_or("Demo stemset");
    SourceMetadata {
        display_name: DisplayName::truncated(display),
        kind: SourceKind::StudioWavBundle,
        has_master,
        sample_rate: stemplayer_domain::SampleRate::new(SAMPLE_RATE).unwrap(),
        channel_count: ChannelCount::new(if has_master { 10 } else { 8 }).unwrap(),
        start_frame: SourceFrame::new(0),
        end_frame_exclusive: SourceFrame::new(END_FRAME),
    }
}

fn invalid_state() -> PlayerError {
    PlayerError::new(
        ErrorCode::TransportStateInvalid,
        "Command is not valid in the current mock transport state",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use stemplayer_domain::{BoundedText, DeviceId, PlayerController};

    fn open(controller: &MockController) {
        controller
            .submit(PlayerCommand::Open(SourceLocator::LocalPath(
                BoundedText::new("demo/track.stem.m4a").unwrap(),
            )))
            .unwrap();
        assert_eq!(
            controller.latest_snapshot().transport_state(),
            TransportState::Inspecting
        );
        controller.advance(Duration::ZERO);
        assert_eq!(
            controller.latest_snapshot().transport_state(),
            TransportState::Ready
        );
    }

    #[test]
    fn virtual_clock_covers_load_play_seek_and_end() {
        let controller = MockController::new();
        open(&controller);
        controller.submit(PlayerCommand::Play).unwrap();
        controller.advance(Duration::from_secs(3));
        assert_eq!(
            controller
                .latest_snapshot()
                .position()
                .unwrap()
                .absolute_frame(),
            SourceFrame::new(i64::from(SAMPLE_RATE) * 3)
        );
        controller
            .submit(PlayerCommand::SeekFrame(SourceFrame::new(END_FRAME)))
            .unwrap();
        assert_eq!(
            controller.latest_snapshot().transport_state(),
            TransportState::Ended
        );
    }

    #[test]
    fn shutdown_bypasses_stalled_operation() {
        for command in [
            PlayerCommand::Open(SourceLocator::LocalPath(
                BoundedText::new("stalled.stem.m4a").unwrap(),
            )),
            PlayerCommand::SeekFrame(SourceFrame::new(24_000)),
            PlayerCommand::Play,
        ] {
            let controller = MockController::new();
            controller.set_stalled(true);
            controller.submit(command).unwrap();
            assert_eq!(
                controller.latest_snapshot().transport_state(),
                TransportState::Empty
            );
            controller.submit(PlayerCommand::Shutdown).unwrap();
            assert_eq!(
                controller.latest_snapshot().transport_state(),
                TransportState::Closed
            );
        }
    }

    #[test]
    fn master_device_buffering_waveform_and_error_are_exposed() {
        let controller = MockController::new();
        open(&controller);
        controller.set_master_present(false);
        controller.set_buffering(true);
        controller.set_waveform(WaveformState::Ready);
        controller
            .submit(PlayerCommand::SelectDevice(Some(
                DeviceId::new("device-1").unwrap(),
            )))
            .unwrap();
        let snapshot = controller.latest_snapshot();
        assert!(snapshot.details().buffering);
        assert_eq!(snapshot.details().waveform, WaveformState::Ready);
        assert!(!snapshot.source().unwrap().has_master);
        controller.fail(PlayerError::new(ErrorCode::Internal, "mock failure"));
        assert_eq!(
            controller.latest_snapshot().transport_state(),
            TransportState::Error
        );
    }
}
