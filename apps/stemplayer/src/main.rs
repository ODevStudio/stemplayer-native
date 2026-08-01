#![deny(unsafe_code)]
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use slint::ComponentHandle;

#[cfg(all(feature = "local-wav", not(feature = "development")))]
mod playlist;
#[cfg(all(feature = "local-wav", any(test, not(feature = "development"))))]
mod studio_controller;
#[cfg(all(feature = "local-wav", not(feature = "development")))]
mod studio_ui;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mode = args.first();
    let duration = args
        .get(
            if mode.is_some_and(|value| matches!(value.as_str(), "--play-source" | "--play-url")) {
                2
            } else {
                1
            },
        )
        .and_then(|value| value.parse::<u64>().ok());
    let automated = mode.is_some_and(|value| {
        matches!(
            value.as_str(),
            "--ui-smoke"
                | "--ui-idle"
                | "--ui-minimized"
                | "--ui-paused-minimized"
                | "--ui-error"
                | "--ui-no-master"
                | "--ui-long-name"
        ) || (matches!(value.as_str(), "--play-source" | "--play-url") && duration.is_some())
    });
    let settings_path = stemplayer_platform::AppPaths::discover("stemplayer-native")
        .ok()
        .map(|paths| paths.config.join("settings.json"));
    let settings = settings_path
        .as_deref()
        .map(stemplayer_platform::load_settings)
        .map(|loaded| loaded.settings().clone())
        .unwrap_or_default();
    let app = AppWindow::new()?;
    app.set_diagnostics_visible(settings.diagnostics_visible);
    app.set_theme_preference(i32::from(settings.theme));
    app.set_waveform_cache_enabled(settings.waveform_cache_enabled);
    app.set_waveform_cache_limit_index(cache_limit_index(settings.waveform_cache_limit_mb));
    if let (Some(width), Some(height)) = (settings.window_width, settings.window_height) {
        app.window()
            .set_size(slint::LogicalSize::new(width as f32, height as f32));
        app.set_playlist_expanded_window_height(height as f32);
    }
    #[cfg(target_os = "windows")]
    {
        let weak = app.as_weak();
        let position = settings.window_x.zip(settings.window_y);
        let first_start = settings.window_width.is_none() || settings.window_height.is_none();
        let maximized = settings.window_maximized;
        slint::Timer::single_shot(std::time::Duration::from_millis(50), move || {
            if let Some(app) = weak.upgrade() {
                initialize_window(&app, position, first_start, maximized);
            }
        });
    }
    #[cfg(feature = "development")]
    let controller = wire_development(&app);
    #[cfg(all(feature = "local-wav", not(feature = "development")))]
    let controller = studio_ui::wire(
        &app,
        mode.is_some_and(|value| matches!(value.as_str(), "--play-source" | "--play-url")),
        settings_path.clone(),
        &settings,
    );
    if mode.is_some_and(|value| value == "--ui-smoke") {
        app.set_playlist_items(slint::ModelRc::new(slint::VecModel::from(vec![
            PlaylistRow {
                label: "Current track".into(),
                selected: true,
            },
            PlaylistRow {
                label: "Next track".into(),
                selected: false,
            },
            PlaylistRow {
                label: "Final track".into(),
                selected: false,
            },
        ])));
    }
    #[cfg(all(feature = "local-wav", not(feature = "development")))]
    if mode.is_some_and(|value| value == "--play-source")
        && let Some(path) = args.get(1)
    {
        use stemplayer_domain::{BoundedText, PlayerCommand, PlayerController, SourceLocator};
        if let Ok(path) = BoundedText::new(path) {
            let _ = controller.submit(PlayerCommand::Open(SourceLocator::LocalPath(path)));
            let _ = controller.submit(PlayerCommand::Play);
        }
    }
    #[cfg(all(feature = "hls", not(feature = "development")))]
    if mode.is_some_and(|value| value == "--play-url")
        && let Some(url) = args.get(1)
    {
        use stemplayer_domain::{BoundedText, PlayerCommand, PlayerController, SourceLocator};
        if let Ok(url) = BoundedText::new(url) {
            let _ = controller.submit(PlayerCommand::Open(SourceLocator::HlsUrl(url)));
            let _ = controller.submit(PlayerCommand::Play);
        }
    }
    if automated {
        #[cfg(feature = "development")]
        {
            use stemplayer_domain::PlayerController;
            app.invoke_open_source();
            match mode.map(String::as_str) {
                Some("--ui-error") => {
                    controller.fail(
                        stemplayer_domain::PlayerError::new(
                            stemplayer_domain::ErrorCode::Internal,
                            "Output device became unavailable",
                        )
                        .with_user_action("Retry output or choose another device"),
                    );
                    present(&app, &controller.latest_snapshot());
                }
                Some("--ui-no-master") => {
                    controller.set_master_present(false);
                    present(&app, &controller.latest_snapshot());
                }
                Some("--ui-long-name") => {
                    use stemplayer_domain::{BoundedText, PlayerCommand, SourceLocator};
                    let source = SourceLocator::LocalPath(
                        BoundedText::new(
                            "D:/Music/Very long qualification source name with Unicode \u{03a9} and translated text.stem.m4a",
                        )
                        .expect("static qualification path is bounded"),
                    );
                    let _ = controller.submit(PlayerCommand::Open(source));
                    controller.advance(std::time::Duration::ZERO);
                    present(&app, &controller.latest_snapshot());
                }
                Some("--ui-idle" | "--ui-paused-minimized") => {}
                _ => app.invoke_play(),
            }
        }
        if matches!(
            mode.map(String::as_str),
            Some("--ui-minimized" | "--ui-paused-minimized")
        ) {
            let weak = app.as_weak();
            slint::Timer::single_shot(std::time::Duration::from_millis(100), move || {
                if let Some(app) = weak.upgrade() {
                    app.set_window_minimized(true);
                }
            });
        }
        slint::Timer::single_shot(
            std::time::Duration::from_millis(duration.unwrap_or(5_000)),
            || {
                let _ = slint::quit_event_loop();
            },
        );
    }
    app.run()?;
    if let Some(path) = settings_path.as_deref() {
        let current = stemplayer_platform::load_settings(path).settings().clone();
        let size = app.window().size().to_logical(app.window().scale_factor());
        let position = app.window().position();
        let maximized = app.window().is_maximized();
        let width = (size.width * if maximized { 0.9 } else { 1.0 })
            .round()
            .clamp(560.0, 3_840.0) as u32;
        let height = (size.height * if maximized { 0.9 } else { 1.0 })
            .round()
            .clamp(256.0, 2_160.0) as u32;
        let expanded_height = app
            .get_playlist_expanded_window_height()
            .round()
            .clamp(256.0, 2_160.0) as u32;
        let settings = stemplayer_platform::LocalSettings {
            diagnostics_visible: app.get_diagnostics_visible(),
            theme: u8::try_from(app.get_theme_preference()).unwrap_or_default(),
            recent_sources_enabled: app.get_recent_sources_enabled(),
            recent_sources: if app.get_recent_sources_enabled() {
                current.recent_sources.clone()
            } else {
                Vec::new()
            },
            window_width: if maximized {
                current.window_width.or(Some(width))
            } else {
                Some(width)
            },
            window_height: if maximized || !app.get_playlist_expanded() {
                Some(expanded_height)
            } else {
                Some(height)
            },
            window_x: if maximized {
                current.window_x
            } else {
                Some(position.x)
            },
            window_y: if maximized {
                current.window_y
            } else {
                Some(position.y)
            },
            window_maximized: maximized,
            waveform_cache_enabled: app.get_waveform_cache_enabled(),
            waveform_cache_limit_mb: cache_limit_mb(app.get_waveform_cache_limit_index()),
            ..current
        };
        let _ = stemplayer_platform::save_settings(path, &settings);
    }
    #[cfg(feature = "development")]
    {
        use stemplayer_domain::{PlayerCommand, PlayerController};
        let _ = controller.submit(PlayerCommand::Shutdown);
    }
    #[cfg(all(feature = "local-wav", not(feature = "development")))]
    if mode.is_some_and(|value| matches!(value.as_str(), "--play-source" | "--play-url")) {
        use stemplayer_domain::PlayerController;
        if let Ok(line) = serde_json::to_string(&controller.latest_snapshot()) {
            if let Some(path) = args.get(3) {
                let _ = std::fs::write(path, line);
            } else {
                println!("{line}");
            }
        }
    }
    #[cfg(all(feature = "local-wav", not(feature = "development")))]
    controller.shutdown_and_join();
    Ok(())
}

