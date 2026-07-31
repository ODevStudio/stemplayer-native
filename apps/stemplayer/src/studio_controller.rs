mod analysis;
mod hls;

use analysis::AnalysisWork;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use stemplayer_domain::{
    CacheCommand, CommandId, DisplayName, ErrorCode, GenerationId, PlaybackPosition, PlayerCommand,
    PlayerController, PlayerError, PlayerSnapshot, SnapshotPublisher, SnapshotSubscription,
    SourceFrame, SourceLocator, TransportState, WaveformCacheState, WaveformState,
};
use stemplayer_engine::{PlaybackEngine, PlaybackSession};
use stemplayer_format::StudioBundle;
use stemplayer_output::{DeviceOutput, PostMixResampler, StereoFrame};
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
const MAX_PLAYLIST_ITEMS: usize = 512;
const PLAYLIST_PROGRESS_HIDDEN: u8 = 101;

enum WorkerCommand {
    Player(PlayerCommand),
    SetPlaylist(Vec<std::path::PathBuf>),
}

struct QueuedCommand {
    command: WorkerCommand,
    epoch: u64,
}

struct Shared {
    snapshot: RwLock<PlayerSnapshot>,
    waveform: RwLock<Option<WaveformSnapshot>>,
    subscribers: Mutex<Vec<SnapshotPublisher>>,
    cancel: Arc<AtomicBool>,
    epoch: AtomicU64,
    shutdown: AtomicBool,
    playlist_cache_progress: AtomicU8,
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
            subscribers: Mutex::new(Vec::new()),
            cancel: Arc::new(AtomicBool::new(false)),
            epoch: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            playlist_cache_progress: AtomicU8::new(PLAYLIST_PROGRESS_HIDDEN),
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

    pub fn playlist_cache_progress(&self) -> Option<u8> {
        let progress = self.shared.playlist_cache_progress.load(Ordering::Acquire);
        (progress <= 100).then_some(progress)
    }

    pub fn set_playlist_sources(
        &self,
        sources: Vec<std::path::PathBuf>,
    ) -> Result<(), PlayerError> {
        if sources.len() > MAX_PLAYLIST_ITEMS {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Playlist exceeds its analysis limit",
            ));
        }
        self.sender
            .try_send(QueuedCommand {
                command: WorkerCommand::SetPlaylist(sources),
                epoch: self.shared.epoch.load(Ordering::Acquire),
            })
            .map_err(command_send_error)
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
            command: WorkerCommand::Player(PlayerCommand::Shutdown),
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
            .try_send(QueuedCommand {
                command: WorkerCommand::Player(command),
                epoch,
            })
            .map_err(command_send_error)?;
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
    analysis: Option<AnalysisWork>,
    cache: Option<Arc<WaveformCache>>,
    cache_enabled: bool,
    fingerprint: Option<SourceFingerprint>,
    hls: Option<Arc<stemplayer_network::HlsPlaybackSource>>,
    output: Option<DeviceOutput>,
    resampler: Option<PostMixResampler>,
    selected_device: Option<String>,
    generation: GenerationId,
    current_source: Option<PathBuf>,
    playlist_analysis: PlaylistAnalysis,
    last_output_underflows: u64,
    last_snapshot: Instant,
}

#[derive(Default)]
struct PlaylistAnalysis {
    sources: Vec<PathBuf>,
    completed: Vec<bool>,
    active: Option<PlaylistAnalysisItem>,
}

struct PlaylistAnalysisItem {
    index: usize,
    engine: PlaybackEngine,
    analysis: AnalysisWork,
    progress: f32,
}

impl PlaylistAnalysis {
    fn replace(&mut self, sources: Vec<PathBuf>) {
        self.completed = vec![false; sources.len()];
        self.sources = sources;
        self.active = None;
    }

    fn mark_source(&mut self, source: &Path) {
        if let Some(index) = self.sources.iter().position(|item| item == source) {
            self.completed[index] = true;
            if self
                .active
                .as_ref()
                .is_some_and(|active| active.index == index)
            {
                self.active = None;
            }
        }
    }

    fn reset(&mut self) {
        self.completed.fill(false);
        self.active = None;
    }

    fn next(&self) -> Option<usize> {
        self.completed.iter().position(|completed| !completed)
    }

    fn pending(&self) -> bool {
        self.completed.iter().any(|completed| !completed)
    }
}

