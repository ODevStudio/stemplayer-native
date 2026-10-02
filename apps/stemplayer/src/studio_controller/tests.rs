use super::*;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

#[test]
fn load_only_failure_does_not_request_playback_and_explicit_commands_update_intent() {
    let fixture = Fixture::new();
    let controller = StudioController::new();
    let missing =
        stemplayer_domain::BoundedText::new(fixture.path.join("missing.wav").to_string_lossy())
            .unwrap();
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(missing)))
        .unwrap();
    wait_for(&controller, TransportState::Error);
    assert!(!controller.play_requested());

    controller.submit(PlayerCommand::Play).unwrap();
    assert!(controller.play_requested());
    controller.submit(PlayerCommand::Pause).unwrap();
    assert!(!controller.play_requested());

    let valid = stemplayer_domain::BoundedText::new(fixture.path.to_string_lossy()).unwrap();
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(valid)))
        .unwrap();
    wait_for(&controller, TransportState::Ready);
    assert!(!controller.play_requested());
    controller.shutdown_and_join();
}

#[test]
fn cancelled_decode_is_not_published_as_an_error() {
    let cancelled = PlayerError::new(ErrorCode::BundleAborted, "cancelled");
    let failure = PlayerError::new(ErrorCode::Internal, "failed");
    assert!(!should_publish_error(&cancelled, true));
    assert!(should_publish_error(&cancelled, false));
    assert!(should_publish_error(&failure, true));
}

#[test]
fn queued_play_preserves_a_failed_open_until_another_source_is_loaded() {
    let fixture = Fixture::new();
    let shared = Shared {
        snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
        waveform: RwLock::new(None),
        current_source: RwLock::new(None),
        subscribers: Mutex::new(Vec::new()),
        command_admission: Mutex::new(()),
        cancel: Arc::new(AtomicBool::new(false)),
        epoch: AtomicU64::new(0),
        play_requested: AtomicBool::new(false),
        shutdown: AtomicBool::new(false),
    };
    let mut state = WorkerState::new(None, false);
    let error = state
        .open(fixture.path.join("missing.wav").to_str().unwrap(), &shared)
        .unwrap_err();
    let code = error.code;
    publish_error(&shared, &mut state, error);

    state.handle_player(PlayerCommand::Play, &shared).unwrap();

    let snapshot = shared.snapshot.read().unwrap();
    assert_eq!(snapshot.transport_state(), TransportState::Error);
    assert_eq!(snapshot.details().error.as_ref().unwrap().code, code);
    drop(snapshot);

    state.open(fixture.path.to_str().unwrap(), &shared).unwrap();
    let snapshot = shared.snapshot.read().unwrap();
    assert_eq!(snapshot.transport_state(), TransportState::Ready);
    assert!(snapshot.details().error.is_none());
}

#[test]
fn waveform_decode_failure_keeps_the_valid_audio_session_playable() {
    let fixture = Fixture::new();
    let shared = Shared {
        snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
        waveform: RwLock::new(None),
        current_source: RwLock::new(None),
        subscribers: Mutex::new(Vec::new()),
        command_admission: Mutex::new(()),
        cancel: Arc::new(AtomicBool::new(false)),
        epoch: AtomicU64::new(0),
        play_requested: AtomicBool::new(false),
        shutdown: AtomicBool::new(false),
    };
    let mut state = WorkerState::new(None, false);
    state.open(fixture.path.to_str().unwrap(), &shared).unwrap();
    let engine = state.engine.as_ref().unwrap();
    let metadata = stemplayer_domain::SourceMetadata {
        end_frame_exclusive: SourceFrame::new(9),
        ..engine.snapshot().unwrap().source().unwrap().clone()
    };
    state.analysis = Some(
        AnalysisWork::new(
            engine,
            CacheStatus::Disabled,
            state.generation,
            state.fingerprint.unwrap(),
            &metadata,
            1,
        )
        .unwrap(),
    );

    state.analyze(&shared).unwrap();

    assert!(state.analysis.is_none());
    let snapshot = shared.snapshot.read().unwrap();
    assert_eq!(snapshot.transport_state(), TransportState::Ready);
    assert_eq!(snapshot.details().waveform, WaveformState::Unavailable);
    assert_eq!(
        snapshot.details().error.as_ref().unwrap().severity,
        Severity::Warning
    );
    drop(snapshot);

    let engine = state.engine.as_mut().unwrap();
    engine.play().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut rendered = [StereoFrame::default(); 8];
    while engine.render(&mut rendered).unwrap() == 0 {
        assert!(Instant::now() < deadline, "audio decode timed out");
        thread::sleep(Duration::from_millis(5));
    }
    assert!(rendered.iter().any(|frame| frame.left != 0.0));
}