#[cfg(feature = "development")]
fn wire_development(app: &AppWindow) -> std::sync::Arc<stemplayer_ui::MockController> {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use stemplayer_domain::{PlayerCommand, PlayerController, SourceFrame, StemRole};
    use stemplayer_ui::{CommandAdapter, MockController, Playhead};

    let controller = Arc::new(MockController::new());
    let adapter = Arc::new(CommandAdapter::new(controller.clone()));
    let epoch = Instant::now();
    let playhead = Rc::new(RefCell::new(Playhead::from_snapshot(
        &controller.latest_snapshot(),
        Duration::ZERO,
    )));
    present(app, &controller.latest_snapshot());

    let weak = app.as_weak();
    let current = controller.clone();
    let commands = adapter.clone();
    app.on_open_source(move || {
        open_development_source(&commands, &current, &weak);
    });

    let weak = app.as_weak();
    let current = controller.clone();
    let commands = adapter.clone();
    app.on_open_manifest(move || {
        open_development_source(&commands, &current, &weak);
    });

    let weak = app.as_weak();
    let current = controller.clone();
    let commands = adapter.clone();
    app.on_open_folder(move || {
        open_development_source(&commands, &current, &weak);
    });

    let weak = app.as_weak();
    let current = controller.clone();
    let commands = adapter.clone();
    app.on_play(move || {
        let _ = commands.submit(PlayerCommand::Play);
        refresh(&weak, &current);
    });

    let weak = app.as_weak();
    let current = controller.clone();
    let commands = adapter.clone();
    app.on_pause(move || {
        let _ = commands.submit(PlayerCommand::Pause);
        refresh(&weak, &current);
    });

    let weak = app.as_weak();
    let current = controller.clone();
    let commands = adapter.clone();
    app.on_stop(move || {
        let _ = commands.submit(PlayerCommand::Stop);
        refresh(&weak, &current);
    });

    let weak = app.as_weak();
    let current = controller.clone();
    let commands = adapter.clone();
    app.on_seek(move |fraction| {
        commands.preview_seek(SourceFrame::new(
            (fraction.clamp(0.0, 1.0) * 28_800_000.0) as i64,
        ));
        let _ = commands.flush();
        refresh(&weak, &current);
    });

    let weak = app.as_weak();
    let visible_playhead = playhead.clone();
    app.on_tick(move || {
        if let Some(app) = weak.upgrade() {
            let frame = visible_playhead.borrow().visible_frame(epoch.elapsed());
            app.set_seek_position((frame.get() as f32 / 28_800_000.0).clamp(0.0, 1.0));
        }
    });

    let weak = app.as_weak();
    let current = controller.clone();
    let authoritative_playhead = playhead.clone();
    app.on_snapshot_tick(move || {
        current.advance(Duration::from_millis(250));
        let snapshot = current.latest_snapshot();
        authoritative_playhead
            .borrow_mut()
            .update(&snapshot, epoch.elapsed());
        if let Some(app) = weak.upgrade() {
            present(&app, &snapshot);
        }
    });

    let commands = adapter.clone();
    app.on_set_gain(move |index, value| {
        if let Ok(gain) = stemplayer_ui::gain_db_from_position(value) {
            if index == -1 {
                for role in StemRole::ALL {
                    commands.preview_gain(role, gain);
                }
            } else if let Some(role) = role(index) {
                commands.preview_gain(role, gain);
            }
        }
    });

    let weak = app.as_weak();
    let current = controller.clone();
    let commands = adapter.clone();
    app.on_commit_commands(move || {
        let _ = commands.flush();
        refresh(&weak, &current);
    });
    let weak = app.as_weak();
    let current = controller.clone();
    let commands = adapter.clone();
    app.on_set_mute(move |index, muted| {
        if let Some(role) = role(index) {
            let _ = commands.submit(PlayerCommand::SetMute { role, muted });
            refresh(&weak, &current);
        }
    });

    let weak = app.as_weak();
    let current = controller.clone();
    app.on_reset_mix(move || {
        let _ = current.submit(PlayerCommand::ResetMix);
        refresh(&weak, &current);
    });

    let weak = app.as_weak();
    let current = controller.clone();
    app.on_retry_device(move |_| {
        let _ = current.submit(PlayerCommand::SelectDevice(None));
        refresh(&weak, &current);
    });

    let weak = app.as_weak();
    let current = controller.clone();
    app.on_reset_player(move || {
        let _ = current.submit(PlayerCommand::Unload);
        refresh(&weak, &current);
    });

    let weak = app.as_weak();
    let current = controller.clone();
    app.on_set_output_preference(move |preference| {
        let preference = match preference {
            0 => stemplayer_domain::OutputPreference::Automatic,
            1 => stemplayer_domain::OutputPreference::MasterReference,
            _ => stemplayer_domain::OutputPreference::StemMix,
        };
        let _ = current.submit(PlayerCommand::SetOutputPreference(preference));
        refresh(&weak, &current);
    });

    fn role(index: i32) -> Option<StemRole> {
        usize::try_from(index)
            .ok()
            .and_then(|value| StemRole::ALL.get(value).copied())
    }

    controller
}