impl WorkerState {
    fn new(cache: Option<Arc<WaveformCache>>, cache_enabled: bool) -> Self {
        Self {
            engine: None,
            analysis: None,
            cache,
            cache_enabled,
            fingerprint: None,
            hls: None,
            output: None,
            resampler: None,
            selected_device: None,
            generation: GenerationId::INITIAL,
            current_source: None,
            playlist_analysis: PlaylistAnalysis::default(),
            last_output_underflows: 0,
            last_snapshot: Instant::now(),
        }
    }

    fn handle(&mut self, command: WorkerCommand, shared: &Shared) -> Result<(), PlayerError> {
        match command {
            WorkerCommand::Player(command) => self.handle_player(command, shared),
            WorkerCommand::SetPlaylist(sources) => {
                self.playlist_analysis.replace(sources);
                self.refresh_playlist_progress(shared);
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
            PlayerCommand::Open(SourceLocator::HlsUrl(url)) => self.open_hls(url.as_str(), shared),
            PlayerCommand::Unload => {
                self.close_output()?;
                self.engine = None;
                self.analysis = None;
                self.fingerprint = None;
                self.current_source = None;
                self.hls = None;
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
            PlayerCommand::Cache(CacheCommand::Enable) => {
                self.cache_enabled = true;
                self.playlist_analysis.reset();
                self.refresh_playlist_progress(shared);
                Ok(())
            }
            PlayerCommand::Cache(CacheCommand::Disable) => {
                self.cache_enabled = false;
                self.playlist_analysis.active = None;
                self.refresh_playlist_progress(shared);
                Ok(())
            }
            PlayerCommand::Cache(CacheCommand::ClearCurrent) => {
                if let (Some(cache), Some(fingerprint)) = (&self.cache, self.fingerprint) {
                    cache.clear_current(fingerprint)?;
                }
                if let Some(source) = self.current_source.as_deref()
                    && let Some(index) = self
                        .playlist_analysis
                        .sources
                        .iter()
                        .position(|item| item == source)
                {
                    self.playlist_analysis.completed[index] = false;
                }
                self.refresh_playlist_progress(shared);
                Ok(())
            }
            PlayerCommand::Cache(CacheCommand::ClearAll) => {
                self.cache
                    .as_ref()
                    .map_or(Ok(()), |cache| cache.clear_all())?;
                self.playlist_analysis.reset();
                self.refresh_playlist_progress(shared);
                Ok(())
            }
            PlayerCommand::Cache(CacheCommand::ClearNetworkSource) => {
                if let Some(source) = &self.hls {
                    source.clear_cache();
                }
                self.publish(shared)
            }
            PlayerCommand::Cache(CacheCommand::SetCapacityBytes(capacity_bytes)) => self
                .cache
                .as_ref()
                .map_or(Ok(()), |cache| cache.set_capacity_bytes(capacity_bytes)),
            PlayerCommand::Cancel(_) => Ok(()),
            PlayerCommand::Shutdown => Ok(()),
        }
    }

    fn open(&mut self, path: &str, shared: &Shared) -> Result<(), PlayerError> {
        self.close_output()?;
        self.engine = None;
        self.analysis = None;
        self.fingerprint = None;
        self.current_source = None;
        self.hls = None;
        *shared
            .waveform
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
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
        let path = Path::new(path);
        self.current_source = Some(path.to_path_buf());
        if self
            .playlist_analysis
            .active
            .as_ref()
            .is_some_and(|active| self.playlist_analysis.sources[active.index] == path)
        {
            self.playlist_analysis.active = None;
        }
        let session = open_local_session(path, self.generation, &shared.cancel)?;
        let fingerprint = waveform_fingerprint(&session)?;
        self.fingerprint = Some(fingerprint);
        let metadata = session.metadata().clone();
        let config = session.buffer_config();
        let engine = PlaybackEngine::with_cancellation(session, Arc::clone(&shared.cancel))?;
        let cached = self
            .cache
            .as_ref()
            .filter(|_| self.cache_enabled)
            .map(|cache| cache.load(fingerprint, self.generation));
        let cache_status = cached
            .as_ref()
            .map_or(CacheStatus::Disabled, |hit| hit.status);
        if let Some(snapshot) = cached.and_then(|hit| hit.snapshot) {
            *shared
                .waveform
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(snapshot);
            self.analysis = None;
            self.playlist_analysis.mark_source(path);
        } else {
            self.analysis = Some(AnalysisWork::new(
                &engine,
                cache_status,
                self.generation,
                fingerprint,
                &metadata,
                config.low_frames,
            )?);
        }
        self.engine = Some(engine);
        self.refresh_playlist_progress(shared);
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

    fn analyze(&mut self, shared: &Shared) -> Result<(), PlayerError> {
        let update = match (self.analysis.as_mut(), self.engine.as_ref()) {
            (Some(analysis), Some(engine)) => analysis.step(engine)?,
            _ => None,
        };
        if let Some((snapshot, complete)) = update {
            if complete
                && self.cache_enabled
                && let Some(cache) = self.cache.as_ref().map(Arc::clone)
            {
                let cached = snapshot.clone();
                thread::spawn(move || drop(cache.write(&cached)));
            }
            *shared
                .waveform
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(snapshot);
            if complete {
                self.analysis = None;
                if let Some(source) = self.current_source.as_deref() {
                    self.playlist_analysis.mark_source(source);
                }
                self.refresh_playlist_progress(shared);
                self.publish(shared)?;
            } else {
                self.refresh_playlist_progress(shared);
            }
        }
        if self.analysis.is_none()
            && self
                .engine
                .as_ref()
                .is_none_or(|engine| engine.state() != TransportState::Playing)
        {
            self.analyze_playlist(shared);
        }
        Ok(())
    }

    fn analyze_playlist(&mut self, shared: &Shared) {
        if !self.cache_enabled || self.cache.is_none() || !self.playlist_analysis.pending() {
            self.refresh_playlist_progress(shared);
            return;
        }
        if let Some(active) = self.playlist_analysis.active.as_mut() {
            match active.analysis.step(&active.engine) {
                Ok(Some((snapshot, complete))) => {
                    active.progress = snapshot.progress;
                    if complete {
                        let index = active.index;
                        if let Some(cache) = &self.cache {
                            let _ = cache.write(&snapshot);
                        }
                        self.playlist_analysis.completed[index] = true;
                        self.playlist_analysis.active = None;
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    let index = active.index;
                    self.playlist_analysis.completed[index] = true;
                    self.playlist_analysis.active = None;
                }
            }
            self.refresh_playlist_progress(shared);
            return;
        }
        let Some(index) = self.playlist_analysis.next() else {
            self.refresh_playlist_progress(shared);
            return;
        };
        let source = self.playlist_analysis.sources[index].clone();
        let item: Result<Option<PlaylistAnalysisItem>, PlayerError> = (|| {
            let cancelled = AtomicBool::new(false);
            let session = open_local_session(&source, GenerationId::INITIAL, &cancelled)?;
            let fingerprint = waveform_fingerprint(&session)?;
            let metadata = session.metadata().clone();
            let config = session.buffer_config();
            let cached = self
                .cache
                .as_ref()
                .map(|cache| cache.load(fingerprint, GenerationId::INITIAL));
            let cache_status = cached
                .as_ref()
                .map_or(CacheStatus::Disabled, |result| result.status);
            if cached.and_then(|result| result.snapshot).is_some() {
                return Ok(None);
            }
            let engine = PlaybackEngine::new(session)?;
            let analysis = AnalysisWork::new(
                &engine,
                cache_status,
                GenerationId::INITIAL,
                fingerprint,
                &metadata,
                config.low_frames,
            )?;
            Ok(Some(PlaylistAnalysisItem {
                index,
                engine,
                analysis,
                progress: 0.0,
            }))
        })();
        match item {
            Ok(Some(item)) => self.playlist_analysis.active = Some(item),
            Ok(None) | Err(_) => self.playlist_analysis.completed[index] = true,
        }
        self.refresh_playlist_progress(shared);
    }

    fn playlist_work_pending(&self) -> bool {
        self.cache_enabled && self.cache.is_some() && self.playlist_analysis.pending()
    }

    fn refresh_playlist_progress(&self, shared: &Shared) {
        let progress = if !self.cache_enabled
            || self.cache.is_none()
            || self.playlist_analysis.sources.is_empty()
        {
            PLAYLIST_PROGRESS_HIDDEN
        } else {
            let completed = self
                .playlist_analysis
                .completed
                .iter()
                .filter(|completed| **completed)
                .count();
            if completed == self.playlist_analysis.sources.len() {
                PLAYLIST_PROGRESS_HIDDEN
            } else {
                let active_progress =
                    self.playlist_analysis
                        .active
                        .as_ref()
                        .map(|active| active.progress)
                        .or_else(|| {
                            let source = self.current_source.as_ref()?;
                            let index = self
                                .playlist_analysis
                                .sources
                                .iter()
                                .position(|item| item == source)?;
                            (!self.playlist_analysis.completed[index] && self.analysis.is_some())
                                .then(|| {
                                    shared
                                        .waveform
                                        .read()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                                        .as_ref()
                                        .map_or(0.0, |waveform| waveform.progress)
                                })
                        })
                        .unwrap_or(0.0);
                (((completed as f32 + active_progress)
                    / self.playlist_analysis.sources.len() as f32)
                    * 100.0)
                    .floor()
                    .clamp(0.0, 99.0) as u8
            }
        };
        if shared
            .playlist_cache_progress
            .swap(progress, Ordering::AcqRel)
            != progress
        {
            let snapshot = shared
                .snapshot
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            publish(shared, snapshot);
        }
    }

    fn publish(&mut self, shared: &Shared) -> Result<(), PlayerError> {
        let Some(engine) = self.engine.as_ref() else {
            return Ok(());
        };
        let mut snapshot = engine.snapshot()?;
        let mut details = snapshot.details().clone();
        let waveform = shared
            .waveform
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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

fn open_local_session(
    path: &Path,
    generation: GenerationId,
    cancelled: &AtomicBool,
) -> Result<PlaybackSession, PlayerError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "mp4" => PlaybackSession::open_compressed(path, generation, cancelled),
        "m4a" => PlaybackSession::open_compressed(path, generation, cancelled)
            .or_else(|_| PlaybackSession::open_audio(path, generation)),
        "mp3" | "flac" | "ogg" | "oga" => PlaybackSession::open_audio(path, generation),
        "wav" if !role_suffixed_wav(path) => PlaybackSession::open_audio(path, generation),
        "json" | "wav" if path.is_file() => {
            let group = stemplayer_platform::select_local_source(path)?;
            let bundle = StudioBundle::inspect(group, false, cancelled)?;
            PlaybackSession::new(bundle, generation)
        }
        _ if path.is_dir() => {
            let group = stemplayer_platform::select_local_source(path)?;
            let bundle = StudioBundle::inspect(group, false, cancelled)?;
            PlaybackSession::new(bundle, generation)
        }
        _ => Err(PlayerError::new(
            ErrorCode::CodecUnsupported,
            "Selected file is not a supported audio or stem source",
        )),
    }
}

fn waveform_fingerprint(session: &PlaybackSession) -> Result<SourceFingerprint, PlayerError> {
    let evidence = session
        .fingerprint_inputs()
        .into_iter()
        .map(|(descriptor, path)| SourceEvidence::from_path(&path, descriptor))
        .collect::<Result<Vec<_>, _>>()?;
    SourceFingerprint::from_evidence(&evidence)
}

fn resampler_input_frames(source_rate: u32, output_rate: u32) -> usize {
    ((RENDER_FRAMES as u128 * u128::from(source_rate)) / u128::from(output_rate))
        .clamp(1, RENDER_FRAMES as u128) as usize
}

fn worker_loop(
    receiver: Receiver<QueuedCommand>,
    shared: Arc<Shared>,
    cache: Option<Arc<WaveformCache>>,
    cache_enabled: bool,
) {
    let mut state = WorkerState::new(cache, cache_enabled);
    while !shared.shutdown.load(Ordering::Acquire) {
        let playing = state
            .engine
            .as_ref()
            .is_some_and(|engine| engine.state() == TransportState::Playing);
        let timeout = if playing {
            ACTIVE_TICK
        } else if state.analysis.is_some() || state.playlist_work_pending() {
            Duration::ZERO
        } else {
            IDLE_TICK
        };
        match receiver.recv_timeout(timeout) {
            Ok(queued) => {
                if queued.epoch < shared.epoch.load(Ordering::Acquire)
                    && matches!(&queued.command, WorkerCommand::Player(_))
                {
                    continue;
                }
                shared.cancel.store(false, Ordering::Release);
                if let Err(error) = state.handle(queued.command, &shared)
                    && should_publish_error(&error, shared.cancel.load(Ordering::Acquire))
                {
                    publish_error(&shared, &state, error);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if let Err(error) = state
            .pump()
            .and_then(|()| state.analyze(&shared))
            .and_then(|()| state.publish_if_due(&shared))
            && should_publish_error(&error, shared.cancel.load(Ordering::Acquire))
        {
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

fn role_suffixed_wav(path: &std::path::Path) -> bool {
    path.file_stem()
        .and_then(|value| value.to_str())
        .and_then(|value| value.rsplit('.').next())
        .is_some_and(|role| {
            ["drums", "bass", "other", "vocals", "master"]
                .iter()
                .any(|candidate| role.eq_ignore_ascii_case(candidate))
        })
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