#[test]
fn playback_source_failure_stops_work_and_preserves_the_error_for_recovery() {
    let fixture = Fixture::new();
    let shared = Shared {
        snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
        waveform: RwLock::new(None),
        current_source: RwLock::new(None),
        subscribers: Mutex::new(Vec::new()),
        command_admission: Mutex::new(()),
        cancel: Arc::new(AtomicBool::new(false)),
        epoch: AtomicU64::new(0),
        play_requested: AtomicBool::new(false),
        shutdown: AtomicBool::new(false),
    };
    let mut state = WorkerState::new(None, false);
    state.open(fixture.path.to_str().unwrap(), &shared).unwrap();

    publish_error(
        &shared,
        &mut state,
        PlayerError::new(ErrorCode::DecodeFailed, "Audio decode failed"),
    );
    state.handle_player(PlayerCommand::Play, &shared).unwrap();
    state.publish_if_due(&shared).unwrap();

    assert!(state.engine.is_none());
    assert!(state.analysis.is_none());
    let snapshot = shared.snapshot.read().unwrap();
    assert_eq!(snapshot.transport_state(), TransportState::Error);
    assert!(snapshot.source().is_some());
    assert_eq!(
        snapshot.details().error.as_ref().unwrap().code,
        ErrorCode::DecodeFailed
    );
    drop(snapshot);

    state.handle_player(PlayerCommand::Unload, &shared).unwrap();
    assert_eq!(
        shared.snapshot.read().unwrap().transport_state(),
        TransportState::Empty
    );
    assert!(shared.current_source.read().unwrap().is_none());
}

#[test]
fn waveform_cache_write_failure_is_only_a_warning_and_resets_on_open() {
    let fixture = Fixture::new();
    let cache_path = fixture.path.join("waveform-cache");
    let cache = Arc::new(
        WaveformCache::new(
            cache_path.clone(),
            stemplayer_waveform::CacheConfig::default(),
        )
        .unwrap(),
    );
    std::fs::remove_dir(&cache_path).unwrap();
    std::fs::write(&cache_path, [0]).unwrap();
    let shared = Shared {
        snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
        waveform: RwLock::new(None),
        current_source: RwLock::new(None),
        subscribers: Mutex::new(Vec::new()),
        command_admission: Mutex::new(()),
        cancel: Arc::new(AtomicBool::new(false)),
        epoch: AtomicU64::new(0),
        play_requested: AtomicBool::new(false),
        shutdown: AtomicBool::new(false),
    };
    let mut state = WorkerState::new(Some(cache), true);
    state.open(fixture.path.to_str().unwrap(), &shared).unwrap();
    state.analyze(&shared).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while state.cache_write_pending() {
        assert!(Instant::now() < deadline, "cache write timed out");
        state.poll_cache_writer(&shared).unwrap();
        thread::sleep(Duration::from_millis(5));
    }

    let snapshot = shared.snapshot.read().unwrap();
    assert_eq!(snapshot.transport_state(), TransportState::Ready);
    assert_eq!(snapshot.details().waveform, WaveformState::Ready);
    assert_eq!(
        snapshot.details().error.as_ref().unwrap().severity,
        Severity::Warning
    );
    drop(snapshot);

    state.cache_enabled = false;
    state.open(fixture.path.to_str().unwrap(), &shared).unwrap();
    assert!(shared.snapshot.read().unwrap().details().error.is_none());
}

#[test]
fn preloader_prepares_only_the_requested_source() {
    let fixture = Fixture::new();
    let generation = GenerationId::INITIAL.next().unwrap();
    let mut preloader = SourcePreloader::default();
    preloader.prepare(fixture.path.clone(), generation, None, false);
    let deadline = Instant::now() + Duration::from_secs(2);
    while preloader.pending() {
        assert!(Instant::now() < deadline, "source prefetch timed out");
        preloader.poll();
        thread::sleep(Duration::from_millis(5));
    }

    let prepared = preloader.take(&fixture.path, generation).unwrap();
    assert_eq!(prepared.path, fixture.path);
    assert_eq!(prepared.session.generation(), generation);
}