#[cfg(feature = "development")]
fn open_development_source(
    commands: &stemplayer_ui::CommandAdapter,
    controller: &stemplayer_ui::MockController,
    window: &slint::Weak<AppWindow>,
) {
    use stemplayer_domain::{BoundedText, PlayerCommand, SourceLocator};
    let locator = SourceLocator::LocalPath(
        BoundedText::new("Phase 8 mock.stem.m4a").expect("static mock path is bounded"),
    );
    let _ = commands.submit(PlayerCommand::Open(locator));
    controller.advance(std::time::Duration::ZERO);
    refresh(window, controller);
}

#[cfg(feature = "development")]
fn refresh(window: &slint::Weak<AppWindow>, controller: &stemplayer_ui::MockController) {
    use stemplayer_domain::PlayerController;
    if let Some(app) = window.upgrade() {
        present(&app, &controller.latest_snapshot());
    }
}

#[cfg(any(feature = "development", feature = "local-wav"))]
fn present(app: &AppWindow, snapshot: &stemplayer_domain::PlayerSnapshot) {
    let model = stemplayer_ui::PresentationModel::from_snapshot(snapshot);
    app.set_source_name(model.file_name.into());
    app.set_source_summary(model.source_summary.into());
    app.set_ordinary_audio(
        snapshot
            .source()
            .is_some_and(|source| source.kind == stemplayer_domain::SourceKind::LocalAudioFile),
    );
    app.set_has_network_source(
        snapshot
            .source()
            .is_some_and(|source| source.kind == stemplayer_domain::SourceKind::HlsStemset),
    );
    app.set_state_text(model.state_text.into());
    app.set_time_text(format!("{} / {}", model.current_time, model.total_time).into());
    app.set_output_mode(model.output_mode.into());
    app.set_output_preference(model.output_preference);
    app.set_has_master(model.has_master);
    app.set_device_name(model.device_name.into());
    app.set_output_rate_text(model.output_rate_text.into());
    app.set_waveform_text(model.waveform_text.into());
    app.set_buffering_text(model.buffering_banner.unwrap_or_default().into());
    app.set_error_text(model.error_banner.unwrap_or_default().into());
    app.set_error_action(model.error_action.into());
    app.set_error_code(model.error_code.into());
    app.set_error_status(model.error_status.into());
    app.set_error_details(model.error_details.into());
    app.set_diagnostics_text(model.diagnostics_text.into());
    app.set_can_open(model.transport.open);
    app.set_can_play(model.transport.play);
    app.set_can_pause(model.transport.pause);
    app.set_can_stop(model.transport.stop);
    app.set_can_seek(model.transport.seek);
    app.set_can_mix(model.transport.mixer);
    app.set_drums_gain_position(model.mixer_rows[0].gain_position);
    app.set_bass_gain_position(model.mixer_rows[1].gain_position);
    app.set_other_gain_position(model.mixer_rows[2].gain_position);
    app.set_vocals_gain_position(model.mixer_rows[3].gain_position);
    app.set_drums_gain_text(model.mixer_rows[0].gain_text.clone().into());
    app.set_bass_gain_text(model.mixer_rows[1].gain_text.clone().into());
    app.set_other_gain_text(model.mixer_rows[2].gain_text.clone().into());
    app.set_vocals_gain_text(model.mixer_rows[3].gain_text.clone().into());
    app.set_drums_muted(model.mixer_rows[0].muted);
    app.set_bass_muted(model.mixer_rows[1].muted);
    app.set_other_muted(model.mixer_rows[2].muted);
    app.set_vocals_muted(model.mixer_rows[3].muted);

    if snapshot.transport_state() != stemplayer_domain::TransportState::Playing {
        let seek_position = snapshot
            .source()
            .zip(snapshot.position())
            .map(|(source, position)| {
                let duration = source
                    .end_frame_exclusive
                    .get()
                    .saturating_sub(source.start_frame.get());
                if duration == 0 {
                    0.0
                } else {
                    ((position.absolute_frame().get() - source.start_frame.get()) as f32
                        / duration as f32)
                        .clamp(0.0, 1.0)
                }
            })
            .unwrap_or(0.0);
        app.set_seek_position(seek_position);
    }
}

