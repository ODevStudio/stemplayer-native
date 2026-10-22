use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use stemplayer_platform::{LocalSettings, save_settings, validate_settings};

#[derive(Clone)]
pub(crate) struct SettingsStore {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    ready: Condvar,
}

struct State {
    current: LocalSettings,
    dirty: bool,
    rejected: bool,
    closed: bool,
}

pub(crate) struct SettingsWriter {
    store: SettingsStore,
    worker: Option<JoinHandle<io::Result<()>>>,
}

impl SettingsStore {
    pub(crate) fn snapshot(&self) -> LocalSettings {
        self.shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .current
            .clone()
    }

    pub(crate) fn update(
        &self,
        update: impl FnOnce(&LocalSettings) -> LocalSettings,
    ) -> Option<LocalSettings> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            return None;
        }
        let next = update(&state.current);
        if validate_settings(&next).is_err() {
            state.rejected = true;
            self.shared.ready.notify_one();
            return None;
        }
        if next != state.current {
            state.current = next.clone();
            state.dirty = true;
            self.shared.ready.notify_one();
        }
        Some(next)
    }

    fn close(&self, update: impl FnOnce(&LocalSettings) -> LocalSettings) -> io::Result<()> {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            return Ok(());
        }
        let next = update(&state.current);
        let valid = validate_settings(&next);
        if valid.is_ok() {
            state.current = next;
        }
        state.closed = true;
        state.dirty = true;
        self.shared.ready.notify_one();
        valid
    }
}

impl SettingsWriter {
    pub(crate) fn start(
        path: Option<PathBuf>,
        initial: LocalSettings,
        on_error: impl FnMut() + Send + 'static,
    ) -> io::Result<Self> {
        Self::spawn(
            initial,
            move |settings| match &path {
                Some(path) => save_settings(path, settings),
                None => Err(io::Error::other("Settings location is unavailable")),
            },
            on_error,
        )
    }

    fn spawn(
        initial: LocalSettings,
        mut write: impl FnMut(&LocalSettings) -> io::Result<()> + Send + 'static,
        mut on_error: impl FnMut() + Send + 'static,
    ) -> io::Result<Self> {
        validate_settings(&initial)?;
        let store = SettingsStore {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    current: initial,
                    dirty: false,
                    rejected: false,
                    closed: false,
                }),
                ready: Condvar::new(),
            }),
        };
        let shared = Arc::clone(&store.shared);
        let worker = thread::Builder::new()
            .name("settings-writer".into())
            .spawn(move || {
                let mut result = Ok(());
                let mut reported_error = false;
                loop {
                    let (pending, rejected, closed) = {
                        let state = shared
                            .state
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        let mut state = shared
                            .ready
                            .wait_while(state, |state| {
                                !state.dirty && !state.rejected && !state.closed
                            })
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        let pending = state.dirty.then(|| state.current.clone());
                        state.dirty = false;
                        let rejected = std::mem::take(&mut state.rejected);
                        (pending, rejected, state.closed)
                    };
                    if rejected && !reported_error {
                        on_error();
                        reported_error = true;
                    }
                    if let Some(settings) = pending {
                        result = write(&settings);
                        if result.is_err() && !reported_error {
                            on_error();
                        }
                        reported_error = result.is_err();
                    }
                    if closed {
                        return result;
                    }
                }
            })?;
        Ok(Self {
            store,
            worker: Some(worker),
        })
    }

    pub(crate) fn store(&self) -> SettingsStore {
        self.store.clone()
    }

    pub(crate) fn finish(
        mut self,
        update: impl FnOnce(&LocalSettings) -> LocalSettings,
    ) -> io::Result<()> {
        let valid = self.store.close(update);
        valid.and(self.join())
    }

    fn join(&mut self) -> io::Result<()> {
        self.worker.take().map_or(Ok(()), |worker| {
            worker
                .join()
                .map_err(|_| io::Error::other("Settings writer stopped unexpectedly"))?
        })
    }
}

impl Drop for SettingsWriter {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.store.close(Clone::clone);
            let _ = self.join();
        }
    }
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