#[test]
fn preloader_rejects_a_source_changed_after_preparation() {
    let fixture = Fixture::new();
    let generation = GenerationId::INITIAL.next().unwrap();
    let mut preloader = SourcePreloader::default();
    preloader.prepare(fixture.path.clone(), generation, None, false);
    let deadline = Instant::now() + Duration::from_secs(2);
    while preloader.pending() {
        assert!(Instant::now() < deadline, "source prefetch timed out");
        preloader.poll();
        thread::sleep(Duration::from_millis(5));
    }
    std::fs::write(fixture.path.join("drums.wav"), [0]).unwrap();

    assert!(preloader.take(&fixture.path, generation).is_none());
}

#[test]
fn preloader_rejects_a_request_from_an_old_track() {
    let shared = Shared {
        snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
        waveform: RwLock::new(None),
        current_source: RwLock::new(None),
        subscribers: Mutex::new(Vec::new()),
        command_admission: Mutex::new(()),
        cancel: Arc::new(AtomicBool::new(false)),
        epoch: AtomicU64::new(0),
        play_requested: AtomicBool::new(false),
        shutdown: AtomicBool::new(false),
    };
    let mut state = WorkerState::new(None, false);

    state
        .handle(
            WorkerCommand::PrepareSource {
                source: PathBuf::from("stale.wav"),
                generation: GenerationId::INITIAL.next().unwrap(),
            },
            &shared,
        )
        .unwrap();

    assert!(!state.preloader.pending());
}

#[test]
fn cache_write_only_work_uses_a_bounded_poll_timeout() {
    assert_eq!(worker_timeout(false, false, true), ACTIVE_TICK);
}

#[test]
fn shutdown_bypasses_a_full_command_queue() {
    let controller = StudioController::new();
    for _ in 0..COMMAND_CAPACITY {
        let _ = controller.submit(PlayerCommand::Cache(CacheCommand::ClearAll));
    }
    assert!(controller.submit(PlayerCommand::Shutdown).is_ok());
    controller.shutdown_and_join();
    assert_eq!(
        controller.latest_snapshot().transport_state(),
        TransportState::Closed
    );
}

#[test]
fn failed_invalidating_admission_does_not_cancel_or_advance_epoch() {
    let (sender, receiver) = sync_channel(COMMAND_CAPACITY);
    let shared = Arc::new(Shared {
        snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
        waveform: RwLock::new(None),
        current_source: RwLock::new(None),
        subscribers: Mutex::new(Vec::new()),
        command_admission: Mutex::new(()),
        cancel: Arc::new(AtomicBool::new(false)),
        epoch: AtomicU64::new(0),
        play_requested: AtomicBool::new(true),
        shutdown: AtomicBool::new(false),
    });
    let controller = Arc::new(StudioController {
        sender,
        shared: Arc::clone(&shared),
        next_id: AtomicU64::new(1),
        worker: Mutex::new(None),
    });
    for _ in 0..COMMAND_CAPACITY {
        controller
            .submit(PlayerCommand::Cache(CacheCommand::ClearAll))
            .unwrap();
    }

    let path = stemplayer_domain::BoundedText::new("missing.wav").unwrap();
    let error = controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(path)))
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::CommandQueueFull);
    assert_eq!(shared.epoch.load(Ordering::Acquire), 0);
    assert!(controller.play_requested());
    assert!(!shared.cancel.load(Ordering::Acquire));
    drop(receiver);
}

#[test]
fn stale_prefetch_command_preserves_newer_cancellation() {
    let (sender, receiver) = sync_channel(COMMAND_CAPACITY);
    let shared = Arc::new(Shared {
        snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
        waveform: RwLock::new(None),
        current_source: RwLock::new(None),
        subscribers: Mutex::new(Vec::new()),
        command_admission: Mutex::new(()),
        cancel: Arc::new(AtomicBool::new(true)),
        epoch: AtomicU64::new(1),
        play_requested: AtomicBool::new(false),
        shutdown: AtomicBool::new(false),
    });
    sender
        .send(QueuedCommand {
            command: WorkerCommand::PrepareSource {
                source: PathBuf::from("stale.wav"),
                generation: GenerationId::INITIAL,
            },
            epoch: 0,
        })
        .unwrap();
    drop(sender);

    worker_loop(receiver, Arc::clone(&shared), None, false);

    assert!(shared.cancel.load(Ordering::Acquire));
}

