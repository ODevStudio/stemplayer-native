use super::*;
use stemplayer_domain::{BoundedText, GainDb, StemRole};
use stemplayer_engine::{ExternalSource, SourceBlock};

struct SyntheticSource;

impl ExternalSource for SyntheticSource {
    fn decode_block(
        &self,
        generation: GenerationId,
        start_frame: u64,
        frames: u32,
        _cancelled: &AtomicBool,
    ) -> Result<SourceBlock, PlayerError> {
        SourceBlock::new(
            generation,
            start_frame,
            frames,
            8,
            vec![0.25; frames as usize * 8],
        )
    }
}

pub(super) fn harness() -> (StudioController, Receiver<QueuedCommand>) {
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
    (
        StudioController {
            sender,
            shared,
            next_id: AtomicU64::new(1),
            worker: Mutex::new(None),
        },
        receiver,
    )
}

pub(super) fn engine(cancelled: Arc<AtomicBool>) -> PlaybackEngine {
    let session = PlaybackSession::new_external(
        "Synthetic controller source",
        48_000,
        96_000,
        false,
        GenerationId::INITIAL,
        Arc::new(SyntheticSource),
    )
    .unwrap();
    PlaybackEngine::with_cancellation(session, cancelled).unwrap()
}

fn service_priming(state: &mut WorkerState) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while state.engine.as_ref().unwrap().state() == TransportState::Priming {
        assert!(Instant::now() < deadline);
        state.pump().unwrap();
        thread::yield_now();
    }
}

pub(super) fn open_command(name: &str) -> PlayerCommand {
    PlayerCommand::Open(SourceLocator::LocalPath(BoundedText::new(name).unwrap()))
}

#[test]
fn pause_then_seek_preserves_order_and_mixer_controls() {
    let (controller, receiver) = harness();
    let mut state = WorkerState::new(None, false);
    state.source_cancelled = Arc::clone(&controller.shared.cancel.lock().unwrap());
    state.engine = Some(engine(Arc::clone(&state.source_cancelled)));
    state.engine_mut().unwrap().play().unwrap();
    service_priming(&mut state);
    controller.submit(PlayerCommand::Pause).unwrap();
    controller
        .submit(PlayerCommand::SetGain {
            role: StemRole::Vocals,
            gain_db: GainDb::new(-3.0).unwrap(),
        })
        .unwrap();
    controller
        .submit(PlayerCommand::SeekFrame(SourceFrame::new(48_000)))
        .unwrap();
    for queued in receiver.try_iter() {
        assert!(should_process_command(&queued, &controller.shared));
        state.handle(queued, &controller.shared).unwrap();
    }
    service_priming(&mut state);
    assert_eq!(
        state.engine.as_ref().unwrap().state(),
        TransportState::Paused
    );
    assert_eq!(
        state.engine.as_ref().unwrap().position(),
        SourceFrame::new(48_000)
    );
    assert_eq!(
        state
            .engine
            .as_ref()
            .unwrap()
            .snapshot()
            .unwrap()
            .details()
            .mixer
            .stems[3]
            .gain_db
            .get(),
        -3.0
    );
    assert!(!state.source_cancelled.load(Ordering::Acquire));
}

#[test]
fn superseded_open_play_cannot_autoplay_a_load_only_replacement() {
    let (controller, receiver) = harness();
    controller.submit(open_command("synthetic-a.wav")).unwrap();
    controller.submit(PlayerCommand::Play).unwrap();
    controller.submit(open_command("synthetic-b.wav")).unwrap();
    let first = receiver.recv().unwrap();
    let play = receiver.recv().unwrap();
    let replacement = receiver.recv().unwrap();
    assert!(!should_process_command(&first, &controller.shared));
    assert!(!should_process_command(&play, &controller.shared));
    assert!(should_process_command(&replacement, &controller.shared));
    assert!(first.cancelled.load(Ordering::Acquire));
    assert!(!replacement.cancelled.load(Ordering::Acquire));
    let mut state = WorkerState::new(None, false);
    state.source_epoch = replacement.epoch;
    state.transport_revision = replacement.transport_revision;
    state.source_cancelled = replacement.cancelled;
    state
        .commit_source(
            engine(Arc::clone(&state.source_cancelled)),
            None,
            &controller.shared,
        )
        .unwrap();
    assert_eq!(
        controller.latest_snapshot().transport_state(),
        TransportState::Ready
    );
    assert!(!controller.play_requested());
    assert!(!state.engine.as_ref().unwrap().playback_requested());
}

