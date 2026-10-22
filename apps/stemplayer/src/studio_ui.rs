use crate::playlist::Playlist;
use crate::settings::SettingsStore;
use crate::studio_controller::{SourceSnapshot, SourceSnapshotSubscription, StudioController};
use crate::{AppWindow, PlaylistRow};
use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use stemplayer_domain::{
    BoundedText, CacheCommand, DeviceId, ErrorCategory, OutputPreference, PlayerCommand,
    PlayerController, PlayerError, PlayerSnapshot, Severity, SourceFrame, SourceLocator, StemRole,
    TransportState,
};
use stemplayer_ui::{CommandAdapter, Playhead};

mod diagnostics;
mod discovery;
mod recovery;

const CACHE_LIMITS_MB: [u32; 4] = [64, 256, 1_024, 4_096];
const COLLAPSED_WINDOW_HEIGHT: f32 = 298.0;
const NEXT_SOURCE_LEAD_TIME: Duration = Duration::from_secs(5);

pub(crate) fn wire(
    app: &AppWindow,
    quit_on_end: bool,
    settings_store: SettingsStore,
    restore_playlist: bool,
) -> Arc<StudioController> {
    let settings = settings_store.snapshot();
    let cache = stemplayer_platform::AppPaths::discover("stemplayer-native")
        .ok()
        .and_then(|paths| {
            stemplayer_waveform::WaveformCache::new(
                paths.cache.join("waveforms-v1"),
                stemplayer_waveform::CacheConfig {
                    capacity_bytes: u64::from(settings.waveform_cache_limit_mb) * 1024 * 1024,
                    ..stemplayer_waveform::CacheConfig::default()
                },
            )
            .ok()
        })
        .map(Arc::new);
    let controller = StudioController::with_cache(cache, settings.waveform_cache_enabled);
    let commands = Arc::new(CommandAdapter::new(controller.clone()));
    let epoch = Instant::now();
    let playhead = Arc::new(Mutex::new(Playhead::from_snapshot(
        &controller.latest_snapshot(),
        Duration::ZERO,
    )));
    let gain_timer = Rc::new(Timer::default());
    let drop_timer = Rc::new(Timer::default());
    let dropped_paths = Rc::new(RefCell::new(Vec::new()));
    let playlist = Arc::new(Mutex::new(Playlist::default()));
    let playlist_restore_cancelled = Arc::new(AtomicBool::new(false));
    let discovery_requests = discovery::Requests::default();
    let collapsed_window_height = Rc::new(Cell::new(COLLAPSED_WINDOW_HEIGHT));
    let (labels, ids, selected) =
        output_device_options_from(Vec::new(), settings.selected_device.as_deref());
    if let Some(device) = ids
        .get(usize::try_from(selected).unwrap_or_default())
        .cloned()
        .flatten()
    {
        let _ = controller.submit(PlayerCommand::SelectDevice(Some(device)));
    }
    let device_ids = Arc::new(RwLock::new(ids));
    app.set_output_devices(ModelRc::new(VecModel::from(labels)));
    app.set_output_device_index(selected);
    app.set_recent_sources(recent_source_model(&settings.recent_sources));
    app.set_recent_sources_enabled(settings.recent_sources_enabled);
    app.set_waveform_cache_enabled(settings.waveform_cache_enabled);
    app.set_waveform_cache_limit_index(
        CACHE_LIMITS_MB
            .iter()
            .position(|value| *value == settings.waveform_cache_limit_mb)
            .and_then(|index| i32::try_from(index).ok())
            .unwrap_or(1),
    );
    app.set_playlist_items(playlist_model(&[]));
    app.set_playlist_index(-1);
    present(app, &controller.latest_snapshot());
    present_waveform(app, controller.latest_waveform().as_ref());
    let weak = app.as_weak();
    let current_device_ids = Arc::clone(&device_ids);
    let selected_device = settings.selected_device.clone();
    std::thread::spawn(move || {
        let (labels, ids, selected) = output_device_options(selected_device.as_deref());
        let _ = weak.upgrade_in_event_loop(move |app| {
            let previous = usize::try_from(app.get_output_device_index())
                .ok()
                .and_then(|index| {
                    current_device_ids
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .get(index)
                        .cloned()
                });
            let selected = previous
                .and_then(|previous| ids.iter().position(|device| *device == previous))
                .and_then(|index| i32::try_from(index).ok())
                .unwrap_or(selected);
            *current_device_ids
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::clone(&ids);
            app.set_output_devices(ModelRc::new(VecModel::from(labels)));
            app.set_output_device_index(selected);
        });
    });
    if restore_playlist && !settings.playlist_sources.is_empty() {
        let sources = settings.playlist_sources.clone();
        let selected = settings.playlist_selected_source.clone();
        let current_playlist = Arc::clone(&playlist);
        let restore_cancelled = Arc::clone(&playlist_restore_cancelled);
        let current = Arc::clone(&controller);
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let restored = Playlist::restore(&sources, selected.as_deref());
            let _ = weak.upgrade_in_event_loop(move |app| {
                if restore_cancelled.load(Ordering::Acquire) {
                    return;
                }
                let (rows, index, source) = {
                    let mut playlist = current_playlist
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *playlist = restored;
                    let source = playlist.activate_selected();
                    (
                        playlist.rows(),
                        playlist
                            .selected()
                            .and_then(|index| i32::try_from(index).ok())
                            .unwrap_or(-1),
                        source,
                    )
                };
                app.set_playlist_items(playlist_model(&rows));
                app.set_playlist_index(index);
                if let Some(source) = source {
                    submit_playlist_source(current.as_ref(), source, false);
                }
            });
        });
    }

    let weak = app.as_weak();
    let current_playlist = Arc::clone(&playlist);
    let current_controller = Arc::clone(&controller);
    let current_drop_timer = Rc::clone(&drop_timer);
    let current_dropped_paths = Rc::clone(&dropped_paths);
    let current_playlist_restore_cancelled = Arc::clone(&playlist_restore_cancelled);
    let current_discovery_requests = discovery_requests.clone();
    let current_collapsed_window_height = Rc::clone(&collapsed_window_height);
    let current_settings = settings_store.clone();
    app.window().on_winit_window_event(move |window, event| {
        match event {
            WindowEvent::DroppedFile(path) => {
                current_playlist_restore_cancelled.store(true, Ordering::Release);
                current_dropped_paths.borrow_mut().push(path.clone());
                current_drop_timer.stop();
                let paths = Rc::clone(&current_dropped_paths);
                let playlist = Arc::clone(&current_playlist);
                let weak = weak.clone();
                let controller = Arc::clone(&current_controller);
                let settings_store = current_settings.clone();
                let request = current_discovery_requests.begin();
                current_drop_timer.start(
                    TimerMode::SingleShot,
                    Duration::from_millis(75),
                    move || {
                        discovery::start(
                            std::mem::take(&mut *paths.borrow_mut()),
                            Arc::clone(&playlist),
                            weak.clone(),
                            controller.clone(),
                            settings_store.clone(),
                            Arc::clone(&request),
                        );
                    },
                );
            }
            WindowEvent::Focused(true) | WindowEvent::Occluded(false) => {
                if let Some(app) = weak.upgrade() {
                    let _ = app.window().take_snapshot();
                }
                window.request_redraw();
            }
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                if !window.is_maximized() {
                    let size = size.to_logical::<f32>(window.scale_factor().into());
                    if let Some(app) = weak.upgrade() {
                        if app.get_playlist_expanded() {
                            app.set_playlist_expanded_window_height(size.height);
                        } else {
                            current_collapsed_window_height.set(size.height);
                        }
                    }
                }
                window.request_redraw();
            }
            _ => {}
        }
        EventResult::Propagate
    });

    let weak = app.as_weak();
    let current_collapsed_window_height = Rc::clone(&collapsed_window_height);
    app.on_playlist_expansion_changed(move |expanded| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if app.window().is_maximized() {
            return;
        }
        let current = app.window().size().to_logical(app.window().scale_factor());
        let (expanded_height, collapsed_height, target_height) = playlist_height_transition(
            expanded,
            current.height,
            app.get_playlist_expanded_window_height(),
            current_collapsed_window_height.get(),
        );
        app.set_playlist_expanded_window_height(expanded_height);
        current_collapsed_window_height.set(collapsed_height);
        let weak = app.as_weak();
        Timer::single_shot(Duration::ZERO, move || {
            if let Some(app) = weak.upgrade()
                && !app.window().is_maximized()
            {
                app.window()
                    .set_size(slint::LogicalSize::new(current.width, target_height));
            }
        });
    });

    let weak = app.as_weak();
    let current_playlist = Arc::clone(&playlist);
    let current = Arc::clone(&controller);
    let current_settings = settings_store.clone();
    let current_playlist_restore_cancelled = Arc::clone(&playlist_restore_cancelled);
    let current_discovery_requests = discovery_requests.clone();
    app.on_open_folder(move || {
        discovery::spawn_picker(
            Arc::clone(&current_playlist),
            weak.clone(),
            Arc::clone(&current),
            current_settings.clone(),
            &current_playlist_restore_cancelled,
            &current_discovery_requests,
        );
    });

    let weak = app.as_weak();
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_store.clone();
    let current_discovery_requests = discovery_requests.clone();
    app.on_select_playlist_item(move |index, control, shift| {
        let changed = usize::try_from(index).is_ok_and(|index| {
            current_playlist
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .select(index, control, shift)
        });
        if changed {
            current_discovery_requests.cancel();
            present_playlist(&weak, &current_playlist);
            persist_playlist(&current_settings, &current_playlist);
        }
    });

    let weak = app.as_weak();
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_store.clone();
    let current_discovery_requests = discovery_requests.clone();
    app.on_navigate_playlist_item(move |index, control, shift| {
        let changed = usize::try_from(index).is_ok_and(|index| {
            current_playlist
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .navigate(index, control, shift)
        });
        if changed {
            current_discovery_requests.cancel();
            present_playlist(&weak, &current_playlist);
            persist_playlist(&current_settings, &current_playlist);
        }
    });

    let weak = app.as_weak();
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_store.clone();
    let current_discovery_requests = discovery_requests.clone();
    app.on_context_select_playlist_item(move |index| {
        let changed = usize::try_from(index).is_ok_and(|index| {
            current_playlist
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .select_context(index)
        });
        if changed {
            current_discovery_requests.cancel();
            present_playlist(&weak, &current_playlist);
            persist_playlist(&current_settings, &current_playlist);
        }
    });

    let weak = app.as_weak();
    let current_playlist = Arc::clone(&playlist);
    let current_discovery_requests = discovery_requests.clone();
    app.on_select_all_playlist_items(move || {
        current_discovery_requests.cancel();
        current_playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .select_all();
        present_playlist(&weak, &current_playlist);
    });

    let weak = app.as_weak();
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_store.clone();
    let current_discovery_requests = discovery_requests.clone();
    app.on_remove_playlist_items(move || {
        let changed = current_playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove_selected();
        if changed {
            current_discovery_requests.cancel();
            present_playlist(&weak, &current_playlist);
            persist_playlist(&current_settings, &current_playlist);
        }
    });

    let weak = app.as_weak();
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_store.clone();
    let current_discovery_requests = discovery_requests.clone();
    app.on_reorder_playlist_item(move |from, to| {
        let changed = usize::try_from(from)
            .ok()
            .zip(usize::try_from(to).ok())
            .is_some_and(|(from, to)| {
                current_playlist
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .move_item(from, to)
            });
        if changed {
            current_discovery_requests.cancel();
            present_playlist(&weak, &current_playlist);
            persist_playlist(&current_settings, &current_playlist);
        }
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_store.clone();
    let current_discovery_requests = discovery_requests.clone();
    app.on_activate_playlist_item(move |index| {
        let path = usize::try_from(index).ok().and_then(|index| {
            current_playlist
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .activate(index)
        });
        if let Some(path) = path {
            current_discovery_requests.cancel();
            present_playlist(&weak, &current_playlist);
            persist_playlist(&current_settings, &current_playlist);
            submit_playlist_source(current.as_ref(), path, true);
        }
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_store.clone();
    let current_discovery_requests = discovery_requests.clone();
    app.on_previous_track(move || {
        let path = current_playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .previous()
            .map(|(_, path)| path);
        if let Some(path) = path {
            current_discovery_requests.cancel();
            present_playlist(&weak, &current_playlist);
            persist_playlist(&current_settings, &current_playlist);
            submit_playlist_source(current.as_ref(), path, true);
        }
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_store.clone();
    let current_discovery_requests = discovery_requests.clone();
    app.on_next_track(move || {
        let path = current_playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .next()
            .map(|(_, path)| path);
        if let Some(path) = path {
            current_discovery_requests.cancel();
            present_playlist(&weak, &current_playlist);
            persist_playlist(&current_settings, &current_playlist);
            submit_playlist_source(current.as_ref(), path, true);
        }
    });

    let current_playlist = Arc::clone(&playlist);
    let current_playlist_restore_cancelled = Arc::clone(&playlist_restore_cancelled);
    app.on_shuffle_changed(move |enabled| {
        current_playlist_restore_cancelled.store(true, Ordering::Release);
        current_playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_shuffle(enabled);
    });

    let current_playlist = Arc::clone(&playlist);
    let current_playlist_restore_cancelled = Arc::clone(&playlist_restore_cancelled);
    app.on_repeat_changed(move |enabled| {
        current_playlist_restore_cancelled.store(true, Ordering::Release);
        current_playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_repeat(enabled);
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_settings = settings_store.clone();
    let current_playlist_restore_cancelled = Arc::clone(&playlist_restore_cancelled);
    let current_discovery_requests = discovery_requests.clone();
    app.on_open_url(move |url| {
        current_playlist_restore_cancelled.store(true, Ordering::Release);
        current_discovery_requests.cancel();
        submit_url(&current, &weak, &current_settings, url.as_str());
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_settings = settings_store.clone();
    let current_playlist_restore_cancelled = Arc::clone(&playlist_restore_cancelled);
    let current_discovery_requests = discovery_requests.clone();
    app.on_open_recent(move |index| {
        let Some(source) = usize::try_from(index).ok().and_then(|index| {
            current_settings
                .snapshot()
                .recent_sources
                .get(index)
                .cloned()
        }) else {
            return;
        };
        current_playlist_restore_cancelled.store(true, Ordering::Release);
        current_discovery_requests.cancel();
        if crate::recent_sources::is_url(&source) {
            submit_url(&current, &weak, &current_settings, &source);
        } else {
            submit_source(&current, &weak, &current_settings, source.into());
        }
    });

    let weak = app.as_weak();
    let current_settings = settings_store.clone();
    app.on_clear_recent(move || {
        if let Some(settings) = current_settings.update(|current| current.without_recent_sources())
            && let Some(app) = weak.upgrade()
        {
            app.set_recent_sources(recent_source_model(&settings.recent_sources));
        }
    });

    let weak = app.as_weak();
    let current_settings = settings_store.clone();
    app.on_set_recent_sources_enabled(move |enabled| {
        if let Some(settings) =
            current_settings.update(|current| current.with_recent_sources_enabled(enabled))
            && let Some(app) = weak.upgrade()
        {
            app.set_recent_sources(recent_source_model(&settings.recent_sources));
        }
    });

    let current = Arc::clone(&controller);
    let current_settings = settings_store.clone();
    app.on_set_waveform_cache_enabled(move |enabled| {
        let command = if enabled {
            CacheCommand::Enable
        } else {
            CacheCommand::Disable
        };
        let _ = current.submit(PlayerCommand::Cache(command));
        let _ = current_settings.update(|current| stemplayer_platform::LocalSettings {
            waveform_cache_enabled: enabled,
            ..current.clone()
        });
    });

    let current = Arc::clone(&controller);
    let current_settings = settings_store.clone();
    app.on_set_waveform_cache_limit(move |index| {
        let Some(limit_mb) = usize::try_from(index)
            .ok()
            .and_then(|index| CACHE_LIMITS_MB.get(index))
            .copied()
        else {
            return;
        };
        let _ = current.submit(PlayerCommand::Cache(CacheCommand::SetCapacityBytes(
            u64::from(limit_mb) * 1024 * 1024,
        )));
        let _ = current_settings.update(|current| stemplayer_platform::LocalSettings {
            waveform_cache_limit_mb: limit_mb,
            ..current.clone()
        });
    });

    let current = Arc::clone(&controller);
    app.on_clear_current_waveform_cache(move || {
        let _ = current.submit(PlayerCommand::Cache(CacheCommand::ClearCurrent));
    });

    let current = Arc::clone(&controller);
    app.on_clear_waveform_cache(move || {
        let _ = current.submit(PlayerCommand::Cache(CacheCommand::ClearAll));
    });

    let current = Arc::clone(&controller);
    app.on_clear_network_cache(move || {
        let _ = current.submit(PlayerCommand::Cache(CacheCommand::ClearNetworkSource));
    });

    diagnostics::wire(app, Arc::clone(&controller));

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_playlist = Arc::clone(&playlist);
    let current_discovery_requests = discovery_requests.clone();
    app.on_play(move || {
        current_discovery_requests.cancel();
        let snapshot = current.latest_snapshot();
        let path = activate_playlist_for_play(&weak, &current_playlist, &snapshot);
        if let Some(path) = path {
            submit_playlist_source(current.as_ref(), path, true);
        } else {
            let _ = current.submit(PlayerCommand::Play);
        }
    });

    let current = Arc::clone(&controller);
    app.on_pause(move || {
        let _ = current.submit(PlayerCommand::Pause);
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
        if let Ok(gain) = stemplayer_ui::gain_db_from_position(value) {
            if index == -1 {
                for role in StemRole::ALL {
                    commands_for_gain.preview_gain(role, gain);
                }
            } else if let Some(role) = role(index) {
                commands_for_gain.preview_gain(role, gain);
            } else {
                return;
            }
            schedule_gain_flush(&gain_timer_for_preview, Arc::clone(&commands_for_gain));
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
    let current_settings = settings_store.clone();
    app.on_select_output_device(move |index| {
        let Some(device) = usize::try_from(index).ok().and_then(|index| {
            device_ids_for_selection
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(index)
                .cloned()
        }) else {
            return;
        };
        let _ = current.submit(PlayerCommand::SelectDevice(device.clone()));
        let _ = current_settings.update(|current| stemplayer_platform::LocalSettings {
            selected_device: device.map(|value| value.as_str().to_owned()),
            ..current.clone()
        });
    });

    let current = Arc::clone(&controller);
    let device_ids_for_retry = Arc::clone(&device_ids);
    app.on_retry_device(move |index| {
        let Some(device) = usize::try_from(index).ok().and_then(|index| {
            device_ids_for_retry
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(index)
                .cloned()
        }) else {
            return;
        };
        let _ = current.submit(PlayerCommand::SelectDevice(device));
    });

    let current = Arc::clone(&controller);
    let current_playlist_restore_cancelled = Arc::clone(&playlist_restore_cancelled);
    let current_discovery_requests = discovery_requests.clone();
    app.on_reset_player(move || {
        current_playlist_restore_cancelled.store(true, Ordering::Release);
        current_discovery_requests.cancel();
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

    let subscription = controller.subscribe_source_snapshots();
    let weak = app.as_weak();
    let authoritative_playhead = Arc::clone(&playhead);
    let waveform_controller = Arc::clone(&controller);
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_store.clone();
    std::thread::spawn(move || {
        let mut presented_waveform =
            waveform_revision(waveform_controller.latest_waveform().as_ref());
        let mut previous_state = None;
        let mut reported_notice = None;
        loop {
            let evidence = receive_snapshot(&subscription, Duration::from_secs(60));
            let snapshot = evidence.snapshot().clone();
            if snapshot.generation() != waveform_controller.latest_snapshot().generation() {
                continue;
            }
            let closed = snapshot.transport_state() == TransportState::Closed;
            let finished = quit_on_end && snapshot.transport_state() == TransportState::Ended;
            let (current_playlist_source, upcoming_source) = {
                let mut playlist = current_playlist
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let active = playlist.active_source();
                let current_playlist_source = active.as_deref().is_some_and(|path| {
                    waveform_controller.current_source_is(path)
                        && waveform_controller.latest_snapshot().generation()
                            == snapshot.generation()
                });
                let upcoming = (current_playlist_source && should_prepare_next(&snapshot))
                    .then(|| playlist.upcoming_source())
                    .flatten();
                (current_playlist_source, upcoming)
            };
            if let Some(source) = upcoming_source {
                let _ = waveform_controller.prepare_source(source, snapshot.generation());
            }
            let notice_revision = snapshot
                .details()
                .error
                .as_ref()
                .map(|error| (snapshot.generation(), error.code, error.severity));
            let notice = source_notice(&snapshot).filter(|_| notice_revision != reported_notice);
            if notice.is_some() {
                reported_notice = notice_revision;
            }
            let source_failed =
                notice.is_some() && snapshot.transport_state() == TransportState::Error;
            let ended = !quit_on_end
                && current_playlist_source
                && recovery::is_new_end(previous_state, &snapshot);
            let recovered = if source_failed || ended {
                let mut playlist = current_playlist
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if waveform_controller.latest_snapshot().generation() != snapshot.generation() {
                    false
                } else {
                    let playlist_source = playlist
                        .active_source()
                        .as_deref()
                        .is_some_and(|path| waveform_controller.current_source_is(path));
                    let play = !source_failed || waveform_controller.play_requested();
                    recovery::admit_playlist_recovery(
                        &mut playlist,
                        playlist_source,
                        source_failed,
                        play,
                        |source, play| {
                            waveform_controller.submit_source_recovery(&evidence, source, play)
                        },
                    )
                    .unwrap_or(false)
                }
            } else {
                false
            };
            previous_state = Some((snapshot.generation(), snapshot.transport_state()));
            if recovered {
                persist_playlist(&current_settings, &current_playlist);
            }
            let presented_playlist = Arc::clone(&current_playlist);
            let playhead = Arc::clone(&authoritative_playhead);
            let waveform = waveform_controller.latest_waveform();
            let revision = waveform_revision(waveform.as_ref());
            let waveform_changed = revision != presented_waveform;
            presented_waveform = revision;
            if weak
                .upgrade_in_event_loop(move |app| {
                    playhead
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .update(&snapshot, epoch.elapsed());
                    present(&app, &source_notice_snapshot(&snapshot));
                    if let Some(notice) = notice {
                        show_source_notice(&app, notice);
                    }
                    if waveform_changed {
                        present_waveform(&app, waveform.as_ref());
                    }
                    if recovered {
                        present_playlist(&app.as_weak(), &presented_playlist);
                    }
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

fn receive_snapshot(
    subscription: &SourceSnapshotSubscription,
    timeout: Duration,
) -> SourceSnapshot {
    loop {
        if let Some(snapshot) = subscription.recv_timeout(timeout) {
            return snapshot;
        }
    }
}

fn playlist_height_transition(
    expanded: bool,
    current_height: f32,
    expanded_height: f32,
    collapsed_height: f32,
) -> (f32, f32, f32) {
    if expanded {
        (expanded_height, current_height, expanded_height)
    } else {
        (current_height, collapsed_height, collapsed_height)
    }
}

fn present_waveform(app: &AppWindow, waveform: Option<&stemplayer_waveform::WaveformSnapshot>) {
    let rows = waveform
        .and_then(|snapshot| {
            let start = snapshot.start_frame.get();
            stemplayer_waveform::project_viewport(
                snapshot,
                start,
                snapshot.end_frame_exclusive.get(),
                800,
            )
            .ok()
        })
        .map(|geometry| geometry.rows)
        .unwrap_or_else(|| std::array::from_fn(|_| Vec::new()));
    let band_scales = [1usize, 2, 3].map(|index| {
        rows[index]
            .iter()
            .map(|peak| peak.maximum.max(0.0))
            .fold(f32::EPSILON, f32::max)
    });
    let colors = rows[0]
        .iter()
        .enumerate()
        .map(|(index, _)| {
            frequency_color(
                rows[1]
                    .get(index)
                    .map_or(0.0, |peak| peak.maximum.max(0.0) / band_scales[0]),
                rows[2]
                    .get(index)
                    .map_or(0.0, |peak| peak.maximum.max(0.0) / band_scales[1]),
                rows[3]
                    .get(index)
                    .map_or(0.0, |peak| peak.maximum.max(0.0) / band_scales[2]),
            )
        })
        .collect::<Vec<_>>();
    let [
        (drums_min, drums_max),
        (bass_min, bass_max),
        (other_min, other_max),
        (vocals_min, vocals_max),
    ] = rows.map(|row| {
        let minimum = row.iter().map(|peak| peak.minimum).collect::<Vec<_>>();
        let maximum = row.iter().map(|peak| peak.maximum).collect::<Vec<_>>();
        (
            ModelRc::new(VecModel::from(minimum)),
            ModelRc::new(VecModel::from(maximum)),
        )
    });
    app.set_drums_wave_min(drums_min);
    app.set_drums_wave_max(drums_max);
    app.set_bass_wave_min(bass_min);
    app.set_bass_wave_max(bass_max);
    app.set_other_wave_min(other_min);
    app.set_other_wave_max(other_max);
    app.set_vocals_wave_min(vocals_min);
    app.set_vocals_wave_max(vocals_max);
    app.set_ordinary_wave_colors(ModelRc::new(VecModel::from(colors)));
}

fn frequency_color(low: f32, mid: f32, high: f32) -> Color {
    let low = low.max(0.0);
    let mid = mid.max(0.0) * 1.15;
    let high = high.max(0.0) * 1.8;
    let total = low + mid + high;
    if total <= f32::EPSILON {
        return Color::from_rgb_u8(0, 120, 145);
    }

    let low_weight = (low / total).powf(1.8);
    let mid_weight = (mid / total).powf(1.8);
    let high_weight = (high / total).powf(1.8);
    let weight_total = low_weight + mid_weight + high_weight;
    let spectral_position =
        ((((mid_weight * 0.5 + high_weight) / weight_total) - 0.5) * 1.8 + 0.5).clamp(0.0, 1.0);
    let rgb = spectral_palette(spectral_position);
    let energy = (total * 2.0).sqrt().clamp(0.0, 1.0);
    let brightness = 0.72 + energy * 0.28;

    Color::from_rgb_u8(
        (rgb[0] * brightness).round() as u8,
        (rgb[1] * brightness).round() as u8,
        (rgb[2] * brightness).round() as u8,
    )
}

fn source_notice(snapshot: &PlayerSnapshot) -> Option<String> {
    snapshot
        .details()
        .error
        .as_ref()
        .filter(|error| {
            error.severity != Severity::Fatal
                && (error.severity == Severity::Warning
                    || matches!(
                        error.category,
                        ErrorCategory::Source | ErrorCategory::Timeline | ErrorCategory::Network
                    ))
        })
        .map(|error| {
            if snapshot.transport_state() == TransportState::Error {
                format!("Track unavailable: {}", error.summary)
            } else {
                error.summary.to_string()
            }
        })
}

fn source_notice_snapshot(snapshot: &PlayerSnapshot) -> PlayerSnapshot {
    if source_notice(snapshot).is_none() {
        return snapshot.clone();
    }
    let mut details = snapshot.details().clone();
    details.error = None;
    snapshot.with_details(details)
}

fn show_source_notice(app: &AppWindow, notice: String) {
    let notice = SharedString::from(notice);
    app.set_skip_notice(notice.clone());
    let weak = app.as_weak();
    Timer::single_shot(Duration::from_secs(4), move || {
        if let Some(app) = weak.upgrade()
            && app.get_skip_notice() == notice
        {
            app.set_skip_notice("".into());
        }
    });
}

fn spectral_palette(position: f32) -> [f32; 3] {
    if position < 0.22 {
        mix_color([255.0, 72.0, 47.0], [255.0, 45.0, 130.0], position / 0.22)
    } else if position < 0.48 {
        mix_color(
            [255.0, 45.0, 130.0],
            [185.0, 70.0, 255.0],
            (position - 0.22) / 0.26,
        )
    } else if position < 0.72 {
        mix_color(
            [185.0, 70.0, 255.0],
            [57.0, 125.0, 255.0],
            (position - 0.48) / 0.24,
        )
    } else {
        mix_color(
            [57.0, 125.0, 255.0],
            [0.0, 205.0, 255.0],
            (position - 0.72) / 0.28,
        )
    }
}

fn mix_color(start: [f32; 3], end: [f32; 3], position: f32) -> [f32; 3] {
    [
        start[0] + (end[0] - start[0]) * position,
        start[1] + (end[1] - start[1]) * position,
        start[2] + (end[2] - start[2]) * position,
    ]
}

fn waveform_revision(
    waveform: Option<&stemplayer_waveform::WaveformSnapshot>,
) -> Option<(stemplayer_domain::GenerationId, SourceFrame, u32)> {
    waveform.map(|snapshot| {
        (
            snapshot.generation,
            snapshot.analyzed_end_frame,
            snapshot.progress.to_bits(),
        )
    })
}

fn should_prepare_next(snapshot: &PlayerSnapshot) -> bool {
    if snapshot.transport_state() != TransportState::Playing {
        return false;
    }
    let (Some(source), Some(position)) = (snapshot.source(), snapshot.position()) else {
        return false;
    };
    let lead_frames = i64::from(source.sample_rate.get()) * NEXT_SOURCE_LEAD_TIME.as_secs() as i64;
    source
        .end_frame_exclusive
        .get()
        .saturating_sub(position.absolute_frame().get())
        <= lead_frames
}

fn present(app: &AppWindow, snapshot: &stemplayer_domain::PlayerSnapshot) {
    crate::present(app, snapshot);
}

fn role(index: i32) -> Option<StemRole> {
    usize::try_from(index)
        .ok()
        .and_then(|value| StemRole::ALL.get(value).copied())
}

fn schedule_gain_flush(timer: &Timer, commands: Arc<CommandAdapter>) {
    if !timer.running() {
        timer.start(
            TimerMode::SingleShot,
            Duration::from_millis(16),
            move || {
                let _ = commands.flush();
            },
        );
    }
}

fn output_device_options(
    selected_device: Option<&str>,
) -> (Vec<SharedString>, Arc<[Option<DeviceId>]>, i32) {
    output_device_options_from(
        stemplayer_output::enumerate_devices().unwrap_or_default(),
        selected_device,
    )
}

fn output_device_options_from(
    devices: Vec<stemplayer_output::DeviceReport>,
    selected_device: Option<&str>,
) -> (Vec<SharedString>, Arc<[Option<DeviceId>]>, i32) {
    let devices = devices
        .into_iter()
        .filter(|device| device.stereo_supported)
        .filter_map(|device| {
            let id = DeviceId::new(device.id).ok()?;
            let name = device.name.chars().take(96).collect::<String>();
            let label = device.default_configuration.map_or(name.clone(), |config| {
                format!("{name} ({} Hz)", config.minimum_sample_rate)
            });
            Some((SharedString::from(label), id))
        })
        .collect::<Vec<_>>();
    let mut labels: Vec<_> = std::iter::once(SharedString::from("System default"))
        .chain(devices.iter().map(|(label, _)| label.clone()))
        .collect();
    let mut ids = std::iter::once(None)
        .chain(devices.into_iter().map(|(_, id)| Some(id)))
        .collect::<Vec<_>>();
    let selected = selected_device.map_or(0, |selected| {
        ids.iter()
            .position(|device| {
                device
                    .as_ref()
                    .is_some_and(|device| device.as_str() == selected)
            })
            .unwrap_or_else(|| {
                let device = DeviceId::new(selected)
                    .unwrap_or_else(|_| DeviceId::truncated("unavailable-saved-output"));
                labels.push("Saved output unavailable".into());
                ids.push(Some(device));
                ids.len() - 1
            })
    });
    let selected = i32::try_from(selected).unwrap_or(-1);
    (labels, ids.into(), selected)
}

fn submit_source(
    controller: &StudioController,
    window: &slint::Weak<AppWindow>,
    settings_store: &SettingsStore,
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
    if let Some(settings) = settings_store.update(|current| current.with_recent_source(value))
        && let Some(app) = window.upgrade()
    {
        app.set_recent_source_index(0);
        app.set_recent_sources(recent_source_model(&settings.recent_sources));
    }
}

fn playlist_error_snapshot(
    snapshot: stemplayer_domain::PlayerSnapshot,
    error: PlayerError,
) -> stemplayer_domain::PlayerSnapshot {
    let mut details = snapshot.details().clone();
    details.error = Some(error);
    snapshot.with_details(details)
}

fn submit_playlist_source(controller: &dyn PlayerController, path: std::path::PathBuf, play: bool) {
    let Some(value) = path.to_str() else {
        return;
    };
    let Ok(locator) = BoundedText::new(value) else {
        return;
    };
    if controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(locator)))
        .is_ok()
        && play
    {
        let _ = controller.submit(PlayerCommand::Play);
    }
}

fn submit_url(
    controller: &StudioController,
    window: &slint::Weak<AppWindow>,
    settings_store: &SettingsStore,
    value: &str,
) {
    let value = match crate::recent_sources::validated_url(value) {
        Ok(value) => value,
        Err(error) => {
            if let Some(app) = window.upgrade() {
                show_source_notice(&app, error.summary.to_string());
            }
            return;
        }
    };
    let Ok(locator) = BoundedText::new(&value) else {
        return;
    };
    if controller
        .submit(PlayerCommand::Open(SourceLocator::HlsUrl(locator)))
        .is_err()
    {
        return;
    }
    if let Some(settings) = settings_store.update(|current| current.with_recent_source(&value))
        && let Some(app) = window.upgrade()
    {
        app.set_recent_source_index(0);
        app.set_recent_sources(recent_source_model(&settings.recent_sources));
    }
}

fn persist_playlist(settings: &SettingsStore, playlist: &Arc<Mutex<Playlist>>) {
    let playlist = playlist
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = settings
        .update(|settings| settings.with_playlist(playlist.sources(), playlist.selected_source()));
}

fn recent_source_model(sources: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        sources
            .iter()
            .map(|value| recent_source_label(value))
            .collect::<Vec<_>>(),
    ))
}

fn present_playlist(window: &slint::Weak<AppWindow>, playlist: &Arc<Mutex<Playlist>>) {
    let (rows, selected) = {
        let playlist = playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (
            playlist.rows(),
            playlist
                .selected()
                .and_then(|index| i32::try_from(index).ok())
                .unwrap_or(-1),
        )
    };
    if let Some(app) = window.upgrade() {
        let model = app.get_playlist_items();
        if model.row_count() == rows.len() {
            for (index, (label, selected)) in rows.iter().enumerate() {
                model.set_row_data(
                    index,
                    PlaylistRow {
                        label: label.into(),
                        selected: *selected,
                    },
                );
            }
        } else {
            app.set_playlist_items(playlist_model(&rows));
        }
        app.set_playlist_index(selected);
    }
}

fn activate_playlist_for_play(
    window: &slint::Weak<AppWindow>,
    playlist: &Arc<Mutex<Playlist>>,
    snapshot: &PlayerSnapshot,
) -> Option<std::path::PathBuf> {
    let source_failed = snapshot.transport_state() == TransportState::Error
        && snapshot.details().error.as_ref().is_some_and(|error| {
            matches!(error.severity, Severity::Error | Severity::Fatal)
                && matches!(
                    error.category,
                    ErrorCategory::Source | ErrorCategory::Timeline | ErrorCategory::Network
                )
        });
    let source_loaded = !source_failed
        && (snapshot.source().is_some()
            || snapshot.transport_state() == TransportState::Inspecting);
    let source = playlist
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .activate_for_play(source_loaded);
    if source.is_some() {
        present_playlist(window, playlist);
    }
    source
}

fn playlist_model(items: &[(String, bool)]) -> ModelRc<PlaylistRow> {
    ModelRc::new(VecModel::from(
        items
            .iter()
            .map(|(label, selected)| PlaylistRow {
                label: label.into(),
                selected: *selected,
            })
            .collect::<Vec<_>>(),
    ))
}

fn recent_source_label(value: &str) -> SharedString {
    crate::recent_sources::label(value).into()
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn choose_sources() -> Option<Vec<std::path::PathBuf>> {
    rfd::FileDialog::new()
        .set_title("Open stem files")
        .add_filter(
            "Audio and stem sources",
            &["m4a", "mp4", "mp3", "flac", "ogg", "oga", "json", "wav"],
        )
        .pick_files()
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn choose_sources() -> Option<Vec<std::path::PathBuf>> {
    std::env::var_os("STEMPLAYER_SOURCE").map(|path| vec![path.into()])
}

#[cfg(test)]
mod regression_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_height_transition_remembers_both_sizes() {
        assert_eq!(
            playlist_height_transition(false, 900.0, 620.0, 298.0),
            (900.0, 298.0, 298.0)
        );
        assert_eq!(
            playlist_height_transition(true, 310.0, 900.0, 298.0),
            (900.0, 310.0, 900.0)
        );
    }

    #[test]
    fn frequency_color_uses_spectral_anchors() {
        let bass = frequency_color(1.0, 0.0, 0.0).to_argb_u8();
        assert_eq!((bass.red, bass.green, bass.blue), (255, 72, 47));

        let high = frequency_color(0.0, 0.0, 1.0).to_argb_u8();
        assert_eq!((high.red, high.green, high.blue), (0, 205, 255));

        let quiet = frequency_color(0.0, 0.0, 0.0).to_argb_u8();
        assert_eq!((quiet.red, quiet.green, quiet.blue), (0, 120, 145));
    }

    #[test]
    fn missing_and_legacy_saved_outputs_never_select_system_default() {
        for saved in ["endpoint-v1-missing", "device-1", &"x".repeat(129)] {
            let (_, ids, selected) = output_device_options_from(Vec::new(), Some(saved));
            assert_eq!(selected, 1);
            assert!(ids[1].is_some());
            assert_eq!(ids[0], None);
        }
        let (_, ids, selected) = output_device_options_from(Vec::new(), None);
        assert_eq!(selected, 0);
        assert_eq!(ids.as_ref(), &[None]);
    }

    #[test]
    fn saved_output_identity_survives_reordering() {
        let report = |id: &str| stemplayer_output::DeviceReport {
            id: id.into(),
            name: "Synthetic output".into(),
            is_default: false,
            default_configuration: None,
            supported_configurations: vec![],
            stereo_supported: true,
        };
        for devices in [
            vec![report("endpoint-v1-first"), report("endpoint-v1-second")],
            vec![report("endpoint-v1-second"), report("endpoint-v1-first")],
        ] {
            let (_, ids, selected) = output_device_options_from(devices, Some("endpoint-v1-first"));
            assert_eq!(
                ids[usize::try_from(selected).unwrap()]
                    .as_ref()
                    .unwrap()
                    .as_str(),
                "endpoint-v1-first"
            );
        }
    }

    #[test]
    fn playlist_discovery_error_uses_a_notice_and_preserves_transport() {
        let error = stemplayer_domain::PlayerError::new(
            stemplayer_domain::ErrorCode::ManifestInvalid,
            "Dropped source is unavailable",
        )
        .with_user_action("Choose supported stem files");
        let snapshot = playlist_error_snapshot(
            stemplayer_domain::PlayerSnapshot::empty(TransportState::Empty),
            error,
        );
        let model =
            stemplayer_ui::PresentationModel::from_snapshot(&source_notice_snapshot(&snapshot));

        assert_eq!(
            source_notice(&snapshot).as_deref(),
            Some("Dropped source is unavailable")
        );
        assert!(model.error_banner.is_none());
        assert_eq!(snapshot.transport_state(), TransportState::Empty);
        assert_eq!(
            snapshot.details().error.as_ref().unwrap().code,
            stemplayer_domain::ErrorCode::ManifestInvalid
        );
    }

    #[test]
    fn source_and_timeline_failures_use_notices_without_a_blocking_banner() {
        for code in [
            stemplayer_domain::ErrorCode::CodecUnsupported,
            stemplayer_domain::ErrorCode::TimelineInvalid,
            stemplayer_domain::ErrorCode::HlsHttpFailed,
        ] {
            let snapshot = playlist_error_snapshot(
                PlayerSnapshot::empty(TransportState::Error),
                PlayerError::new(code, "Audio is unavailable"),
            );

            assert_eq!(
                source_notice(&snapshot).as_deref(),
                Some("Track unavailable: Audio is unavailable")
            );
            let presented = source_notice_snapshot(&snapshot);
            assert!(
                stemplayer_ui::PresentationModel::from_snapshot(&presented)
                    .error_banner
                    .is_none()
            );
            assert_eq!(presented.transport_state(), TransportState::Error);
            assert_eq!(snapshot.details().error.as_ref().unwrap().code, code);
        }
    }

    #[test]
    fn internal_device_and_fatal_errors_keep_their_blocking_banner() {
        for code in [
            stemplayer_domain::ErrorCode::Internal,
            stemplayer_domain::ErrorCode::FeatureUnsupported,
        ] {
            let snapshot = playlist_error_snapshot(
                PlayerSnapshot::empty(TransportState::Error),
                PlayerError::new(code, "Output failed"),
            );
            assert!(source_notice(&snapshot).is_none());
            assert!(
                stemplayer_ui::PresentationModel::from_snapshot(&source_notice_snapshot(&snapshot))
                    .error_banner
                    .is_some()
            );
        }
        let mut error =
            PlayerError::new(stemplayer_domain::ErrorCode::DecodeFailed, "Fatal failure");
        error.severity = Severity::Fatal;
        let snapshot = playlist_error_snapshot(PlayerSnapshot::empty(TransportState::Error), error);
        assert!(source_notice(&snapshot).is_none());
    }

    #[test]
    fn waveform_warnings_use_a_notice_without_disabling_playback() {
        let mut error = PlayerError::new(
            stemplayer_domain::ErrorCode::AudioDataInvalid,
            "Waveform unavailable",
        );
        error.severity = Severity::Warning;
        let snapshot = playlist_error_snapshot(PlayerSnapshot::empty(TransportState::Ready), error);

        assert_eq!(
            source_notice(&snapshot).as_deref(),
            Some("Waveform unavailable")
        );
        let model =
            stemplayer_ui::PresentationModel::from_snapshot(&source_notice_snapshot(&snapshot));
        assert!(model.error_banner.is_none());
        assert!(model.transport.play);
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

    #[test]
    fn next_source_preparation_starts_only_in_the_last_five_playing_seconds() {
        let source = stemplayer_domain::SourceMetadata {
            display_name: stemplayer_domain::DisplayName::truncated("track.wav"),
            kind: stemplayer_domain::SourceKind::LocalAudioFile,
            has_master: false,
            sample_rate: stemplayer_domain::SampleRate::new(48_000).unwrap(),
            channel_count: stemplayer_domain::ChannelCount::new(2).unwrap(),
            start_frame: SourceFrame::new(0),
            end_frame_exclusive: SourceFrame::new(480_000),
        };
        let snapshot = |state, frame| {
            PlayerSnapshot::new(
                state,
                stemplayer_domain::GenerationId::INITIAL,
                Some(stemplayer_domain::PlaybackPosition::new(
                    SourceFrame::new(frame),
                    stemplayer_domain::Microseconds::new(0),
                )),
                Some(source.clone()),
                vec![],
            )
            .unwrap()
        };

        assert!(!should_prepare_next(&snapshot(
            TransportState::Playing,
            239_999
        )));
        assert!(should_prepare_next(&snapshot(
            TransportState::Playing,
            240_000
        )));
        assert!(!should_prepare_next(&snapshot(
            TransportState::Paused,
            240_000
        )));
    }

    #[test]
    fn snapshot_listener_survives_idle_timeouts() {
        let controller = StudioController::new();
        let subscription = controller.subscribe_source_snapshots();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            controller.shutdown_and_join();
        });
        let snapshot = receive_snapshot(&subscription, Duration::from_millis(1));
        sender.join().unwrap();
        assert_eq!(
            snapshot.snapshot().transport_state(),
            TransportState::Closed
        );
    }
}
