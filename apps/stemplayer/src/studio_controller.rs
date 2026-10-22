mod admission;
mod analysis;
mod cache_writer;
mod hls;
mod lifecycle;
mod local_source;
mod output;
mod preloader;
mod source_snapshot;

use analysis::AnalysisWork;
use cache_writer::CacheWriter;
use lifecycle::worker_loop;
#[cfg(test)]
use lifecycle::{publish_error, should_process_command, worker_timeout};
use local_source::{open_local_session, waveform_fingerprint};
use output::with_output_clipping;
use preloader::{PreparedSource, SourcePreloader};
pub use source_snapshot::SourceSnapshot;
#[cfg(not(feature = "development"))]
pub use source_snapshot::SourceSnapshotSubscription;
use source_snapshot::{SourceSnapshotPublisher, publish};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use stemplayer_domain::{
    CacheCommand, CommandId, DisplayName, ErrorCategory, ErrorCode, GenerationId, PlaybackPosition,
    PlayerCommand, PlayerController, PlayerError, PlayerSnapshot, Severity, SnapshotPublisher,
    SnapshotSubscription, SourceFrame, SourceLocator, TransportState, WaveformCacheState,
    WaveformState,
};
use stemplayer_engine::{PlaybackEngine, PlaybackSession};
use stemplayer_format::StudioBundle;
use stemplayer_output::{
    AudioClock, DeviceLifecycle, DeviceOutput, LifecycleAction, PostMixResampler, StereoFrame,
};
use stemplayer_timeline::{frames_to_microseconds, microseconds_to_frames};
use stemplayer_waveform::{
    CacheStatus, SourceEvidence, SourceFingerprint, WaveformCache, WaveformSnapshot,
};

const COMMAND_CAPACITY: usize = 32;
const OUTPUT_CAPACITY_FRAMES: usize = 8_192;
const RENDER_FRAMES: usize = 2_048;
const ACTIVE_TICK: Duration = Duration::from_millis(25);
const IDLE_TICK: Duration = Duration::from_secs(60);
const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(1);
const CLEAR_TIMEOUT: Duration = Duration::from_millis(500);
const OUTPUT_REOPEN_ATTEMPTS: u8 = 3;

enum WorkerCommand {
    Player(PlayerCommand),
    SourceRecovery {
        source: Option<SourceLocator>,
        play: bool,
    },
    PrepareSource {
        source: PathBuf,
        generation: GenerationId,
    },
}

struct QueuedCommand {
    command: WorkerCommand,
    epoch: u64,
    transport_revision: u64,
    cancelled: Arc<AtomicBool>,
}

struct Shared {
    snapshot: RwLock<PlayerSnapshot>,
    waveform: RwLock<Option<WaveformSnapshot>>,
    current_source: RwLock<Option<PathBuf>>,
    subscribers: Mutex<Vec<SnapshotPublisher>>,
    source_subscribers: Mutex<Vec<SourceSnapshotPublisher>>,
    command_admission: Mutex<()>,
    cancel: Mutex<Arc<AtomicBool>>,
    epoch: AtomicU64,
    transport_revision: AtomicU64,
    play_requested: AtomicBool,
    shutdown: AtomicBool,
}

