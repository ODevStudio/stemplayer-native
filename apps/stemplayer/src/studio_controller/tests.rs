use super::*;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

#[test]
fn cancelled_decode_is_not_published_as_an_error() {
    let cancelled = PlayerError::new(ErrorCode::BundleAborted, "cancelled");
    let failure = PlayerError::new(ErrorCode::Internal, "failed");
    assert!(!should_publish_error(&cancelled, true));
    assert!(should_publish_error(&cancelled, false));
    assert!(should_publish_error(&failure, true));
}

#[test]
fn playlist_inspection_preserves_cancellation() {
    let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let source = std::env::temp_dir().join(format!(
        "stemplayer-controller-{}-{id}.m4a",
        std::process::id()
    ));
    let cache_path = source.with_extension("waveform-cache");
    std::fs::write(&source, [0]).unwrap();
    let cache = Arc::new(
        WaveformCache::new(
            cache_path.clone(),
            stemplayer_waveform::CacheConfig::default(),
        )
        .unwrap(),
    );
    let shared = Shared {
        snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
        waveform: RwLock::new(None),
        current_source: RwLock::new(None),
        subscribers: Mutex::new(Vec::new()),
        command_admission: Mutex::new(()),
        cancel: Arc::new(AtomicBool::new(true)),
        epoch: AtomicU64::new(0),
        shutdown: AtomicBool::new(false),
        playlist_cache_progress: AtomicU8::new(PLAYLIST_PROGRESS_HIDDEN),
    };
    let mut state = WorkerState::new(Some(cache), true);
    state.playlist_analysis.replace(vec![source.clone()]);

    let result = state.analyze_playlist(&shared);
    let completed = state.playlist_analysis.completed.clone();
    let _ = std::fs::remove_file(source);
    let _ = std::fs::remove_dir_all(cache_path);

    assert_eq!(result.unwrap_err().code, ErrorCode::BundleAborted);
    assert_eq!(completed, vec![false]);
}

#[test]
fn cache_write_only_work_uses_a_bounded_poll_timeout() {
    assert_eq!(worker_timeout(false, false, true), ACTIVE_TICK);
}

#[test]
fn playlist_reorder_preserves_completed_analysis() {
    let first = PathBuf::from("first.mp3");
    let second = PathBuf::from("second.mp3");
    let third = PathBuf::from("third.mp3");
    let mut analysis = PlaylistAnalysis::default();
    analysis.replace(vec![first.clone(), second.clone()]);
    analysis.completed = vec![true, false];

    analysis.replace(vec![second, first, third]);

    assert_eq!(analysis.completed, vec![false, true, false]);
    assert!(analysis.active.is_none());
}

#[test]
fn playlist_checks_all_cache_entries_before_analyzing_misses() {
    let mut analysis = PlaylistAnalysis::default();
    analysis.replace(vec!["first.mp3".into(), "second.mp3".into()]);
    analysis.checked[0] = true;

    assert_eq!(analysis.next(), Some(1));
    analysis.checked[1] = true;
    assert_eq!(analysis.next(), Some(0));
}

#[test]
fn failed_playlist_items_do_not_count_as_cached_or_retry_forever() {
    let mut analysis = PlaylistAnalysis::default();
    analysis.replace(vec!["failed.mp3".into(), "cached.mp3".into()]);
    analysis.failed[0] = true;
    analysis.checked[0] = true;
    analysis.completed[1] = true;
    analysis.checked[1] = true;

    assert_eq!(analysis.next(), None);
    assert!(!analysis.pending());
    assert_eq!(analysis.completed, vec![false, true]);
}

#[test]
fn playlist_progress_tracks_only_new_sources() {
    let first = PathBuf::from("first.mp3");
    let second = PathBuf::from("second.mp3");
    let third = PathBuf::from("third.mp3");
    let mut analysis = PlaylistAnalysis::default();
    analysis.replace(vec![first.clone(), second.clone()]);
    analysis.completed = vec![true, true];

    analysis.replace(vec![first, second, third.clone()]);

    assert_eq!(analysis.progress_sources, vec![third]);
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
        shutdown: AtomicBool::new(false),
        playlist_cache_progress: AtomicU8::new(PLAYLIST_PROGRESS_HIDDEN),
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
    assert!(!shared.cancel.load(Ordering::Acquire));
    drop(receiver);
}

#[test]
fn older_playlist_command_preserves_newer_cancellation() {
    let (sender, receiver) = sync_channel(COMMAND_CAPACITY);
    let shared = Arc::new(Shared {
        snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
        waveform: RwLock::new(None),
        current_source: RwLock::new(None),
        subscribers: Mutex::new(Vec::new()),
        command_admission: Mutex::new(()),
        cancel: Arc::new(AtomicBool::new(true)),
        epoch: AtomicU64::new(1),
        shutdown: AtomicBool::new(false),
        playlist_cache_progress: AtomicU8::new(PLAYLIST_PROGRESS_HIDDEN),
    });
    sender
        .send(QueuedCommand {
            command: WorkerCommand::SetPlaylist(Vec::new()),
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
fn paused_controller_caches_the_remaining_playlist() {
    let first_fixture = Fixture::with_frames(200_000);
    let second_fixture = Fixture::with_frames(200_001);
    let cache_path = first_fixture.path.with_extension("playlist-waveform-cache");
    let cache = Arc::new(
        WaveformCache::new(
            cache_path.clone(),
            stemplayer_waveform::CacheConfig::default(),
        )
        .unwrap(),
    );
    let controller = StudioController::with_cache(Some(cache), true);
    controller
        .set_playlist_sources(vec![
            first_fixture.path.clone(),
            second_fixture.path.clone(),
        ])
        .unwrap();
    let path = stemplayer_domain::BoundedText::new(first_fixture.path.to_string_lossy()).unwrap();
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(path)))
        .unwrap();
    wait_for(&controller, TransportState::Ready);
    wait_for_waveform(&controller);
    let deadline = Instant::now() + Duration::from_secs(4);
    while controller.playlist_cache_progress().is_some() {
        assert!(
            Instant::now() < deadline,
            "playlist cache analysis timed out"
        );
        thread::sleep(Duration::from_millis(5));
    }
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
    wait_until(&controller, |snapshot| {
        snapshot.details().waveform_cache == stemplayer_domain::WaveformCacheState::Hit
    });
    assert_eq!(controller.playlist_cache_progress(), None);
    assert_eq!(controller.latest_waveform().unwrap().levels.len(), 1);
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