#[test]
fn controller_loads_seeks_and_stops_without_opening_audio() {
    let fixture = Fixture::new();
    let controller = StudioController::new();
    let path = stemplayer_domain::BoundedText::new(fixture.path.to_string_lossy()).unwrap();
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(path)))
        .unwrap();
    wait_for(&controller, TransportState::Ready);
    assert_eq!(
        controller
            .latest_snapshot()
            .source()
            .unwrap()
            .end_frame_exclusive,
        SourceFrame::new(8)
    );

    wait_for_waveform(&controller);
    let waveform = controller.latest_waveform().unwrap();
    assert_eq!(waveform.progress, 1.0);
    assert_eq!(waveform.levels.len(), 1);
    assert!(waveform.levels.iter().all(|level| level.roles.len() == 4));

    controller
        .submit(PlayerCommand::SeekFrame(SourceFrame::new(4)))
        .unwrap();
    wait_for(&controller, TransportState::Paused);
    assert_eq!(
        controller
            .latest_snapshot()
            .position()
            .unwrap()
            .absolute_frame(),
        SourceFrame::new(4)
    );

    controller.submit(PlayerCommand::Stop).unwrap();
    wait_for(&controller, TransportState::Ready);
    assert_eq!(
        controller
            .latest_snapshot()
            .position()
            .unwrap()
            .absolute_frame(),
        SourceFrame::new(0)
    );
    controller.shutdown_and_join();
}

#[test]
fn source_switch_replaces_in_progress_waveform_analysis() {
    let first_fixture = Fixture::with_frames(200_000);
    let second_fixture = Fixture::with_frames(16);
    let controller = StudioController::new();
    let first = stemplayer_domain::BoundedText::new(first_fixture.path.to_string_lossy()).unwrap();
    let second =
        stemplayer_domain::BoundedText::new(second_fixture.path.to_string_lossy()).unwrap();

    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(first)))
        .unwrap();
    wait_for(&controller, TransportState::Ready);
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(second)))
        .unwrap();
    wait_until(&controller, |snapshot| {
        snapshot
            .source()
            .is_some_and(|source| source.end_frame_exclusive == SourceFrame::new(16))
    });
    wait_for_waveform(&controller);
    let snapshot = controller.latest_snapshot();
    let waveform = controller.latest_waveform().unwrap();
    assert_eq!(waveform.generation, snapshot.generation());
    assert_eq!(waveform.end_frame_exclusive, SourceFrame::new(16));
    controller.shutdown_and_join();
}

#[test]
fn controller_reopens_from_the_waveform_cache() {
    let fixture = Fixture::new();
    let cache_path = fixture.path.with_extension("waveform-cache");
    let cache = Arc::new(
        WaveformCache::new(
            cache_path.clone(),
            stemplayer_waveform::CacheConfig::default(),
        )
        .unwrap(),
    );
    let path = stemplayer_domain::BoundedText::new(fixture.path.to_string_lossy()).unwrap();
    let first = StudioController::with_cache(Some(Arc::clone(&cache)), true);
    first
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(path.clone())))
        .unwrap();
    wait_for(&first, TransportState::Ready);
    wait_for_waveform(&first);
    let deadline = Instant::now() + Duration::from_secs(2);
    while std::fs::read_dir(&cache_path).unwrap().all(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_none_or(|value| value != "spwf")
    }) {
        assert!(Instant::now() < deadline, "waveform cache write timed out");
        thread::sleep(Duration::from_millis(5));
    }
    first.shutdown_and_join();

    let second = StudioController::with_cache(Some(Arc::clone(&cache)), true);
    second
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(path.clone())))
        .unwrap();
    wait_for(&second, TransportState::Ready);
    wait_until(&second, |snapshot| {
        snapshot.details().waveform_cache == stemplayer_domain::WaveformCacheState::Hit
    });
    assert_eq!(
        second.latest_waveform().unwrap().cache_status,
        CacheStatus::Hit
    );
    second
        .submit(PlayerCommand::Cache(CacheCommand::ClearCurrent))
        .unwrap();
    wait_for_cache_entries(&cache_path, false);
    second.shutdown_and_join();

    let third = StudioController::with_cache(Some(cache), true);
    third
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(path)))
        .unwrap();
    wait_for(&third, TransportState::Ready);
    wait_for_waveform(&third);
    assert_ne!(
        third.latest_waveform().unwrap().cache_status,
        CacheStatus::Hit
    );
    third.shutdown_and_join();
    let _ = std::fs::remove_dir_all(cache_path);
}

