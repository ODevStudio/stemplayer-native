use super::*;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter};
use stemplayer_domain::{
    ChannelCount, CommandId, DisplayName, ErrorCode, GenerationId, PlayerCommand, SampleRate,
    SnapshotDetails, SnapshotSubscription, SourceKind, SourceMetadata,
};

struct TestPlatform {
    window: Rc<MinimalSoftwareWindow>,
    now: Rc<Cell<Duration>>,
}

impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }

    fn duration_since_start(&self) -> Duration {
        self.now.get()
    }
}

#[derive(Default)]
struct Recorder(Mutex<Vec<PlayerCommand>>);

impl PlayerController for Recorder {
    fn submit(&self, command: PlayerCommand) -> Result<CommandId, PlayerError> {
        let mut commands = self.0.lock().unwrap();
        commands.push(command);
        Ok(CommandId::new(commands.len() as u64))
    }

    fn latest_snapshot(&self) -> PlayerSnapshot {
        PlayerSnapshot::empty(TransportState::Empty)
    }

    fn subscribe(&self) -> SnapshotSubscription {
        SnapshotSubscription::coalescing().1
    }
}

#[test]
fn play_selection_export_admission_and_gain_flush_are_consistent() {
    let now = Rc::new(Cell::new(Duration::ZERO));
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(TestPlatform {
        window: Rc::clone(&window),
        now: Rc::clone(&now),
    }))
    .unwrap();
    let app = AppWindow::new().unwrap();
    let playlist = Arc::new(Mutex::new(Playlist::default()));
    playlist.lock().unwrap().append(
        (0..2)
            .map(|index| crate::playlist::PlaylistItem {
                label: format!("Track {index}"),
                source: format!("track-{index}.mp3").into(),
            })
            .collect(),
    );
    playlist.lock().unwrap().select(1, true, false);
    present_playlist(&app.as_weak(), &playlist);
    assert!(app.get_playlist_items().row_data(0).unwrap().selected);
    assert!(app.get_playlist_items().row_data(1).unwrap().selected);
    let ready = source_metadata_snapshot(TransportState::Ready, None);
    assert_eq!(
        activate_playlist_for_play(&app.as_weak(), &playlist, &ready),
        Some("track-1.mp3".into())
    );
    assert!(!app.get_playlist_items().row_data(0).unwrap().selected);
    assert!(app.get_playlist_items().row_data(1).unwrap().selected);
    assert_eq!(
        activate_playlist_for_play(&app.as_weak(), &playlist, &ready),
        None
    );
    assert_eq!(
        activate_playlist_for_play(
            &app.as_weak(),
            &playlist,
            &PlayerSnapshot::empty(TransportState::Empty),
        ),
        Some("track-1.mp3".into())
    );
    failed_source_metadata_reopens_without_reloading_live_faults(&app, &playlist);
    assert!(diagnostics::begin_export(&app));
    assert!(!diagnostics::begin_export(&app));
    assert_eq!(app.get_diagnostics_export_status(), "Exporting...");

    let recorder = Arc::new(Recorder::default());
    let commands = Arc::new(CommandAdapter::new(recorder.clone()));
    let timer = Timer::default();
    commands.preview_gain(StemRole::Drums, stemplayer_domain::GainDb::UNITY);
    schedule_gain_flush(&timer, commands);
    now.set(Duration::from_millis(20));
    slint::platform::update_timers_and_animations();
    assert_eq!(recorder.0.lock().unwrap().len(), 1);
    assert!(!timer.running());
    now.set(Duration::from_secs(1));
    slint::platform::update_timers_and_animations();
    assert_eq!(recorder.0.lock().unwrap().len(), 1);
    playing_header_does_not_follow_playlist_focus(&app, &window, &playlist);
    delayed_discovery_preserves_newer_source_intent(&app);
    latest_discovery_activates_in_either_completion_order(&app);
    stale_discovery_preserves_playlist_selection_and_history(&app);
}