const fn cache_limit_index(limit_mb: u32) -> i32 {
    match limit_mb {
        0..=64 => 0,
        65..=256 => 1,
        257..=1_024 => 2,
        _ => 3,
    }
}

const fn cache_limit_mb(index: i32) -> u32 {
    match index {
        0 => 64,
        1 => 256,
        2 => 1_024,
        _ => 4_096,
    }
}

#[cfg(target_os = "windows")]
fn initialize_window(
    app: &AppWindow,
    position: Option<(i32, i32)>,
    first_start: bool,
    maximized: bool,
) {
    if first_start {
        size_first_window(app);
    } else if let Some((x, y)) = position {
        restore_window_position(app, x, y);
    }
    app.window().set_maximized(maximized);
}

#[cfg(target_os = "windows")]
fn size_first_window(app: &AppWindow) {
    use slint::winit_030::{WinitWindowAccessor, winit::dpi::PhysicalPosition};

    let target = std::cell::Cell::new(None);
    app.window().with_winit_window(|window| {
        let Some(monitor) = window
            .current_monitor()
            .or_else(|| window.primary_monitor())
            .or_else(|| window.available_monitors().next())
        else {
            return;
        };
        let origin = monitor.position();
        let size = monitor.size();
        let width = (size.width as f32 * 0.9).round() as u32;
        let height = (size.height as f32 * 0.9).round() as u32;
        let scale = window.scale_factor() as f32;
        target.set(Some((
            width as f32 / scale,
            height as f32 / scale,
            origin.x.saturating_add((size.width - width) as i32 / 2),
            origin.y.saturating_add((size.height - height) as i32 / 2),
        )));
    });
    if let Some((width, height, x, y)) = target.get() {
        app.window()
            .set_size(slint::LogicalSize::new(width, height));
        app.window().with_winit_window(|window| {
            window.set_outer_position(PhysicalPosition::new(x, y));
        });
    }
}

#[cfg(target_os = "windows")]
fn restore_window_position(app: &AppWindow, x: i32, y: i32) {
    use slint::winit_030::{WinitWindowAccessor, winit::dpi::PhysicalPosition};

    app.window().with_winit_window(|window| {
        let Some(monitor) = window.available_monitors().find(|monitor| {
            let origin = monitor.position();
            let size = monitor.size();
            x >= origin.x
                && y >= origin.y
                && x < origin.x.saturating_add(size.width as i32)
                && y < origin.y.saturating_add(size.height as i32)
        }) else {
            return;
        };
        let origin = monitor.position();
        let monitor_size = monitor.size();
        let window_size = window.outer_size();
        let max_x = origin
            .x
            .saturating_add(monitor_size.width.saturating_sub(window_size.width) as i32);
        let max_y = origin
            .y
            .saturating_add(monitor_size.height.saturating_sub(window_size.height) as i32);
        window.set_outer_position(PhysicalPosition::new(
            x.clamp(origin.x, max_x),
            y.clamp(origin.y, max_y),
        ));
    });
}