#[test]
fn prefetch_does_not_analyze_unopened_sources_or_remove_saved_waveforms() {
    let first_fixture = Fixture::with_frames(200_000);
    let second_fixture = Fixture::with_frames(200_001);
    let cache_path = first_fixture.path.with_extension("current-waveform-cache");
    let cache = Arc::new(
        WaveformCache::new(
            cache_path.clone(),
            stemplayer_waveform::CacheConfig::default(),
        )
        .unwrap(),
    );
    let controller = StudioController::with_cache(Some(cache), true);
    let path = stemplayer_domain::BoundedText::new(first_fixture.path.to_string_lossy()).unwrap();
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(path)))
        .unwrap();
    wait_for(&controller, TransportState::Ready);
    wait_for_waveform(&controller);
    wait_for_cache_entry_count(&cache_path, 1);
    controller
        .prepare_source(
            second_fixture.path.clone(),
            controller.latest_snapshot().generation(),
        )
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    assert_eq!(cache_entry_count(&cache_path), 1);

    let second =
        stemplayer_domain::BoundedText::new(second_fixture.path.to_string_lossy()).unwrap();
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(second)))
        .unwrap();
    wait_until(&controller, |snapshot| {
        snapshot
            .source()
            .is_some_and(|source| source.end_frame_exclusive == SourceFrame::new(200_001))
    });
    wait_for_waveform(&controller);
    assert_ne!(
        controller.latest_snapshot().details().waveform_cache,
        stemplayer_domain::WaveformCacheState::Hit
    );
    wait_for_cache_entry_count(&cache_path, 2);

    let first = stemplayer_domain::BoundedText::new(first_fixture.path.to_string_lossy()).unwrap();
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(first)))
        .unwrap();
    wait_until(&controller, |snapshot| {
        snapshot.details().waveform_cache == stemplayer_domain::WaveformCacheState::Hit
    });
    controller.shutdown_and_join();
    let _ = std::fs::remove_dir_all(cache_path);
}

#[test]
fn controller_applies_and_resets_mixer_commands_without_opening_audio() {
    let fixture = Fixture::new();
    let controller = StudioController::new();
    let path = stemplayer_domain::BoundedText::new(fixture.path.to_string_lossy()).unwrap();
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(path)))
        .unwrap();
    wait_for(&controller, TransportState::Ready);
    controller
        .submit(PlayerCommand::SetGain {
            role: stemplayer_domain::StemRole::Vocals,
            gain_db: stemplayer_domain::GainDb::new(-3.0).unwrap(),
        })
        .unwrap();
    controller
        .submit(PlayerCommand::SetMute {
            role: stemplayer_domain::StemRole::Bass,
            muted: true,
        })
        .unwrap();
    controller
        .submit(PlayerCommand::SetSolo {
            role: stemplayer_domain::StemRole::Drums,
            solo: true,
        })
        .unwrap();
    controller
        .submit(PlayerCommand::SetOutputPreference(
            stemplayer_domain::OutputPreference::StemMix,
        ))
        .unwrap();
    wait_until(&controller, |snapshot| {
        snapshot.details().mixer.automation_count == 4
    });
    let snapshot = controller.latest_snapshot();
    assert_eq!(snapshot.details().mixer.stems[3].gain_db.get(), -3.0);
    assert!(snapshot.details().mixer.stems[1].muted);
    assert!(snapshot.details().mixer.stems[0].solo);
    assert_eq!(
        snapshot.details().mixer.output_preference,
        stemplayer_domain::OutputPreference::StemMix
    );

    controller.submit(PlayerCommand::ResetMix).unwrap();
    wait_until(&controller, |snapshot| {
        snapshot.details().mixer.automation_count == 5
    });
    let mixer = controller.latest_snapshot().details().mixer;
    assert_eq!(
        mixer.output_preference,
        stemplayer_domain::OutputPreference::Automatic
    );
    assert!(mixer.stems.iter().all(|stem| {
        stem.gain_db == stemplayer_domain::GainDb::UNITY && !stem.muted && !stem.solo
    }));
    controller.shutdown_and_join();
}