fn playing_header_does_not_follow_playlist_focus(
    app: &AppWindow,
    window: &MinimalSoftwareWindow,
    playlist: &Arc<Mutex<Playlist>>,
) {
    app.set_playlist_expanded(false);
    app.show().unwrap();
    app.window().set_size(slint::PhysicalSize::new(780, 620));
    let render = || {
        app.window().request_redraw();
        let size = app.window().size();
        let mut pixels = vec![slint::Rgb8Pixel::default(); (size.width * size.height) as usize];
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, size.width as usize);
        }));
        assert!(pixels.iter().any(|pixel| *pixel != pixels[0]));
        pixels
    };
    for state in [TransportState::Playing, TransportState::Paused] {
        present(app, &source_metadata_snapshot(state, None));
        playlist.lock().unwrap().select(0, false, false);
        present_playlist(&app.as_weak(), playlist);
        let before = render();
        playlist.lock().unwrap().select(1, false, false);
        present_playlist(&app.as_weak(), playlist);
        assert!(
            before == render(),
            "Playlist focus changed the playing header"
        );
        app.set_source_name("Another playing source".into());
        assert!(
            before != render(),
            "The playing source did not update the header"
        );
    }
    for snapshot in [
        PlayerSnapshot::empty(TransportState::Empty),
        source_metadata_snapshot(TransportState::Error, None),
    ] {
        present(app, &snapshot);
        let before = render();
        app.set_source_name("Unplayed source".into());
        assert!(
            before == render(),
            "An inactive source appeared in the header"
        );
    }
    app.hide().unwrap();
}

fn discovered_item(index: usize) -> crate::playlist::PlaylistItem {
    crate::playlist::PlaylistItem {
        label: format!("Discovered {index}"),
        source: format!("discovered-{index}.mp3").into(),
    }
}

fn delayed_discovery_preserves_newer_source_intent(app: &AppWindow) {
    for command in [
        PlayerCommand::Open(SourceLocator::HlsUrl(
            BoundedText::new("https://example.invalid/newer.m3u8").unwrap(),
        )),
        PlayerCommand::Open(SourceLocator::LocalPath(
            BoundedText::new("newer-recent.mp3").unwrap(),
        )),
        PlayerCommand::Unload,
    ] {
        let playlist = Arc::new(Mutex::new(Playlist::default()));
        let requests = discovery::Requests::default();
        let request = requests.begin();
        let recorder = Recorder::default();
        requests.cancel();
        recorder.submit(command).unwrap();
        assert!(discovery::apply(
            Ok(vec![discovered_item(0)]),
            &playlist,
            app,
            &recorder,
            &request,
        ));
        assert_eq!(recorder.0.lock().unwrap().len(), 1);
        assert_eq!(playlist.lock().unwrap().selected(), None);
        assert_eq!(playlist.lock().unwrap().active_source(), None);
        assert_eq!(app.get_playlist_index(), -1);
        assert_eq!(app.get_playlist_items().row_count(), 1);
        assert!(!app.get_playlist_items().row_data(0).unwrap().selected);
        app.set_skip_notice("Newer source notice".into());
        assert!(!discovery::apply(
            Err(PlayerError::new(
                ErrorCode::DecodeFailed,
                "Stale discovery failed"
            )),
            &playlist,
            app,
            &recorder,
            &request,
        ));
        assert_eq!(app.get_skip_notice(), "Newer source notice");
        assert_eq!(recorder.0.lock().unwrap().len(), 1);
    }
}

fn latest_discovery_activates_in_either_completion_order(app: &AppWindow) {
    for latest_first in [false, true] {
        for duplicate in [false, true] {
            let playlist = Arc::new(Mutex::new(Playlist::default()));
            let requests = discovery::Requests::default();
            let stale = requests.begin();
            let latest = requests.begin();
            let recorder = Recorder::default();
            let latest_item = discovered_item(usize::from(!duplicate));
            let stale_completion = (&stale, discovered_item(0));
            let latest_completion = (&latest, latest_item.clone());
            let completions = if latest_first {
                [latest_completion, stale_completion]
            } else {
                [stale_completion, latest_completion]
            };
            for (request, item) in completions {
                assert!(discovery::apply(
                    Ok(vec![item]),
                    &playlist,
                    app,
                    &recorder,
                    request,
                ));
            }
            let commands = recorder.0.lock().unwrap();
            assert_eq!(commands.len(), 1);
            assert!(matches!(
                &commands[0],
                PlayerCommand::Open(SourceLocator::LocalPath(path))
                    if std::path::Path::new(path.as_str()) == latest_item.source
            ));
            let playlist = playlist.lock().unwrap();
            assert_eq!(playlist.active_source(), Some(latest_item.source.clone()));
            assert_eq!(
                playlist.selected_source(),
                latest_item.source.to_str().map(str::to_owned)
            );
            let selected = usize::try_from(app.get_playlist_index()).unwrap();
            assert_eq!(
                app.get_playlist_items().row_data(selected).unwrap().label,
                latest_item.label
            );
        }
    }
}

