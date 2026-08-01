use crate::playlist::{Playlist, discover};
use crate::studio_controller::StudioController;
use crate::{AppWindow, PlaylistRow};
use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use stemplayer_domain::{
    BoundedText, CacheCommand, DeviceId, OutputPreference, PlayerCommand, PlayerController,
    SnapshotSubscription, SourceFrame, SourceLocator, StemRole, TransportState,
};
use stemplayer_ui::{CommandAdapter, Playhead};

const CACHE_LIMITS_MB: [u32; 4] = [64, 256, 1_024, 4_096];
const COLLAPSED_WINDOW_HEIGHT: f32 = 298.0;
static SETTINGS_WRITE_LOCK: Mutex<()> = Mutex::new(());

pub fn wire(
    app: &AppWindow,
    quit_on_end: bool,
    settings_path: Option<std::path::PathBuf>,
    settings: &stemplayer_platform::LocalSettings,
) -> Arc<StudioController> {
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
    let playlist = Arc::new(Mutex::new(Playlist::restore(
        &settings.playlist_sources,
        settings.playlist_selected_source.as_deref(),
    )));
    let collapsed_window_height = Rc::new(Cell::new(COLLAPSED_WINDOW_HEIGHT));
    let (device_labels, device_ids, selected_device_index) =
        output_device_options(settings.selected_device.as_deref());
    app.set_output_devices(ModelRc::new(VecModel::from(device_labels)));
    app.set_output_device_index(selected_device_index);
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
    let (playlist_rows, playlist_index, playlist_sources) = {
        let playlist = playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (
            playlist.rows(),
            playlist
                .selected()
                .and_then(|index| i32::try_from(index).ok())
                .unwrap_or(-1),
            playlist.source_paths(),
        )
    };
    app.set_playlist_items(playlist_model(&playlist_rows));
    app.set_playlist_index(playlist_index);
    let _ = controller.set_playlist_sources(playlist_sources);
    present(app, &controller.latest_snapshot());
    present_waveform(app, controller.latest_waveform().as_ref());
    if let Some(device) = device_ids
        .get(usize::try_from(selected_device_index).unwrap_or_default())
        .cloned()
        .flatten()
    {
        let _ = controller.submit(PlayerCommand::SelectDevice(Some(device)));
    }
    let restored_source = playlist
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .activate_selected();
    if let Some(source) = restored_source {
        submit_playlist_source(&controller, source, false);
    }

    let weak = app.as_weak();
    let current_playlist = Arc::clone(&playlist);
    let current_controller = Arc::clone(&controller);
    let current_drop_timer = Rc::clone(&drop_timer);
    let current_dropped_paths = Rc::clone(&dropped_paths);
    let current_collapsed_window_height = Rc::clone(&collapsed_window_height);
    let current_settings = settings_path.clone();
    app.window().on_winit_window_event(move |window, event| {
        match event {
            WindowEvent::DroppedFile(path) => {
                current_dropped_paths.borrow_mut().push(path.clone());
                current_drop_timer.stop();
                let paths = Rc::clone(&current_dropped_paths);
                let playlist = Arc::clone(&current_playlist);
                let weak = weak.clone();
                let controller = Arc::clone(&current_controller);
                let settings_path = current_settings.clone();
                current_drop_timer.start(
                    TimerMode::SingleShot,
                    Duration::from_millis(75),
                    move || {
                        discover_playlist_async(
                            std::mem::take(&mut *paths.borrow_mut()),
                            Arc::clone(&playlist),
                            weak.clone(),
                            controller.clone(),
                            settings_path.clone(),
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
    let current_settings = settings_path.clone();
    app.on_open_folder(move || {
        spawn_playlist_picker(
            Arc::clone(&current_playlist),
            weak.clone(),
            Arc::clone(&current),
            current_settings.clone(),
        );
    });

    let weak = app.as_weak();
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_path.clone();
    app.on_select_playlist_item(move |index, control, shift| {
        let changed = usize::try_from(index).is_ok_and(|index| {
            current_playlist
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .select(index, control, shift)
        });
        if changed {
            present_playlist(&weak, &current_playlist);
            persist_playlist(current_settings.as_deref(), &current_playlist);
        }
    });

    let weak = app.as_weak();
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_path.clone();
    app.on_context_select_playlist_item(move |index| {
        let changed = usize::try_from(index).is_ok_and(|index| {
            current_playlist
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .select_context(index)
        });
        if changed {
            present_playlist(&weak, &current_playlist);
            persist_playlist(current_settings.as_deref(), &current_playlist);
        }
    });

    let weak = app.as_weak();
    let current_playlist = Arc::clone(&playlist);
    app.on_select_all_playlist_items(move || {
        current_playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .select_all();
        present_playlist(&weak, &current_playlist);
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_path.clone();
    app.on_remove_playlist_items(move || {
        let sources = {
            let mut playlist = current_playlist
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            playlist.remove_selected().then(|| playlist.source_paths())
        };
        if let Some(sources) = sources {
            let _ = current.set_playlist_sources(sources);
            present_playlist(&weak, &current_playlist);
            persist_playlist(current_settings.as_deref(), &current_playlist);
        }
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_path.clone();
    app.on_reorder_playlist_item(move |from, to| {
        let sources = usize::try_from(from)
            .ok()
            .zip(usize::try_from(to).ok())
            .and_then(|(from, to)| {
                let mut playlist = current_playlist
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                playlist
                    .move_item(from, to)
                    .then(|| playlist.source_paths())
            });
        if let Some(sources) = sources {
            let _ = current.set_playlist_sources(sources);
            present_playlist(&weak, &current_playlist);
            persist_playlist(current_settings.as_deref(), &current_playlist);
        }
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_path.clone();
    app.on_activate_playlist_item(move |index| {
        let path = usize::try_from(index).ok().and_then(|index| {
            current_playlist
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .activate(index)
        });
        if let Some(path) = path {
            present_playlist(&weak, &current_playlist);
            persist_playlist(current_settings.as_deref(), &current_playlist);
            submit_playlist_source(&current, path, true);
        }
    });
    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_settings = settings_path.clone();
    app.on_open_url(move |url| {
        submit_url(&current, &weak, current_settings.as_deref(), url.as_str());
    });

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    let current_settings = settings_path.clone();
    app.on_open_recent(move |index| {
        let Some(source) = current_settings.as_deref().and_then(|settings_path| {
            usize::try_from(index).ok().and_then(|index| {
                stemplayer_platform::load_settings(settings_path)
                    .settings()
                    .recent_sources
                    .get(index)
                    .cloned()
            })
        }) else {
            return;
        };
        if source.starts_with("https://") || source.starts_with("http://") {
            submit_url(&current, &weak, current_settings.as_deref(), &source);
        } else {
            submit_source(&current, &weak, current_settings.as_deref(), source.into());
        }
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
    let current_settings = settings_path.clone();
    app.on_set_waveform_cache_enabled(move |enabled| {
        let command = if enabled {
            CacheCommand::Enable
        } else {
            CacheCommand::Disable
        };
        let _ = current.submit(PlayerCommand::Cache(command));
        let _ = update_settings(current_settings.as_deref(), |current| {
            stemplayer_platform::LocalSettings {
                waveform_cache_enabled: enabled,
                ..current.clone()
            }
        });
    });

    let current = Arc::clone(&controller);
    let current_settings = settings_path.clone();
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
        let _ = update_settings(current_settings.as_deref(), |current| {
            stemplayer_platform::LocalSettings {
                waveform_cache_limit_mb: limit_mb,
                ..current.clone()
            }
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

    let weak = app.as_weak();
    let current = Arc::clone(&controller);
    app.on_export_diagnostics(move || {
        let Some(path) = choose_diagnostics_export() else {
            return;
        };
        let result = diagnostics_json(&current.latest_snapshot())
            .and_then(|json| std::fs::write(path, json).map_err(serde_json::Error::io));
        if let Some(app) = weak.upgrade() {
            app.set_diagnostics_export_status(
                if result.is_ok() {
                    "Exported"
                } else {
                    "Export failed"
                }
                .into(),
            );
        }
    });

    let current = Arc::clone(&controller);
    let current_playlist = Arc::clone(&playlist);
    app.on_play(move || {
        let path = current_playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .activate_selected_if_changed();
        if let Some(path) = path {
            submit_playlist_source(&current, path, true);
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
    let current_settings = settings_path.clone();
    app.on_select_output_device(move |index| {
        let device = usize::try_from(index)
            .ok()
            .and_then(|index| device_ids_for_selection.get(index))
            .cloned()
            .flatten();
        let _ = current.submit(PlayerCommand::SelectDevice(device.clone()));
        let _ = update_settings(current_settings.as_deref(), |current| {
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
    let waveform_controller = Arc::clone(&controller);
    let current_playlist = Arc::clone(&playlist);
    let current_settings = settings_path.clone();
    std::thread::spawn(move || {
        let mut presented_waveform =
            waveform_revision(waveform_controller.latest_waveform().as_ref());
        let mut previous_state = waveform_controller.latest_snapshot().transport_state();
        loop {
            let snapshot = receive_snapshot(&subscription, Duration::from_secs(60));
            let closed = snapshot.transport_state() == TransportState::Closed;
            let finished = quit_on_end && snapshot.transport_state() == TransportState::Ended;
            let next = if !quit_on_end
                && previous_state != TransportState::Ended
                && snapshot.transport_state() == TransportState::Ended
            {
                current_playlist
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .next()
            } else {
                None
            };
            previous_state = snapshot.transport_state();
            if next.is_some() {
                persist_playlist(current_settings.as_deref(), &current_playlist);
            }
            if let Some((_, path)) = &next {
                submit_playlist_source(&waveform_controller, path.clone(), true);
            }
            let playlist_rows = next.as_ref().map(|_| {
                current_playlist
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .rows()
            });
            let playhead = Arc::clone(&authoritative_playhead);
            let waveform = waveform_controller.latest_waveform();
            let playlist_cache_progress = waveform_controller
                .playlist_cache_progress()
                .map(|progress| format!("{progress}%"))
                .unwrap_or_default();
            let revision = waveform_revision(waveform.as_ref());
            let waveform_changed = revision != presented_waveform;
            presented_waveform = revision;
            if weak
                .upgrade_in_event_loop(move |app| {
                    playhead
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .update(&snapshot, epoch.elapsed());
                    present(&app, &snapshot);
                    if waveform_changed {
                        present_waveform(&app, waveform.as_ref());
                    }
                    app.set_playlist_cache_progress(playlist_cache_progress.into());
                    if let Some((index, _)) = next {
                        app.set_playlist_items(playlist_model(&playlist_rows.unwrap_or_default()));
                        app.set_playlist_index(i32::try_from(index).unwrap_or_default());
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
    subscription: &SnapshotSubscription,
    timeout: Duration,
) -> stemplayer_domain::PlayerSnapshot {
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
    let colors = rows[0]
        .iter()
        .enumerate()
        .map(|(index, _)| {
            frequency_color(
                rows[1].get(index).map_or(0.0, |peak| peak.maximum),
                rows[2].get(index).map_or(0.0, |peak| peak.maximum),
                rows[3].get(index).map_or(0.0, |peak| peak.maximum),
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
    let mid = mid.max(0.0) * 1.5;
    let high = high.max(0.0) * 3.0;
    let total = low + mid + high;
    if total <= f32::EPSILON {
        return Color::from_rgb_u8(0, 167, 189);
    }
    Color::from_rgb_u8(
        ((234.0 * low + 55.0 * mid) / total).round() as u8,
        ((61.0 * low + 104.0 * mid + 222.0 * high) / total).round() as u8,
        ((76.0 * low + 255.0 * mid + 181.0 * high) / total).round() as u8,
    )
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

fn spawn_playlist_picker(
    playlist: Arc<Mutex<Playlist>>,
    window: slint::Weak<AppWindow>,
    controller: Arc<StudioController>,
    settings_path: Option<std::path::PathBuf>,
) {
    let Some(paths) = choose_sources() else {
        return;
    };
    std::thread::spawn(move || {
        apply_playlist(
            discover(&paths).unwrap_or_default(),
            playlist,
            window,
            controller,
            settings_path,
        );
    });
}
fn discover_playlist_async(
    paths: Vec<std::path::PathBuf>,
    playlist: Arc<Mutex<Playlist>>,
    window: slint::Weak<AppWindow>,
    controller: Arc<StudioController>,
    settings_path: Option<std::path::PathBuf>,
) {
    std::thread::spawn(move || {
        apply_playlist(
            discover(&paths).unwrap_or_default(),
            playlist,
            window,
            controller,
            settings_path,
        );
    });
}

fn apply_playlist(
    items: Vec<crate::playlist::PlaylistItem>,
    playlist: Arc<Mutex<Playlist>>,
    window: slint::Weak<AppWindow>,
    controller: Arc<StudioController>,
    settings_path: Option<std::path::PathBuf>,
) {
    let (rows, selected, sources, initial_source) = {
        let mut playlist = playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let initial_source = playlist.append(items);
        (
            playlist.rows(),
            playlist
                .selected()
                .and_then(|index| i32::try_from(index).ok())
                .unwrap_or(-1),
            playlist.source_paths(),
            initial_source,
        )
    };
    let _ = controller.set_playlist_sources(sources);
    persist_playlist(settings_path.as_deref(), &playlist);
    let _ = window.upgrade_in_event_loop(move |app| {
        app.set_playlist_items(playlist_model(&rows));
        app.set_playlist_index(selected);
    });
    if let Some(source) = initial_source {
        submit_playlist_source(&controller, source, false);
    }
}
fn submit_playlist_source(controller: &StudioController, path: std::path::PathBuf, play: bool) {
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
    settings_path: Option<&std::path::Path>,
    value: &str,
) {
    let Ok(locator) = BoundedText::new(value) else {
        return;
    };
    if controller
        .submit(PlayerCommand::Open(SourceLocator::HlsUrl(locator)))
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
    let _guard = SETTINGS_WRITE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let settings = update(stemplayer_platform::load_settings(path).settings());
    stemplayer_platform::save_settings(path, &settings)
        .ok()
        .map(|()| settings)
}

fn persist_playlist(path: Option<&std::path::Path>, playlist: &Arc<Mutex<Playlist>>) {
    let (sources, selected) = {
        let playlist = playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (playlist.sources(), playlist.selected_source())
    };
    let _ = update_settings(path, |settings| settings.with_playlist(sources, selected));
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
    if let Some(authority) = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .and_then(|value| value.split('/').next())
    {
        return SharedString::from(format!("HLS ({authority})"));
    }
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

fn diagnostics_json(
    snapshot: &stemplayer_domain::PlayerSnapshot,
) -> Result<Vec<u8>, serde_json::Error> {
    let mut value = serde_json::to_value(snapshot)?;
    if let Some(source) = value
        .get_mut("source")
        .and_then(serde_json::Value::as_object_mut)
    {
        source.remove("display_name");
    }
    serde_json::to_vec_pretty(&value)
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

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn choose_diagnostics_export() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Export diagnostics")
        .set_file_name("stemplayer-diagnostics.json")
        .add_filter("JSON", &["json"])
        .save_file()
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn choose_diagnostics_export() -> Option<std::path::PathBuf> {
    std::env::var_os("STEMPLAYER_DIAGNOSTICS_EXPORT").map(std::path::PathBuf::from)
}

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

    #[test]
    fn diagnostics_export_redacts_source_name() {
        let source = stemplayer_domain::SourceMetadata {
            display_name: stemplayer_domain::DisplayName::truncated("private-track.stem.m4a"),
            kind: stemplayer_domain::SourceKind::LocalStemContainer,
            has_master: true,
            sample_rate: stemplayer_domain::SampleRate::new(48_000).unwrap(),
            channel_count: stemplayer_domain::ChannelCount::new(10).unwrap(),
            start_frame: SourceFrame::new(0),
            end_frame_exclusive: SourceFrame::new(48_000),
        };
        let snapshot = stemplayer_domain::PlayerSnapshot::new(
            TransportState::Ready,
            stemplayer_domain::GenerationId::INITIAL,
            None,
            Some(source),
            vec![],
        )
        .unwrap();
        let json = String::from_utf8(diagnostics_json(&snapshot).unwrap()).unwrap();
        assert!(!json.contains("private-track"));
        assert!(json.contains("sample_rate"));
        assert!(json.contains("waveform_cache"));
    }

    #[test]
    fn snapshot_listener_survives_idle_timeouts() {
        let (publisher, subscription) = SnapshotSubscription::coalescing();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            publisher.publish(stemplayer_domain::PlayerSnapshot::empty(
                TransportState::Ready,
            ));
        });
        let snapshot = receive_snapshot(&subscription, Duration::from_millis(1));
        sender.join().unwrap();
        assert_eq!(snapshot.transport_state(), TransportState::Ready);
    }
}