#[test]
#[ignore = "requires two native output devices with different default rates"]
fn active_device_rate_change_recreates_and_reprimes_output() {
    let alternate = stemplayer_output::enumerate_devices()
        .unwrap()
        .into_iter()
        .find(|device| {
            !device.is_default
                && device
                    .default_configuration
                    .as_ref()
                    .is_some_and(|config| config.minimum_sample_rate != 44_100)
        })
        .expect("a non-default device with a different rate is required");
    let expected_rate = alternate.default_configuration.unwrap().minimum_sample_rate;
    let fixture = Fixture::with_frames(88_200);
    let controller = StudioController::new();
    let path = stemplayer_domain::BoundedText::new(fixture.path.to_string_lossy()).unwrap();
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(path)))
        .unwrap();
    wait_for(&controller, TransportState::Ready);
    controller.submit(PlayerCommand::Play).unwrap();
    controller
        .submit(PlayerCommand::SelectDevice(Some(
            stemplayer_domain::BoundedText::new(alternate.id).unwrap(),
        )))
        .unwrap();
    wait_until(&controller, |snapshot| {
        snapshot
            .details()
            .resampler
            .is_some_and(|resampler| resampler.discontinuity_count == 1)
    });
    let snapshot = controller.latest_snapshot();
    let resampler = snapshot.details().resampler.unwrap();
    assert_eq!(resampler.output_rate, expected_rate);
    assert_eq!(resampler.reset_count, 1);
    assert_eq!(snapshot.details().underrun_count, 0);
    assert_eq!(snapshot.details().overflow_count, 0);
    assert_eq!(snapshot.transport_state(), TransportState::Playing);
    controller.shutdown_and_join();
}

struct Fixture {
    path: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::with_frames(8)
    }

    fn with_frames(frames: usize) -> Self {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("stemplayer-controller-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let roles = ["drums", "bass", "other", "vocals"];
        for (index, role) in roles.iter().enumerate() {
            std::fs::write(
                path.join(format!("{role}.wav")),
                wave(&vec![1000_i16 + index as i16; frames]),
            )
            .unwrap();
        }
        let outputs = roles
            .iter()
            .map(|role| {
                format!(
                    r#"{{"logical_type":"{role}","filename":"{role}.wav","sample_rate":44100,"channels":2,"frames":{frames}}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(
            path.join("manifest.json"),
            format!(r#"{{"schema_version":1,"outputs":[{outputs}]}}"#),
        )
        .unwrap();
        Self { path }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn wait_for(controller: &StudioController, state: TransportState) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while controller.latest_snapshot().transport_state() != state {
        assert!(
            Instant::now() < deadline,
            "controller did not reach {state:?}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn wait_until(controller: &StudioController, predicate: impl Fn(&PlayerSnapshot) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !predicate(&controller.latest_snapshot()) {
        assert!(Instant::now() < deadline, "controller condition timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_waveform(controller: &StudioController) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while controller
        .latest_waveform()
        .is_none_or(|waveform| waveform.progress != 1.0)
    {
        assert!(Instant::now() < deadline, "waveform analysis timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_cache_entries(path: &std::path::Path, expected: bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let present = std::fs::read_dir(path).unwrap().any(|entry| {
            entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|value| value == "spwf")
        });
        if present == expected {
            return;
        }
        assert!(Instant::now() < deadline, "waveform cache update timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

fn wait_for_cache_entry_count(path: &std::path::Path, expected: usize) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while cache_entry_count(path) != expected {
        assert!(Instant::now() < deadline, "waveform cache update timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

fn cache_entry_count(path: &std::path::Path) -> usize {
    std::fs::read_dir(path)
        .unwrap()
        .filter(|entry| {
            entry.as_ref().is_ok_and(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|value| value == "spwf")
            })
        })
        .count()
}

fn wave(samples: &[i16]) -> Vec<u8> {
    let data_size = (samples.len() * 4) as u32;
    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend(b"RIFF");
    bytes.extend((36 + data_size).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(44_100_u32.to_le_bytes());
    bytes.extend((44_100_u32 * 4).to_le_bytes());
    bytes.extend(4_u16.to_le_bytes());
    bytes.extend(16_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data_size.to_le_bytes());
    for sample in samples {
        bytes.extend(sample.to_le_bytes());
        bytes.extend(sample.to_le_bytes());
    }
    bytes
}