#[test]
fn same_source_play_remains_ordered_before_seek() {
    let (controller, receiver) = harness();
    controller.submit(PlayerCommand::Play).unwrap();
    controller
        .submit(PlayerCommand::SeekFrame(SourceFrame::new(48_000)))
        .unwrap();
    let play = receiver.recv().unwrap();
    let seek = receiver.recv().unwrap();
    assert!(should_process_command(&play, &controller.shared));
    assert!(should_process_command(&seek, &controller.shared));
    assert!(matches!(
        play.command,
        WorkerCommand::Player(PlayerCommand::Play)
    ));
    assert!(matches!(
        seek.command,
        WorkerCommand::Player(PlayerCommand::SeekFrame(_))
    ));
}

#[test]
fn device_and_cache_preferences_survive_source_supersession() {
    let (controller, receiver) = harness();
    controller
        .submit(PlayerCommand::SelectDevice(Some(
            BoundedText::new("missing-saved-endpoint").unwrap(),
        )))
        .unwrap();
    controller
        .submit(PlayerCommand::Cache(CacheCommand::Disable))
        .unwrap();
    controller.submit(open_command("synthetic.wav")).unwrap();
    let mut state = WorkerState::new(None, true);
    for _ in 0..2 {
        let queued = receiver.recv().unwrap();
        assert!(should_process_command(&queued, &controller.shared));
        state.handle(queued, &controller.shared).unwrap();
    }
    assert_eq!(
        state.selected_device.as_deref(),
        Some("missing-saved-endpoint")
    );
    assert!(!state.cache_enabled);
    assert!(state.output.is_none());
}

#[test]
fn superseded_success_and_failure_cannot_publish() {
    for shutdown in [false, true] {
        let (controller, receiver) = harness();
        controller.submit(open_command("synthetic-a.wav")).unwrap();
        let queued = receiver.recv().unwrap();
        let mut state = WorkerState::new(None, false);
        state.source_epoch = queued.epoch;
        state.transport_revision = queued.transport_revision;
        state.source_cancelled = queued.cancelled;
        if shutdown {
            controller.signal_shutdown();
        } else {
            controller.submit(open_command("synthetic-b.wav")).unwrap();
        }
        let error = state
            .commit_source(
                engine(Arc::clone(&state.source_cancelled)),
                None,
                &controller.shared,
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::BundleAborted);
        publish_error(
            &controller.shared,
            &mut state,
            PlayerError::new(ErrorCode::DecodeFailed, "Synthetic obsolete failure"),
        );
        assert_eq!(
            controller.latest_snapshot().transport_state(),
            TransportState::Empty
        );
        assert!(controller.shared.waveform.read().unwrap().is_none());
        assert!(state.source_cancelled.load(Ordering::Acquire));
    }
}

#[test]
fn fault_during_priming_disarms_resume_and_paused_seek_still_refills() {
    let mut state = WorkerState::new(None, false);
    state.engine = Some(engine(Arc::clone(&state.source_cancelled)));
    state.engine_mut().unwrap().play().unwrap();
    assert_eq!(
        state.engine.as_ref().unwrap().state(),
        TransportState::Priming
    );
    state.pause_for_output_fault().unwrap();
    state
        .engine_mut()
        .unwrap()
        .seek(SourceFrame::new(48_000))
        .unwrap();
    assert!(!state.engine.as_ref().unwrap().playback_requested());
    service_priming(&mut state);
    assert_eq!(
        state.engine.as_ref().unwrap().state(),
        TransportState::Paused
    );
    assert!(state.output.is_none());
}

#[test]
fn fault_at_the_submitted_end_cannot_claim_presentation_completed() {
    let mut state = WorkerState::new(None, false);
    state.engine = Some(engine(Arc::clone(&state.source_cancelled)));
    state.engine_mut().unwrap().play().unwrap();
    state.pause_for_output_fault().unwrap();
    state
        .seek_before_source_end(SourceFrame::new(96_000))
        .unwrap();
    service_priming(&mut state);
    let engine = state.engine.as_ref().unwrap();
    assert_eq!(engine.state(), TransportState::Paused);
    assert_eq!(engine.position(), SourceFrame::new(95_999));
    assert!(!engine.playback_requested());
}

#[test]
fn analysis_survives_seek_stop_and_uses_the_current_generation() {
    let (controller, _) = harness();
    let mut state = WorkerState::new(None, false);
    state.source_cancelled = Arc::clone(&controller.shared.cancel.lock().unwrap());
    let engine = engine(Arc::clone(&state.source_cancelled));
    let metadata = engine.snapshot().unwrap().source().unwrap().clone();
    state.analysis = Some(
        AnalysisWork::new(
            &engine,
            CacheStatus::Disabled,
            engine.generation(),
            SourceFingerprint::from_bytes([9; 32]),
            &metadata,
            1,
            Arc::clone(&state.source_cancelled),
        )
        .unwrap(),
    );
    state.engine = Some(engine);
    state.seek(SourceFrame::new(1), &controller.shared).unwrap();
    assert!(state.analysis.is_some());
    state
        .handle_player(PlayerCommand::Stop, &controller.shared)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while state.analysis.is_some() {
        assert!(Instant::now() < deadline);
        state.analyze(&controller.shared).unwrap();
    }
    assert!(!state.source_cancelled.load(Ordering::Acquire));
    assert_eq!(
        controller
            .shared
            .waveform
            .read()
            .unwrap()
            .as_ref()
            .unwrap()
            .generation,
        controller.latest_snapshot().generation()
    );
}

