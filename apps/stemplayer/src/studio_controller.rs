use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use stemplayer_domain::{
    CommandId, DisplayName, ErrorCode, GenerationId, PlaybackPosition, PlayerCommand,
    PlayerController, PlayerError, PlayerSnapshot, SnapshotPublisher, SnapshotSubscription,
    SourceFrame, SourceLocator, TransportState,
};
use stemplayer_engine::{PlaybackEngine, PlaybackSession};
use stemplayer_format::StudioBundle;
use stemplayer_output::{DeviceOutput, PostMixResampler, StereoFrame};
use stemplayer_timeline::{frames_to_microseconds, microseconds_to_frames};

const COMMAND_CAPACITY: usize = 32;
const OUTPUT_CAPACITY_FRAMES: usize = 8_192;
const RENDER_FRAMES: usize = 2_048;
const ACTIVE_TICK: Duration = Duration::from_millis(25);
const IDLE_TICK: Duration = Duration::from_secs(60);
const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);
const CLEAR_TIMEOUT: Duration = Duration::from_millis(500);

struct QueuedCommand {
    command: PlayerCommand,
    epoch: u64,
}

struct Shared {
    snapshot: RwLock<PlayerSnapshot>,
    subscribers: Mutex<Vec<SnapshotPublisher>>,
    cancel: Arc<AtomicBool>,
    epoch: AtomicU64,
    shutdown: AtomicBool,
}

pub struct StudioController {
    sender: SyncSender<QueuedCommand>,
    shared: Arc<Shared>,
    next_id: AtomicU64,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl StudioController {
    pub fn new() -> Arc<Self> {
        let (sender, receiver) = sync_channel(COMMAND_CAPACITY);
        let shared = Arc::new(Shared {
            snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
            subscribers: Mutex::new(Vec::new()),
            cancel: Arc::new(AtomicBool::new(false)),
            epoch: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
        });
        let worker_shared = Arc::clone(&shared);
        let worker = thread::spawn(move || worker_loop(receiver, worker_shared));
        Arc::new(Self {
            sender,
            shared,
            next_id: AtomicU64::new(1),
            worker: Mutex::new(Some(worker)),
        })
    }

    pub fn shutdown_and_join(&self) {
        self.signal_shutdown();
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = worker.join();
        }
    }

    fn signal_shutdown(&self) {
        self.shared.shutdown.store(true, Ordering::Release);
        self.shared.cancel.store(true, Ordering::Release);
        let _ = self.sender.try_send(QueuedCommand {
            command: PlayerCommand::Shutdown,
            epoch: self.shared.epoch.load(Ordering::Acquire),
        });
    }

    fn next_id(&self) -> Result<CommandId, PlayerError> {
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
}

impl PlayerController for StudioController {
    fn submit(&self, command: PlayerCommand) -> Result<CommandId, PlayerError> {
        let id = self.next_id()?;
        if command == PlayerCommand::Shutdown {
            self.signal_shutdown();
            return Ok(id);
        }
        let invalidates = matches!(
            command,
            PlayerCommand::Open(_)
                | PlayerCommand::Unload
                | PlayerCommand::Stop
                | PlayerCommand::SeekFrame(_)
                | PlayerCommand::SeekTime(_)
        );
        let epoch = if invalidates {
            self.shared.cancel.store(true, Ordering::Release);
            self.shared.epoch.fetch_add(1, Ordering::AcqRel) + 1
        } else {
            self.shared.epoch.load(Ordering::Acquire)
        };
        self.sender
            .try_send(QueuedCommand { command, epoch })
            .map_err(|error| match error {
                TrySendError::Full(_) => {
                    PlayerError::new(ErrorCode::CommandQueueFull, "Command queue is full")
                }
                TrySendError::Disconnected(_) => {
                    PlayerError::new(ErrorCode::PlaybackSessionClosed, "Controller is closed")
                }
            })?;
        Ok(id)
    }

