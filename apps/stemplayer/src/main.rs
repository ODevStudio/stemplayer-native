#![deny(unsafe_code)]

use slint::ComponentHandle;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let mut args = std::env::args().skip(1);
    let mode = args.next();
    let duration = args.next().and_then(|value| value.parse::<u64>().ok());
    let automated = mode
        .as_deref()
        .is_some_and(|value| matches!(value, "--ui-smoke" | "--ui-idle" | "--ui-minimized"));
    let settings = stemplayer_platform::AppPaths::discover("stemplayer-native")
        .ok()
        .map(|paths| stemplayer_platform::load_settings(&paths.config.join("settings.json")))
        .map(|loaded| loaded.settings().clone())
        .unwrap_or_default();
    let app = AppWindow::new()?;
    app.set_diagnostics_visible(settings.diagnostics_visible);
    #[cfg(feature = "development")]
    let controller = wire_development(&app);
    if automated {
        #[cfg(feature = "development")]
        {
            app.invoke_open_source();
            if mode.as_deref() != Some("--ui-idle") {
                app.invoke_play();
            }
        }
        if mode.as_deref() == Some("--ui-minimized") {
            app.set_window_minimized(true);
        }
        slint::Timer::single_shot(
            std::time::Duration::from_millis(duration.unwrap_or(5_000)),
            || {
                let _ = slint::quit_event_loop();
            },
        );
    }
    app.run()?;
    #[cfg(feature = "development")]
    {
        use stemplayer_domain::{PlayerCommand, PlayerController};
        let _ = controller.submit(PlayerCommand::Shutdown);
    }
    Ok(())
}

#[cfg(feature = "development")]
fn wire_development(app: &AppWindow) -> std::sync::Arc<stemplayer_ui::MockController> {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use stemplayer_domain::{
        BoundedText, GainDb, PlayerCommand, PlayerController, SourceFrame, SourceLocator, StemRole,
    };
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
        let locator = SourceLocator::LocalPath(
            BoundedText::new("Phase 2 mock.stem.m4a").expect("static mock path is bounded"),
        );
        let _ = commands.submit(PlayerCommand::Open(locator));
        current.advance(Duration::ZERO);
        refresh(&weak, &current);
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
        if let (Some(role), Ok(gain)) = (role(index), GainDb::new(value)) {
            commands.preview_gain(role, gain);
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
    let commands = adapter.clone();
    app.on_set_solo(move |index, solo| {
        if let Some(role) = role(index) {
            let _ = commands.submit(PlayerCommand::SetSolo { role, solo });
            refresh(&weak, &current);
        }
    });

    let weak = app.as_weak();
    let current = controller.clone();
    app.on_reset_mix(move || {
        let _ = current.submit(PlayerCommand::ResetMix);
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
fn refresh(window: &slint::Weak<AppWindow>, controller: &stemplayer_ui::MockController) {
    use stemplayer_domain::PlayerController;
    if let Some(app) = window.upgrade() {
        present(&app, &controller.latest_snapshot());
    }
}

#[cfg(feature = "development")]
fn present(app: &AppWindow, snapshot: &stemplayer_domain::PlayerSnapshot) {
    let model = stemplayer_ui::PresentationModel::from_snapshot(snapshot);
    app.set_source_name(model.file_name.into());
    app.set_source_summary(model.source_summary.into());
    app.set_state_text(model.state_text.into());
    app.set_time_text(format!("{} / {}", model.current_time, model.total_time).into());
    app.set_output_mode(model.output_mode.into());
    app.set_device_name(model.device_name.into());
    app.set_waveform_text(model.waveform_text.into());
    app.set_error_text(model.error_banner.unwrap_or_default().into());
    app.set_can_open(model.transport.open);
    app.set_can_play(model.transport.play);
    app.set_can_pause(model.transport.pause);
    app.set_can_stop(model.transport.stop);
    app.set_can_seek(model.transport.seek);
    app.set_can_mix(model.transport.mixer);
    app.set_drums_gain(model.mixer_rows[0].gain_db);
    app.set_bass_gain(model.mixer_rows[1].gain_db);
    app.set_other_gain(model.mixer_rows[2].gain_db);
    app.set_vocals_gain(model.mixer_rows[3].gain_db);
    app.set_drums_muted(model.mixer_rows[0].muted);
    app.set_bass_muted(model.mixer_rows[1].muted);
    app.set_other_muted(model.mixer_rows[2].muted);
    app.set_vocals_muted(model.mixer_rows[3].muted);
    app.set_drums_soloed(model.mixer_rows[0].solo);
    app.set_bass_soloed(model.mixer_rows[1].solo);
    app.set_other_soloed(model.mixer_rows[2].solo);
    app.set_vocals_soloed(model.mixer_rows[3].solo);
}