fn temporary_cache() -> (Arc<WaveformCache>, PathBuf) {
    static NEXT_CACHE: AtomicU64 = AtomicU64::new(1);
    let root = std::env::temp_dir().join(format!(
        "stemplayer-controller-cache-{}-{}",
        std::process::id(),
        NEXT_CACHE.fetch_add(1, Ordering::Relaxed),
    ));
    let cache =
        WaveformCache::new(root.clone(), stemplayer_waveform::CacheConfig::default()).unwrap();
    (Arc::new(cache), root)
}

#[test]
fn cache_management_failure_is_a_nonfatal_warning() {
    let (controller, _) = harness();
    let (cache, root) = temporary_cache();
    let mut state = WorkerState::new(Some(Arc::clone(&cache)), true);
    state.engine = Some(engine(Arc::clone(&state.source_cancelled)));
    state
        .handle_player(
            PlayerCommand::Cache(CacheCommand::SetCapacityBytes(0)),
            &controller.shared,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while state.cache_write_pending() {
        assert!(Instant::now() < deadline);
        state.poll_cache_writer(&controller.shared).unwrap();
        thread::yield_now();
    }
    let snapshot = controller.latest_snapshot();
    assert_eq!(snapshot.transport_state(), TransportState::Ready);
    assert_eq!(
        snapshot.details().error.as_ref().unwrap().severity,
        Severity::Warning
    );
    assert!(state.engine.is_some());
    drop(state);
    drop(cache);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn source_replacement_and_shutdown_prevent_analysis_publication_and_cache_enqueue() {
    for shutdown in [false, true] {
        let (controller, _receiver) = harness();
        let (cache, root) = temporary_cache();
        let mut state = WorkerState::new(Some(Arc::clone(&cache)), true);
        state.source_cancelled = Arc::clone(&controller.shared.cancel.lock().unwrap());
        let engine = engine(Arc::clone(&state.source_cancelled));
        let metadata = engine.snapshot().unwrap().source().unwrap().clone();
        state.analysis = Some(
            AnalysisWork::new(
                &engine,
                CacheStatus::Disabled,
                engine.generation(),
                SourceFingerprint::from_bytes([8; 32]),
                &metadata,
                1,
                Arc::clone(&state.source_cancelled),
            )
            .unwrap(),
        );
        state.engine = Some(engine);
        if shutdown {
            controller.signal_shutdown();
        } else {
            controller
                .submit(open_command("synthetic-next.wav"))
                .unwrap();
        }
        state.analyze(&controller.shared).unwrap();
        assert!(controller.shared.waveform.read().unwrap().is_none());
        assert!(controller.latest_snapshot().details().error.is_none());
        assert!(!state.cache_write_pending());
        drop(state);
        drop(cache);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn source_failure_remains_primary_when_output_teardown_also_fails() {
    let detail = stemplayer_domain::ErrorDetail {
        key: BoundedText::new("role").unwrap(),
        value: BoundedText::new("vocals").unwrap(),
    };
    let teardown = PlayerError::new(ErrorCode::Internal, "Synthetic output teardown failure");
    for count in [0, 16] {
        let primary = PlayerError::new(ErrorCode::DecodeFailed, "Synthetic source failure")
            .with_user_action("Choose another source")
            .with_details(vec![detail.clone(); count])
            .unwrap();
        let selected = lifecycle::with_teardown_error(primary.clone(), Some(&teardown));
        assert_eq!(selected.code, primary.code);
        assert_eq!(selected.category, primary.category);
        assert_eq!(selected.severity, primary.severity);
        assert_eq!(selected.recoverability, primary.recoverability);
        assert_eq!(selected.summary, primary.summary);
        assert_eq!(selected.user_action, primary.user_action);
        if count == 0 {
            assert_eq!(selected.details.len(), 1);
            assert_eq!(selected.details[0].key.as_str(), "output_teardown_error");
            assert_eq!(selected.details[0].value.as_str(), teardown.code.as_str());
        } else {
            assert_eq!(selected.details, primary.details);
        }
        assert_eq!(
            lifecycle::with_teardown_error(primary.clone(), None).details,
            primary.details
        );
    }
}