    fn latest_snapshot(&self) -> PlayerSnapshot {
        self.shared
            .snapshot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn subscribe(&self) -> SnapshotSubscription {
        let (publisher, subscription) = SnapshotSubscription::coalescing();
        self.shared
            .subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(publisher);
        subscription
    }
}

struct WorkerState {
    engine: Option<PlaybackEngine>,
    output: Option<DeviceOutput>,
    resampler: Option<PostMixResampler>,
    selected_device: Option<String>,
    generation: GenerationId,
    last_output_underflows: u64,
    last_snapshot: Instant,
}

impl WorkerState {
    fn new() -> Self {
        Self {
            engine: None,
            output: None,
            resampler: None,
            selected_device: None,
            generation: GenerationId::INITIAL,
            last_output_underflows: 0,
            last_snapshot: Instant::now(),
        }
    }

    fn handle(&mut self, command: PlayerCommand, shared: &Shared) -> Result<(), PlayerError> {
        match command {
            PlayerCommand::Open(SourceLocator::LocalPath(path)) => self.open(path.as_str(), shared),
            PlayerCommand::Open(SourceLocator::HlsUrl(_)) => Err(PlayerError::new(
                ErrorCode::FeatureUnsupported,
                "HLS playback is not available in Phase 6",
            )),
            PlayerCommand::Unload => {
                self.close_output()?;
                self.engine = None;
                self.generation = self.generation.next()?;
                publish(
                    shared,
                    PlayerSnapshot::new(
                        TransportState::Empty,
                        self.generation,
                        None,
                        None,
                        vec![],
                    )?,
                );
                Ok(())
            }
            PlayerCommand::Play => {
                self.ensure_output()?;
                let replaying = self.engine_mut()?.state() == TransportState::Ended;
                self.engine_mut()?.play()?;
                if replaying {
                    self.reset_resampler(false)?;
                }
                self.pump()?;
                self.output_mut()?.play()?;
                self.publish(shared)
            }
            PlayerCommand::Pause => {
                self.engine_mut()?.pause()?;
                if let Some(output) = self.output.as_mut() {
                    output.pause()?;
                }
                self.publish(shared)
            }
            PlayerCommand::Stop => {
                self.clear_output()?;
                self.engine_mut()?.stop()?;
                self.reset_resampler(false)?;
                self.publish(shared)
            }
            PlayerCommand::SeekFrame(frame) => self.seek(frame, shared),
            PlayerCommand::SeekTime(time) => {
                let rate = self
                    .engine
                    .as_ref()
                    .ok_or_else(not_ready)?
                    .snapshot()?
                    .source()
                    .ok_or_else(not_ready)?
                    .sample_rate;
                self.seek(microseconds_to_frames(time, rate)?, shared)
            }
            PlayerCommand::SetGain { role, gain_db } => {
                self.engine_mut()?.set_gain(role, gain_db);
                self.publish(shared)
            }
            PlayerCommand::SetMute { role, muted } => {
                self.engine_mut()?.set_mute(role, muted);
                self.publish(shared)
            }
            PlayerCommand::SetSolo { role, solo } => {
                self.engine_mut()?.set_solo(role, solo);
                self.publish(shared)
            }
            PlayerCommand::ResetMix => {
                self.engine_mut()?.reset_mix();
                self.publish(shared)
            }
            PlayerCommand::SetOutputPreference(preference) => {
                self.engine_mut()?.set_output_preference(preference)?;
                self.publish(shared)
            }
            PlayerCommand::SelectDevice(device) => {
                self.select_device(device.map(|value| value.as_str().to_owned()), shared)
            }
            PlayerCommand::Cache(_) | PlayerCommand::Cancel(_) => Ok(()),
            PlayerCommand::Shutdown => Ok(()),
        }
    }

