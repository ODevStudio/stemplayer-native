use super::{
    AppWindow, StudioController, choose_sources, persist_playlist, playlist_error_snapshot,
    present, present_playlist, show_source_notice, source_notice, source_notice_snapshot,
    submit_playlist_source,
};
use crate::playlist::{Playlist, PlaylistItem, discover};
use crate::settings::SettingsStore;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use stemplayer_domain::{PlayerController, PlayerError};

#[derive(Clone, Default)]
pub(super) struct Requests(Rc<RefCell<Arc<AtomicBool>>>);

impl Requests {
    pub(super) fn begin(&self) -> Arc<AtomicBool> {
        let request = Arc::new(AtomicBool::new(false));
        self.0
            .replace(Arc::clone(&request))
            .store(true, Ordering::Release);
        request
    }

    pub(super) fn cancel(&self) {
        self.0.borrow().store(true, Ordering::Release);
    }
}

pub(super) fn spawn_picker(
    playlist: Arc<Mutex<Playlist>>,
    window: slint::Weak<AppWindow>,
    controller: Arc<StudioController>,
    settings_store: SettingsStore,
    restore_cancelled: &AtomicBool,
    requests: &Requests,
) {
    restore_cancelled.store(true, Ordering::Release);
    let request = requests.begin();
    let Some(paths) = choose_sources() else {
        return;
    };
    start(paths, playlist, window, controller, settings_store, request);
}

pub(super) fn start(
    paths: Vec<PathBuf>,
    playlist: Arc<Mutex<Playlist>>,
    window: slint::Weak<AppWindow>,
    controller: Arc<StudioController>,
    settings_store: SettingsStore,
    request: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let items = discover(&paths);
        let _ = window.upgrade_in_event_loop(move |app| {
            if apply(items, &playlist, &app, controller.as_ref(), &request) {
                persist_playlist(&settings_store, &playlist);
            }
        });
    });
}

pub(super) fn apply(
    items: Result<Vec<PlaylistItem>, PlayerError>,
    playlist: &Arc<Mutex<Playlist>>,
    app: &AppWindow,
    controller: &dyn PlayerController,
    request: &AtomicBool,
) -> bool {
    let cancelled = request.load(Ordering::Acquire);
    let items = match items {
        Ok(items) => items,
        Err(_) if cancelled => return false,
        Err(error) => {
            let snapshot = playlist_error_snapshot(controller.latest_snapshot(), error);
            present(app, &source_notice_snapshot(&snapshot));
            if let Some(notice) = source_notice(&snapshot) {
                show_source_notice(app, notice);
            }
            return false;
        }
    };
    let initial_source = {
        let mut playlist = playlist
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cancelled {
            playlist.append_preserving_selection(items);
            None
        } else {
            playlist.append(items)
        }
    };
    present_playlist(&app.as_weak(), playlist);
    if let Some(source) = initial_source {
        submit_playlist_source(controller, source, false);
    }
    true
}
