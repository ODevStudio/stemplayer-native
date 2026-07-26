use crate::AppWindow;
use crate::studio_controller::StudioController;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use stemplayer_domain::{
    BoundedText, DeviceId, OutputPreference, PlayerCommand, PlayerController, SourceFrame,
    SourceLocator, StemRole, TransportState,
};
use stemplayer_ui::{CommandAdapter, Playhead};

pub fn wire(
    app: &AppWindow,
    quit_on_end: bool,
    settings_path: Option<std::path::PathBuf>,
    settings: &stemplayer_platform::LocalSettings,
) -> Arc<StudioController> {
    let controller = StudioController::new();
    let commands = Arc::new(CommandAdapter::new(controller.clone()));
    let epoch = Instant::now();
    let playhead = Arc::new(Mutex::new(Playhead::from_snapshot(
        &controller.latest_snapshot(),
        Duration::ZERO,
    )));
    let gain_timer = Rc::new(Timer::default());
    let (device_labels, device_ids, selected_device_index) =
        output_device_options(settings.selected_device.as_deref());
    app.set_output_devices(ModelRc::new(VecModel::from(device_labels)));
    app.set_output_device_index(selected_device_index);
    app.set_recent_sources(recent_source_model(&settings.recent_sources));
    app.set_recent_sources_enabled(settings.recent_sources_enabled);
    present(app, &controller.latest_snapshot());
    if let Some(device) = device_ids
        .get(usize::try_from(selected_device_index).unwrap_or_default())
        .cloned()
        .flatten()
    {
        let _ = controller.submit(PlayerCommand::SelectDevice(Some(device)));
    }

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_settings = settings_path.clone();
    app.window().on_winit_window_event(move |_, event| {
        if let WindowEvent::DroppedFile(path) = event {
            submit_source(&current, &weak, current_settings.as_deref(), path.clone());
        }
        EventResult::Propagate
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_settings = settings_path.clone();
    app.on_open_source(move || {
        if let Some(path) = choose_container() {
            submit_source(&current, &weak, current_settings.as_deref(), path);
        }
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_settings = settings_path.clone();
    app.on_open_manifest(move || {
        if let Some(path) = choose_manifest() {
            submit_source(&current, &weak, current_settings.as_deref(), path);
        }
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_settings = settings_path.clone();
    app.on_open_folder(move || {
        if let Some(path) = choose_folder() {
            submit_source(&current, &weak, current_settings.as_deref(), path);
        }
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_settings = settings_path.clone();
    app.on_open_recent(move |index| {
        let Some(path) = current_settings.as_deref().and_then(|settings_path| {
            usize::try_from(index).ok().and_then(|index| {
                stemplayer_platform::load_settings(settings_path)
                    .settings()
                    .recent_sources
                    .get(index)
                    .map(std::path::PathBuf::from)
            })
        }) else {
            return;
        };
        submit_source(&current, &weak, current_settings.as_deref(), path);
    });

    let weak = app.as_weak();
    let current_settings = settings_path.clone();
    app.on_clear_recent(move || {
        if let Some(settings) = update_settings(current_settings.as_deref(), |current| {
            current.without_recent_sources()
        }) && let Some(app) = weak.upgrade()
        {
            app.set_recent_sources(recent_source_model(&settings.recent_sources));
        }
    });

    let weak = app.as_weak();
    let current_settings = settings_path.clone();
    app.on_set_recent_sources_enabled(move |enabled| {
        if let Some(settings) = update_settings(current_settings.as_deref(), |current| {
            current.with_recent_sources_enabled(enabled)
        }) && let Some(app) = weak.upgrade()
        {
            app.set_recent_sources(recent_source_model(&settings.recent_sources));
        }
    });

    let current = Arc::clone(&controller);
    app.on_play(move || {
        let _ = current.submit(PlayerCommand::Play);
    });

    let current = Arc::clone(&controller);
    app.on_pause(move || {
        let _ = current.submit(PlayerCommand::Pause);
    });

    let current = Arc::clone(&controller);
    app.on_stop(move || {
        let _ = current.submit(PlayerCommand::Stop);
    });

    let current = Arc::clone(&controller);
    app.on_seek(move |fraction| {
        let snapshot = current.latest_snapshot();
        if let Some(source) = snapshot.source() {
            let duration = source
                .end_frame_exclusive
                .get()
                .saturating_sub(source.start_frame.get());
            let frame = source
                .start_frame
                .get()
                .saturating_add((fraction.clamp(0.0, 1.0) * duration as f32) as i64);
            let _ = current.submit(PlayerCommand::SeekFrame(SourceFrame::new(frame)));
        }
    });

    let commands_for_gain = Arc::clone(&commands);
    let gain_timer_for_preview = Rc::clone(&gain_timer);
    app.on_set_gain(move |index, value| {
        if let (Some(role), Ok(gain)) = (role(index), stemplayer_ui::gain_db_from_position(value)) {
            commands_for_gain.preview_gain(role, gain);
            if !gain_timer_for_preview.running() {
                let commands_for_tick = Arc::clone(&commands_for_gain);
                gain_timer_for_preview.start(
                    TimerMode::Repeated,
                    Duration::from_millis(16),
                    move || {
                        let _ = commands_for_tick.flush();
                    },
                );
            }
        }
    });

    let commands_for_commit = Arc::clone(&commands);
    let gain_timer_for_commit = Rc::clone(&gain_timer);
    app.on_commit_commands(move || {
        gain_timer_for_commit.stop();
        let _ = commands_for_commit.flush();
    });

    let current = Arc::clone(&controller);
    app.on_set_mute(move |index, muted| {
        if let Some(role) = role(index) {
            let _ = current.submit(PlayerCommand::SetMute { role, muted });
        }
    });

    let current = Arc::clone(&controller);
    app.on_set_solo(move |index, solo| {
        if let Some(role) = role(index) {
            let _ = current.submit(PlayerCommand::SetSolo { role, solo });
        }
    });

    let current = Arc::clone(&controller);
    let commands_for_reset = Arc::clone(&commands);
    let gain_timer_for_reset = Rc::clone(&gain_timer);
    app.on_reset_mix(move || {
        gain_timer_for_reset.stop();
        let _ = commands_for_reset.flush();
        let _ = current.submit(PlayerCommand::ResetMix);
    });

    let current = Arc::clone(&controller);
    app.on_set_output_preference(move |preference| {
        let preference = match preference {
            0 => OutputPreference::Automatic,
            1 => OutputPreference::MasterReference,
            _ => OutputPreference::StemMix,
        };
        let _ = current.submit(PlayerCommand::SetOutputPreference(preference));
    });

    let current = Arc::clone(&controller);
    let device_ids_for_selection = Arc::clone(&device_ids);
    app.on_select_output_device(move |index| {
        let device = usize::try_from(index)
            .ok()
            .and_then(|index| device_ids_for_selection.get(index))
            .cloned()
            .flatten();
        let _ = current.submit(PlayerCommand::SelectDevice(device.clone()));
        let _ = update_settings(settings_path.as_deref(), |current| {
            stemplayer_platform::LocalSettings {
                selected_device: device.map(|value| value.as_str().to_owned()),
                ..current.clone()
            }
        });
    });

    let current = Arc::clone(&controller);
    let device_ids_for_retry = Arc::clone(&device_ids);
    app.on_retry_device(move |index| {
        let device = usize::try_from(index)
            .ok()
            .and_then(|index| device_ids_for_retry.get(index))
            .cloned()
            .flatten();
        let _ = current.submit(PlayerCommand::SelectDevice(device));
    });

    let current = Arc::clone(&controller);
    app.on_reset_player(move || {
        let _ = current.submit(PlayerCommand::Unload);
    });

    let weak = app.as_weak();
    let visible_playhead = Arc::clone(&playhead);
    let current = Arc::clone(&controller);
    app.on_tick(move || {
        let snapshot = current.latest_snapshot();
        let Some(source) = snapshot.source() else {
            return;
        };
        let duration = source
            .end_frame_exclusive
            .get()
            .saturating_sub(source.start_frame.get());
        if duration <= 0 {
            return;
        }
        let frame = visible_playhead
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .visible_frame(epoch.elapsed());
        if let Some(app) = weak.upgrade() {
            app.set_seek_position(
                ((frame.get() - source.start_frame.get()) as f32 / duration as f32).clamp(0.0, 1.0),
            );
        }
    });

    let subscription = controller.subscribe();
    let weak = app.as_weak();
    let authoritative_playhead = Arc::clone(&playhead);
    std::thread::spawn(move || {
        while let Some(snapshot) = subscription.recv_timeout(Duration::from_secs(60)) {
            let closed = snapshot.transport_state() == TransportState::Closed;
            let finished = quit_on_end && snapshot.transport_state() == TransportState::Ended;
            let playhead = Arc::clone(&authoritative_playhead);
            if weak
                .upgrade_in_event_loop(move |app| {
                    playhead
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .update(&snapshot, epoch.elapsed());
                    present(&app, &snapshot);
                    if finished {
                        let _ = slint::quit_event_loop();
                    }
                })
                .is_err()
            {
                break;
            }
            if closed || finished {
                break;
            }
        }
    });

    controller
}

fn present(app: &AppWindow, snapshot: &stemplayer_domain::PlayerSnapshot) {
    crate::present(app, snapshot);
}

fn role(index: i32) -> Option<StemRole> {
    usize::try_from(index)
        .ok()
        .and_then(|value| StemRole::ALL.get(value).copied())
}

fn output_device_options(
    selected_device: Option<&str>,
) -> (Vec<SharedString>, Arc<[Option<DeviceId>]>, i32) {
    let devices = stemplayer_output::enumerate_devices()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|device| {
            let id = DeviceId::new(device.id).ok()?;
            let name = device.name.chars().take(96).collect::<String>();
            let label = device.default_configuration.map_or(name.clone(), |config| {
                format!("{name} ({} Hz)", config.minimum_sample_rate)
            });
            Some((SharedString::from(label), id))
        })
        .collect::<Vec<_>>();
    let labels = std::iter::once(SharedString::from("System default"))
        .chain(devices.iter().map(|(label, _)| label.clone()))
        .collect();
    let ids = std::iter::once(None)
        .chain(devices.into_iter().map(|(_, id)| Some(id)))
        .collect::<Vec<_>>();
    let selected = selected_device
        .and_then(|selected| {
            ids.iter().position(|device| {
                device
                    .as_ref()
                    .is_some_and(|device| device.as_str() == selected)
            })
        })
        .and_then(|index| i32::try_from(index).ok())
        .unwrap_or_default();
    (labels, ids.into(), selected)
}

fn submit_source(
    controller: &StudioController,
    window: &slint::Weak<AppWindow>,
    settings_path: Option<&std::path::Path>,
    path: std::path::PathBuf,
) {
    let Some(value) = path.to_str() else {
        return;
    };
    let Ok(locator) = BoundedText::new(value) else {
        return;
    };
    if controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(locator)))
        .is_err()
    {
        return;
    }
    if let Some(settings) =
        update_settings(settings_path, |current| current.with_recent_source(value))
        && let Some(app) = window.upgrade()
    {
        app.set_recent_source_index(0);
        app.set_recent_sources(recent_source_model(&settings.recent_sources));
    }
}

fn update_settings(
    path: Option<&std::path::Path>,
    update: impl FnOnce(&stemplayer_platform::LocalSettings) -> stemplayer_platform::LocalSettings,
) -> Option<stemplayer_platform::LocalSettings> {
    let path = path?;
    let settings = update(stemplayer_platform::load_settings(path).settings());
    stemplayer_platform::save_settings(path, &settings)
        .ok()
        .map(|()| settings)
}

fn recent_source_model(sources: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        sources
            .iter()
            .map(|value| recent_source_label(value))
            .collect::<Vec<_>>(),
    ))
}

fn recent_source_label(value: &str) -> SharedString {
    let path = std::path::Path::new(value);
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Local source");
    let parent = path
        .parent()
        .and_then(std::path::Path::file_name)
        .and_then(|value| value.to_str());
    parent.map_or_else(
        || SharedString::from(name.chars().take(96).collect::<String>()),
        |parent| {
            SharedString::from(
                format!("{name} ({parent})")
                    .chars()
                    .take(96)
                    .collect::<String>(),
            )
        },
    )
}

#[cfg(target_os = "windows")]
fn choose_container() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Open stem container")
        .add_filter("Stem containers", &["m4a", "mp4"])
        .pick_file()
}

#[cfg(not(target_os = "windows"))]
fn choose_container() -> Option<std::path::PathBuf> {
    std::env::var_os("STEMPLAYER_SOURCE").map(std::path::PathBuf::from)
}

#[cfg(target_os = "windows")]
fn choose_manifest() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Open Studio bundle manifest")
        .add_filter("Studio manifest", &["json"])
        .pick_file()
}

#[cfg(not(target_os = "windows"))]
fn choose_manifest() -> Option<std::path::PathBuf> {
    std::env::var_os("STEMPLAYER_SOURCE").map(std::path::PathBuf::from)
}

#[cfg(target_os = "windows")]
fn choose_folder() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Open Studio bundle folder")
        .pick_folder()
}

#[cfg(not(target_os = "windows"))]
fn choose_folder() -> Option<std::path::PathBuf> {
    std::env::var_os("STEMPLAYER_SOURCE").map(std::path::PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_device_selection_falls_back_to_default() {
        let (_, ids, selected) = output_device_options(Some("missing-device"));
        assert_eq!(selected, 0);
        assert_eq!(ids.first(), Some(&None));
    }

    #[test]
    fn recent_labels_are_bounded_and_identify_the_parent() {
        let path = std::path::Path::new("music")
            .join("set")
            .join("track.stem.m4a");
        assert_eq!(
            recent_source_label(path.to_str().unwrap()),
            "track.stem.m4a (set)"
        );
        assert!(recent_source_label(&format!("C:\\{}", "a".repeat(200))).len() <= 96);
    }
}