    fn open(&mut self, path: &str, shared: &Shared) -> Result<(), PlayerError> {
        self.close_output()?;
        self.engine = None;
        self.generation = self.generation.next()?;
        publish(
            shared,
            PlayerSnapshot::new(
                TransportState::Inspecting,
                self.generation,
                None,
                None,
                vec![],
            )?,
        );
        let path = std::path::Path::new(path);
        let is_manifest = path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("manifest.json"));
        let session = if path.is_file() && !is_manifest {
            PlaybackSession::open_compressed(path, self.generation, &shared.cancel)?
        } else {
            let group = stemplayer_platform::select_local_source(path)?;
            let bundle = StudioBundle::inspect(group, false, &shared.cancel)?;
            PlaybackSession::new(bundle, self.generation)?
        };
        self.engine = Some(PlaybackEngine::with_cancellation(
            session,
            Arc::clone(&shared.cancel),
        )?);
        self.publish(shared)
    }

    fn seek(&mut self, frame: SourceFrame, shared: &Shared) -> Result<(), PlayerError> {
        let resume = matches!(
            self.engine.as_ref().ok_or_else(not_ready)?.state(),
            TransportState::Playing | TransportState::Priming
        );
        self.clear_output()?;
        self.engine_mut()?.seek(frame)?;
        self.reset_resampler(false)?;
        if resume
            && self
                .engine
                .as_ref()
                .is_some_and(|engine| !engine.source_exhausted())
        {
            self.ensure_output()?;
            self.pump()?;
            self.output_mut()?.play()?;
        }
        self.publish(shared)
    }

    fn ensure_output(&mut self) -> Result<(), PlayerError> {
        if self.output.is_none() {
            let engine = self.engine.as_ref().ok_or_else(not_ready)?;
            let source_rate = engine.sample_rate().get();
            let remaining = engine.remaining_source_frames();
            let output =
                DeviceOutput::open(self.selected_device.as_deref(), OUTPUT_CAPACITY_FRAMES)?;
            let output_rate = output.negotiated().sample_rate;
            let input_chunk = resampler_input_frames(source_rate, output_rate);
            let resampler =
                PostMixResampler::new(source_rate, output_rate, input_chunk, remaining)?;
            if resampler.maximum_output_frames() > OUTPUT_CAPACITY_FRAMES {
                return Err(PlayerError::new(
                    ErrorCode::FeatureUnsupported,
                    "The device sample rate exceeds the bounded output queue",
                ));
            }
            self.resampler = Some(resampler);
            self.output = Some(output);
            self.last_output_underflows = 0;
        }
        Ok(())
    }

    fn clear_output(&mut self) -> Result<(), PlayerError> {
        if let Some(output) = self.output.as_mut() {
            output.clear(CLEAR_TIMEOUT)?;
        }
        Ok(())
    }

    fn close_output(&mut self) -> Result<(), PlayerError> {
        self.resampler = None;
        if let Some(mut output) = self.output.take() {
            output.pause()?;
        }
        Ok(())
    }

    fn engine_mut(&mut self) -> Result<&mut PlaybackEngine, PlayerError> {
        self.engine.as_mut().ok_or_else(not_ready)
    }

    fn output_mut(&mut self) -> Result<&mut DeviceOutput, PlayerError> {
        self.output.as_mut().ok_or_else(not_ready)
    }