fn stale_discovery_preserves_playlist_selection_and_history(app: &AppWindow) {
    let playlist = Arc::new(Mutex::new(Playlist::default()));
    {
        let mut playlist = playlist.lock().unwrap();
        playlist.append((0..3).map(discovered_item).collect());
        playlist.set_shuffle(true);
        playlist.next().unwrap();
    }
    let requests = discovery::Requests::default();
    let request = requests.begin();
    let (active, rows) = {
        let mut playlist = playlist.lock().unwrap();
        playlist.select(1, false, false);
        playlist.navigate(2, true, false);
        (playlist.active_source(), playlist.rows())
    };
    requests.cancel();
    let recorder = Recorder::default();
    assert!(discovery::apply(
        Ok(vec![discovered_item(3)]),
        &playlist,
        app,
        &recorder,
        &request,
    ));
    assert!(recorder.0.lock().unwrap().is_empty());
    let mut playlist = playlist.lock().unwrap();
    assert_eq!(playlist.active_source(), active);
    assert_eq!(playlist.selected(), Some(2));
    assert_eq!(&playlist.rows()[..3], rows.as_slice());
    playlist.navigate(0, false, true);
    assert_eq!(
        playlist
            .rows()
            .iter()
            .map(|(_, selected)| *selected)
            .collect::<Vec<_>>(),
        vec![true, true, false, false]
    );
    assert_eq!(playlist.previous(), Some((0, discovered_item(0).source)));
}

fn failed_source_metadata_reopens_without_reloading_live_faults(
    app: &AppWindow,
    playlist: &Arc<Mutex<Playlist>>,
) {
    for code in [
        ErrorCode::DecodeFailed,
        ErrorCode::TimelineInvalid,
        ErrorCode::HlsHttpFailed,
    ] {
        for severity in [Severity::Error, Severity::Fatal] {
            let mut error = PlayerError::new(code, "Synthetic source failure");
            error.severity = severity;
            let snapshot = source_metadata_snapshot(TransportState::Error, Some(error));
            assert!(snapshot.source().is_some());
            assert_eq!(
                activate_playlist_for_play(&app.as_weak(), playlist, &snapshot),
                Some("track-1.mp3".into()),
                "{code:?} {severity:?} must reopen the selected source"
            );
        }
    }
    for (state, code, severity) in [
        (
            TransportState::Ready,
            ErrorCode::DecodeFailed,
            Severity::Warning,
        ),
        (
            TransportState::Paused,
            ErrorCode::HlsHttpFailed,
            Severity::Warning,
        ),
        (
            TransportState::Error,
            ErrorCode::DecodeFailed,
            Severity::Warning,
        ),
        (
            TransportState::Error,
            ErrorCode::PlaybackNotReady,
            Severity::Error,
        ),
        (TransportState::Error, ErrorCode::Internal, Severity::Error),
    ] {
        let mut error = PlayerError::new(code, "Synthetic live-session diagnostic");
        error.severity = severity;
        let snapshot = source_metadata_snapshot(state, Some(error));
        assert_eq!(
            activate_playlist_for_play(&app.as_weak(), playlist, &snapshot),
            None,
            "{state:?} {code:?} {severity:?} must retain the live session"
        );
    }
    assert_eq!(
        activate_playlist_for_play(
            &app.as_weak(),
            playlist,
            &PlayerSnapshot::empty(TransportState::Inspecting),
        ),
        None
    );
}

fn source_metadata_snapshot(state: TransportState, error: Option<PlayerError>) -> PlayerSnapshot {
    PlayerSnapshot::new(
        state,
        GenerationId::INITIAL,
        None,
        Some(SourceMetadata {
            display_name: DisplayName::truncated("Synthetic source"),
            kind: SourceKind::LocalAudioFile,
            has_master: false,
            sample_rate: SampleRate::new(48_000).unwrap(),
            channel_count: ChannelCount::new(2).unwrap(),
            start_frame: SourceFrame::new(0),
            end_frame_exclusive: SourceFrame::new(48_000),
        }),
        vec![],
    )
    .unwrap()
    .with_details(SnapshotDetails {
        error,
        ..SnapshotDetails::default()
    })
}