pub struct StudioController {
    sender: SyncSender<QueuedCommand>,
    shared: Arc<Shared>,
    next_id: AtomicU64,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl StudioController {
    #[cfg(test)]
    pub fn new() -> Arc<Self> {
        Self::with_cache(None, false)
    }

    pub fn with_cache(cache: Option<Arc<WaveformCache>>, cache_enabled: bool) -> Arc<Self> {
        let (sender, receiver) = sync_channel(COMMAND_CAPACITY);
        let shared = Arc::new(Shared {
            snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
            waveform: RwLock::new(None),
            current_source: RwLock::new(None),
            subscribers: Mutex::new(Vec::new()),
            source_subscribers: Mutex::new(Vec::new()),
            command_admission: Mutex::new(()),
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            epoch: AtomicU64::new(0),
            transport_revision: AtomicU64::new(0),
            play_requested: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
        });
        let worker_shared = Arc::clone(&shared);
        let worker =
            thread::spawn(move || worker_loop(receiver, worker_shared, cache, cache_enabled));
        Arc::new(Self {
            sender,
            shared,
            next_id: AtomicU64::new(1),
            worker: Mutex::new(Some(worker)),
        })
    }

    pub fn latest_waveform(&self) -> Option<WaveformSnapshot> {
        self.shared
            .waveform
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    #[cfg(not(feature = "development"))]
    pub fn current_source_is(&self, path: &Path) -> bool {
        self.shared
            .current_source
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_deref()
            == Some(path)
    }

    #[cfg(any(test, not(feature = "development")))]
    pub fn play_requested(&self) -> bool {
        self.shared.play_requested.load(Ordering::Acquire)
    }

    pub fn prepare_source(
        &self,
        source: PathBuf,
        generation: GenerationId,
    ) -> Result<(), PlayerError> {
        self.enqueue(WorkerCommand::PrepareSource { source, generation }, false)
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
}

impl PlayerController for StudioController {
    fn submit(&self, command: PlayerCommand) -> Result<CommandId, PlayerError> {
        let id = self.next_id()?;
        if command == PlayerCommand::Shutdown {
            self.signal_shutdown();
            return Ok(id);
        }
        let invalidates = matches!(command, PlayerCommand::Open(_) | PlayerCommand::Unload);
        self.enqueue(WorkerCommand::Player(command), invalidates)?;
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
    source_epoch: u64,
    transport_revision: u64,
    source_cancelled: Arc<AtomicBool>,
    engine: Option<PlaybackEngine>,
    analysis: Option<AnalysisWork>,
    waveform_error: Option<PlayerError>,
    output_error: Option<PlayerError>,
    preloader: SourcePreloader,
    cache: Option<Arc<WaveformCache>>,
    cache_writer: Option<CacheWriter>,
    cache_enabled: bool,
    fingerprint: Option<SourceFingerprint>,
    hls: Option<Arc<stemplayer_network::HlsPlaybackSource>>,
    output: Option<DeviceOutput>,
    resampler: Option<PostMixResampler>,
    output_clock: Option<AudioClock>,
    output_origin: SourceFrame,
    paused_position: Option<SourceFrame>,
    output_drain_started: Option<Instant>,
    drain_resume: output::DrainResume,
    active_tick: Duration,
    output_lifecycle: DeviceLifecycle,
    output_reopen_at: Option<Instant>,
    selected_device: Option<String>,
    generation: GenerationId,
    current_source: Option<PathBuf>,
    last_output_underflows: u64,
    last_snapshot: Instant,
}

impl WorkerState {
    fn new(cache: Option<Arc<WaveformCache>>, cache_enabled: bool) -> Self {
        let cache_writer = cache
            .as_ref()
            .map(|cache| CacheWriter::new(Arc::clone(cache)));
        Self {
            source_epoch: 0,
            transport_revision: 0,
            source_cancelled: Arc::new(AtomicBool::new(false)),
            engine: None,
            analysis: None,
            waveform_error: None,
            output_error: None,
            preloader: SourcePreloader::default(),
            cache,
            cache_writer,
            cache_enabled,
            fingerprint: None,
            hls: None,
            output: None,
            resampler: None,
            output_clock: None,
            output_origin: SourceFrame::new(0),
            paused_position: None,
            output_drain_started: None,
            drain_resume: output::DrainResume::default(),
            active_tick: ACTIVE_TICK,
            output_lifecycle: DeviceLifecycle::new(OUTPUT_REOPEN_ATTEMPTS),
            output_reopen_at: None,
            selected_device: None,
            generation: GenerationId::INITIAL,
            current_source: None,
            last_output_underflows: 0,
            last_snapshot: Instant::now(),
        }
    }

    fn handle(&mut self, queued: QueuedCommand, shared: &Shared) -> Result<(), PlayerError> {
        self.transport_revision = queued.transport_revision;
        if matches!(
            &queued.command,
            WorkerCommand::Player(PlayerCommand::Open(_) | PlayerCommand::Unload)
                | WorkerCommand::SourceRecovery { .. }
        ) {
            self.source_cancelled.store(true, Ordering::Release);
            self.source_epoch = queued.epoch;
            self.source_cancelled = queued.cancelled;
        }
        match queued.command {
            WorkerCommand::Player(command) => self.handle_player(command, shared),
            WorkerCommand::SourceRecovery { source, play } => {
                self.handle_player(
                    source.map_or(PlayerCommand::Unload, PlayerCommand::Open),
                    shared,
                )?;
                if play
                    && self.source_is_current(shared)
                    && !self.source_cancelled.load(Ordering::Acquire)
                {
                    self.handle_player(PlayerCommand::Play, shared)?;
                }
                Ok(())
            }
            WorkerCommand::PrepareSource { source, generation } => {
                if self.generation != generation {
                    return Ok(());
                }
                self.preloader.prepare(
                    source,
                    generation.next()?,
                    self.cache.clone(),
                    self.cache_enabled,
                );
                Ok(())
            }
        }
    }

    fn handle_player(
        &mut self,
        command: PlayerCommand,
        shared: &Shared,
    ) -> Result<(), PlayerError> {
        match command {
            PlayerCommand::Open(SourceLocator::LocalPath(path)) => self.open(path.as_str(), shared),
            PlayerCommand::Open(SourceLocator::HlsUrl(url)) => {
                self.preloader.cancel();
                self.open_hls(url.as_str(), shared)
            }
            PlayerCommand::Unload => {
                self.preloader.cancel();
                self.engine = None;
                self.analysis = None;
                self.waveform_error = None;
                self.output_error = None;
                self.fingerprint = None;
                self.current_source = None;
                self.hls = None;
                self.close_output()?;
                let _admission = shared
                    .command_admission
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if !self.source_is_current(shared) {
                    return Ok(());
                }
                *shared
                    .current_source
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                *shared
                    .waveform
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
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
                    self.transport_revision,
                );
                Ok(())
            }
            PlayerCommand::Play => {
                if self.engine.is_none()
                    && shared
                        .snapshot
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .transport_state()
                        == TransportState::Error
                {
                    return Ok(());
                }
                let replaying = self.engine_mut()?.state() == TransportState::Ended;
                if replaying {
                    self.clear_output()?;
                }
                self.engine_mut()?.play()?;
                self.paused_position = None;
                self.output_drain_started = None;
                let position = self.output.as_ref().map(|output| {
                    let metrics = output.metrics();
                    (metrics.generation, metrics.generation_consumed_frames)
                });
                self.drain_resume.resume(Instant::now(), position);
                self.ensure_output()?;
                if replaying {
                    self.reset_resampler(false)?;
                }
                self.pump()?;
                self.output_error = None;
                self.publish(shared)
            }
            PlayerCommand::Pause => {
                let position = self.audible_position();
                self.engine_mut()?.pause()?;
                if self
                    .resampler
                    .as_ref()
                    .is_some_and(PostMixResampler::finished)
                    && let Some(output) = self.output.as_ref()
                {
                    self.drain_resume
                        .pause(output.presentation_estimate().remaining, Instant::now());
                }
                if let Some(output) = self.output.as_mut() {
                    output.pause()?;
                }
                self.paused_position = position;
                self.output_drain_started = None;
                self.publish(shared)
            }
            PlayerCommand::Stop => {
                self.preloader.cancel();
                self.engine_mut()?.stop()?;
                self.clear_output()?;
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
            PlayerCommand::Cache(CacheCommand::Enable) => {
                self.cache_enabled = true;
                Ok(())
            }
            PlayerCommand::Cache(CacheCommand::Disable) => {
                self.cache_enabled = false;
                self.preloader.cancel();
                Ok(())
            }
            PlayerCommand::Cache(CacheCommand::ClearCurrent) => {
                let result = if let (Some(writer), Some(fingerprint)) =
                    (self.cache_writer.as_mut(), self.fingerprint)
                {
                    writer.clear_current(fingerprint)
                } else {
                    Ok(())
                };
                self.cache_result(result, shared)
            }
            PlayerCommand::Cache(CacheCommand::ClearAll) => {
                self.preloader.cancel();
                let result = self
                    .cache_writer
                    .as_mut()
                    .map_or(Ok(()), CacheWriter::clear_all);
                self.cache_result(result, shared)
            }
            PlayerCommand::Cache(CacheCommand::ClearNetworkSource) => {
                if let Some(source) = &self.hls {
                    source.clear_cache();
                }
                self.publish(shared)
            }
            PlayerCommand::Cache(CacheCommand::SetCapacityBytes(capacity_bytes)) => {
                let result = self
                    .cache_writer
                    .as_mut()
                    .map_or(Ok(()), |writer| writer.set_capacity(capacity_bytes));
                self.cache_result(result, shared)
            }
            PlayerCommand::Cancel(_) => Ok(()),
            PlayerCommand::Shutdown => Ok(()),
        }
    }

    fn open(&mut self, path: &str, shared: &Shared) -> Result<(), PlayerError> {
        let path = Path::new(path);
        self.begin_inspection(Some(path), shared)?;
        let (session, fingerprint, prefetched_cache) =
            match self.preloader.take(path, self.generation) {
                Some(PreparedSource {
                    session,
                    fingerprint,
                    cache,
                    cache_enabled,
                    ..
                }) => (
                    session,
                    fingerprint,
                    (cache_enabled == self.cache_enabled).then_some(cache),
                ),
                None => {
                    let session =
                        open_local_session(path, self.generation, &self.source_cancelled)?;
                    let fingerprint = waveform_fingerprint(&session)?;
                    (session, fingerprint, None)
                }
            };
        self.fingerprint = Some(fingerprint);
        let metadata = session.metadata().clone();
        let config = session.buffer_config();
        let engine =
            PlaybackEngine::with_cancellation(session, Arc::clone(&self.source_cancelled))?;
        let cached = prefetched_cache.or_else(|| {
            self.cache
                .as_ref()
                .filter(|_| self.cache_enabled)
                .map(|cache| {
                    cache.load(
                        fingerprint,
                        self.generation,
                        metadata.sample_rate,
                        metadata.start_frame..metadata.end_frame_exclusive,
                    )
                })
        });
        let cache_status = cached
            .as_ref()
            .map_or(CacheStatus::Disabled, |hit| hit.status);
        let waveform = cached.and_then(|hit| hit.snapshot);
        if waveform.is_some() {
            self.analysis = None;
        } else {
            self.analysis = match AnalysisWork::new(
                &engine,
                cache_status,
                self.generation,
                fingerprint,
                &metadata,
                config.start_frames,
                Arc::clone(&self.source_cancelled),
            ) {
                Ok(analysis) => Some(analysis),
                Err(error) => {
                    self.waveform_error = Some(waveform_warning(error));
                    None
                }
            };
        }
        self.commit_source(engine, waveform, shared)
    }

    fn seek(&mut self, frame: SourceFrame, shared: &Shared) -> Result<(), PlayerError> {
        self.preloader.cancel();
        let resume = self
            .engine
            .as_ref()
            .ok_or_else(not_ready)?
            .playback_requested();
        self.engine_mut()?.seek(frame)?;
        self.clear_output()?;
        self.reset_resampler(false)?;
        if resume
            && self
                .engine
                .as_ref()
                .is_some_and(|engine| !engine.source_exhausted())
        {
            self.ensure_output()?;
            self.pump()?;
        }
        self.publish(shared)
    }

    fn analyze(&mut self, shared: &Shared) -> Result<(), PlayerError> {
        if !self.source_is_current(shared) || self.source_cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let update = match (self.analysis.as_mut(), self.engine.as_ref()) {
            (Some(analysis), Some(engine)) => analysis.step(engine),
            _ => return Ok(()),
        };
        let _admission = shared
            .command_admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.source_is_current(shared) || self.source_cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let update = match update {
            Ok(update) => update,
            Err(error) => {
                self.analysis = None;
                self.waveform_error = Some(waveform_warning(error));
                *shared
                    .waveform
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
                drop(_admission);
                return self.publish(shared);
            }
        };
        if let Some((mut snapshot, complete)) = update {
            snapshot.generation = self.engine.as_ref().ok_or_else(not_ready)?.generation();
            if complete
                && self.cache_enabled
                && let Some(writer) = self.cache_writer.as_mut()
                && let Err(error) = writer.write(snapshot.clone(), self.source_epoch)
            {
                self.waveform_error = Some(waveform_warning(error));
            }
            *shared
                .waveform
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(snapshot);
            if complete {
                self.analysis = None;
                drop(_admission);
                self.publish(shared)?;
            }
        }
        Ok(())
    }

    fn cache_write_pending(&self) -> bool {
        self.cache_writer.as_ref().is_some_and(CacheWriter::pending)
    }

    fn poll_cache_writer(&mut self, shared: &Shared) -> Result<(), PlayerError> {
        let result = self
            .cache_writer
            .as_mut()
            .map_or(Ok(()), |writer| writer.poll(self.source_epoch));
        self.cache_result(result, shared)
    }

    fn shutdown_cache_writer(&mut self) -> Result<(), PlayerError> {
        self.cache_writer
            .as_mut()
            .map_or(Ok(()), CacheWriter::shutdown)
    }

    fn publish(&mut self, shared: &Shared) -> Result<(), PlayerError> {
        let _admission = shared
            .command_admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.source_is_current(shared) {
            return Ok(());
        }
        let Some(engine) = self.engine.as_ref() else {
            return Ok(());
        };
        let mut snapshot = engine.snapshot()?;
        let mut details = snapshot.details().clone();
        if details.error.is_none() {
            details.error = self
                .output_error
                .clone()
                .or_else(|| self.waveform_error.clone());
        }
        let mut waveform = shared
            .waveform
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(waveform) = waveform.as_mut() {
            waveform.generation = snapshot.generation();
        }
        details.waveform = waveform.as_ref().map_or_else(
            || {
                if self.analysis.is_some() {
                    WaveformState::Analyzing
                } else {
                    WaveformState::Unavailable
                }
            },
            |waveform| {
                if waveform.progress == 1.0 {
                    WaveformState::Ready
                } else {
                    WaveformState::Analyzing
                }
            },
        );
        let analysis_diagnostics = self
            .analysis
            .as_ref()
            .map(|analysis| analysis.diagnostics(engine));
        if let Some(waveform) = waveform.as_ref() {
            details.waveform_progress_percent = (waveform.progress * 100.0).round() as u8;
            details.waveform_cache = waveform_cache_state(waveform.cache_status);
        } else if let Some((cache_status, _)) = analysis_diagnostics {
            details.waveform_cache = waveform_cache_state(cache_status);
        }
        details.waveform_throttled = analysis_diagnostics.is_some_and(|(_, throttled)| throttled);
        details.network_diagnostics = self.network_diagnostics();
        drop(waveform);
        snapshot = snapshot.with_details(details);
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
                let frame = self.audible_position().unwrap_or(position.absolute_frame());
                snapshot = snapshot.with_position(PlaybackPosition::new(
                    frame,
                    frames_to_microseconds(frame, source.sample_rate)?,
                ));
            }
            snapshot = snapshot.with_details(details);
            if metrics.clipped_samples > 0 {
                snapshot = with_output_clipping(&snapshot, metrics.clipped_samples)?;
            }
        }
        self.generation = snapshot.generation();
        self.last_snapshot = Instant::now();
        publish(shared, snapshot, self.transport_revision);
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
}

fn resampler_input_frames(source_rate: u32, output_rate: u32) -> usize {
    ((RENDER_FRAMES as u128 * u128::from(source_rate)) / u128::from(output_rate))
        .clamp(1, RENDER_FRAMES as u128) as usize
}

fn waveform_cache_state(status: CacheStatus) -> WaveformCacheState {
    match status {
        CacheStatus::Disabled => WaveformCacheState::Disabled,
        CacheStatus::Miss => WaveformCacheState::Miss,
        CacheStatus::Hit => WaveformCacheState::Hit,
        CacheStatus::Written => WaveformCacheState::Written,
        CacheStatus::Error => WaveformCacheState::Error,
    }
}

fn command_send_error(error: TrySendError<QueuedCommand>) -> PlayerError {
    match error {
        TrySendError::Full(_) => {
            PlayerError::new(ErrorCode::CommandQueueFull, "Command queue is full")
        }
        TrySendError::Disconnected(_) => {
            PlayerError::new(ErrorCode::PlaybackSessionClosed, "Controller is closed")
        }
    }
}

fn should_publish_error(error: &PlayerError, cancelled: bool) -> bool {
    error.code != ErrorCode::BundleAborted || !cancelled
}

fn waveform_warning(mut error: PlayerError) -> PlayerError {
    error.severity = Severity::Warning;
    error
}

fn not_ready() -> PlayerError {
    PlayerError::new(ErrorCode::PlaybackNotReady, "No local source is ready")
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod lifecycle_tests;

#[cfg(test)]
mod recovery_tests;