    fn pump(&mut self) -> Result<(), PlayerError> {
        let Some(engine) = self.engine.as_mut() else {
            return Ok(());
        };
        let Some(output) = self.output.as_mut() else {
            return Ok(());
        };
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(());
        };
        if engine.state() != TransportState::Playing {
            return Ok(());
        }
        engine.refill()?;
        let underflows = output.metrics().underflows;
        let recovering = underflows > self.last_output_underflows && !engine.source_exhausted();
        self.last_output_underflows = underflows;
        if recovering {
            output.pause()?;
        }
        let mut rendered = [StereoFrame::default(); RENDER_FRAMES];
        while output.available_frames() >= resampler.maximum_output_frames()
            && !resampler.finished()
        {
            let needed = resampler.input_frames_next();
            if needed > rendered.len() {
                return Err(PlayerError::new(
                    ErrorCode::FeatureUnsupported,
                    "The device sample-rate ratio exceeds the bounded render chunk",
                ));
            }
            let produced = if engine.source_exhausted() {
                0
            } else {
                engine.render(&mut rendered[..needed])?
            };
            let converted = resampler.process(&rendered[..produced], engine.source_exhausted())?;
            let converted_len = converted.len();
            if output.push(converted) != converted_len {
                return Err(PlayerError::new(
                    ErrorCode::Internal,
                    "Output queue accepted a partial render block",
                ));
            }
            if produced == 0 && converted_len == 0 && !resampler.finished() {
                return Err(PlayerError::new(
                    ErrorCode::Internal,
                    "Resampler made no progress while draining",
                ));
            }
        }
        if resampler.finished() {
            output.begin_drain();
        }
        if resampler.finished() && engine.finish_if_output_drained(output.queued_frames())? {
            output.pause()?;
        } else if recovering {
            output.play()?;
        }
        Ok(())
    }

    fn publish(&mut self, shared: &Shared) -> Result<(), PlayerError> {
        let Some(engine) = self.engine.as_ref() else {
            return Ok(());
        };
        let mut snapshot = engine.snapshot()?;
        if let Some(output) = self.output.as_ref() {
            let metrics = output.metrics();
            let queued = output.queued_frames() as u64;
            let mut details = snapshot.details().clone();
            details.device_name = Some(DisplayName::truncated(output.device_name()));
            details.device_queue_frames = queued;
            details.audio_stream_active = output.is_active();
            details.underrun_count = details.underrun_count.saturating_add(metrics.underflows);
            details.overflow_count = details.overflow_count.saturating_add(metrics.overflows);
            details.resampler = self.resampler.as_ref().map(PostMixResampler::snapshot);
            if let (Some(position), Some(source)) = (snapshot.position(), snapshot.source()) {
                let output_rate = u64::from(output.negotiated().sample_rate);
                let queued_source = (u128::from(queued) * u128::from(source.sample_rate.get()))
                    .div_ceil(u128::from(output_rate))
                    .min(i64::MAX as u128) as i64;
                let frame = SourceFrame::new(
                    position
                        .absolute_frame()
                        .get()
                        .saturating_sub(queued_source)
                        .max(source.start_frame.get()),
                );
                snapshot = snapshot.with_position(PlaybackPosition::new(
                    frame,
                    frames_to_microseconds(frame, source.sample_rate)?,
                ));
            }
            snapshot = snapshot.with_details(details);
        }
        self.generation = snapshot.generation();
        self.last_snapshot = Instant::now();
        publish(shared, snapshot);
        Ok(())
    }

    fn publish_if_due(&mut self, shared: &Shared) -> Result<(), PlayerError> {
        let state = self.engine.as_ref().map(PlaybackEngine::state);
        let published = shared
            .snapshot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .transport_state();
        if state == Some(TransportState::Ended) && published != TransportState::Ended
            || (state.is_some() && self.last_snapshot.elapsed() >= SNAPSHOT_INTERVAL)
        {
            self.publish(shared)?;
        }
        Ok(())
    }

    fn reset_resampler(&mut self, discontinuity: bool) -> Result<(), PlayerError> {
        let remaining = self
            .engine
            .as_ref()
            .ok_or_else(not_ready)?
            .remaining_source_frames();
        if let Some(resampler) = self.resampler.as_mut() {
            resampler.reset(remaining);
            if discontinuity {
                resampler.record_discontinuity();
            }
        }
        Ok(())
    }

    fn select_device(
        &mut self,
        device: Option<String>,
        shared: &Shared,
    ) -> Result<(), PlayerError> {
        let resume = self
            .engine
            .as_ref()
            .is_some_and(|engine| engine.state() == TransportState::Playing);
        let audible = self.audible_position();
        self.close_output()?;
        self.selected_device = device;
        if let Some(frame) = audible {
            self.engine_mut()?.seek(frame)?;
        }
        if self.engine.is_some() {
            self.ensure_output()?;
            self.reset_resampler(true)?;
            if resume {
                self.pump()?;
                self.output_mut()?.play()?;
            }
        }
        self.publish(shared)
    }

    fn audible_position(&self) -> Option<SourceFrame> {
        let engine = self.engine.as_ref()?;
        let output = self.output.as_ref()?;
        let queued = output.queued_frames() as u128;
        let source_rate = u128::from(engine.sample_rate().get());
        let output_rate = u128::from(output.negotiated().sample_rate);
        let queued_source = (queued * source_rate)
            .div_ceil(output_rate)
            .min(i64::MAX as u128);
        Some(SourceFrame::new(
            engine
                .position()
                .get()
                .saturating_sub(queued_source as i64)
                .max(0),
        ))
    }
}

fn resampler_input_frames(source_rate: u32, output_rate: u32) -> usize {
    ((RENDER_FRAMES as u128 * u128::from(source_rate)) / u128::from(output_rate))
        .clamp(1, RENDER_FRAMES as u128) as usize
}

fn worker_loop(receiver: Receiver<QueuedCommand>, shared: Arc<Shared>) {
    let mut state = WorkerState::new();
    while !shared.shutdown.load(Ordering::Acquire) {
        let active = state
            .engine
            .as_ref()
            .is_some_and(|engine| engine.state() == TransportState::Playing);
        match receiver.recv_timeout(if active { ACTIVE_TICK } else { IDLE_TICK }) {
            Ok(queued) => {
                if queued.epoch < shared.epoch.load(Ordering::Acquire) {
                    continue;
                }
                shared.cancel.store(false, Ordering::Release);
                if let Err(error) = state.handle(queued.command, &shared)
                    && !(error.code == ErrorCode::BundleAborted
                        && shared.cancel.load(Ordering::Acquire))
                {
                    publish_error(&shared, &state, error);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if let Err(error) = state.pump().and_then(|()| state.publish_if_due(&shared)) {
            publish_error(&shared, &state, error);
        }
    }
    let _ = state.close_output();
    publish(
        &shared,
        PlayerSnapshot::new(TransportState::Closed, state.generation, None, None, vec![])
            .unwrap_or_else(|_| PlayerSnapshot::empty(TransportState::Closed)),
    );
}

fn publish_error(shared: &Shared, state: &WorkerState, error: PlayerError) {
    let current = state
        .engine
        .as_ref()
        .and_then(|engine| engine.snapshot().ok());
    let snapshot = PlayerSnapshot::new(
        TransportState::Error,
        current
            .as_ref()
            .map_or(state.generation, PlayerSnapshot::generation),
        current.as_ref().and_then(PlayerSnapshot::position),
        current.as_ref().and_then(PlayerSnapshot::source).cloned(),
        vec![],
    )
    .map(|snapshot| {
        let mut details = current
            .as_ref()
            .map_or_else(Default::default, |value| value.details().clone());
        details.error = Some(error);
        snapshot.with_details(details)
    })
    .unwrap_or_else(|_| PlayerSnapshot::empty(TransportState::Error));
    publish(shared, snapshot);
}

fn publish(shared: &Shared, snapshot: PlayerSnapshot) {
    *shared
        .snapshot
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot.clone();
    shared
        .subscribers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|publisher| publisher.publish(snapshot.clone()));
}

fn not_ready() -> PlayerError {
    PlayerError::new(ErrorCode::PlaybackNotReady, "No local source is ready")
}

#[cfg(test)]
mod tests;
