use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::thread::{self, JoinHandle};
use stemplayer_domain::{GenerationId, PlayerError};
use stemplayer_engine::PlaybackSession;
use stemplayer_waveform::{CacheResult, CacheStatus, SourceFingerprint, WaveformCache};

pub(super) struct PreparedSource {
    pub(super) path: PathBuf,
    pub(super) session: PlaybackSession,
    pub(super) fingerprint: SourceFingerprint,
    pub(super) cache: CacheResult,
    pub(super) cache_enabled: bool,
}

#[derive(Clone)]
struct Request {
    path: PathBuf,
    generation: GenerationId,
    cache: Option<Arc<WaveformCache>>,
    cache_enabled: bool,
}

impl Request {
    fn matches(&self, path: &Path, generation: GenerationId, cache_enabled: bool) -> bool {
        self.path == path && self.generation == generation && self.cache_enabled == cache_enabled
    }
}

struct Job {
    request: Request,
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<PreparedSource, PlayerError>>,
    worker: JoinHandle<()>,
}

#[derive(Default)]
pub(super) struct SourcePreloader {
    wanted: Option<Request>,
    job: Option<Job>,
    ready: Option<PreparedSource>,
}

impl SourcePreloader {
    pub(super) fn prepare(
        &mut self,
        path: PathBuf,
        generation: GenerationId,
        cache: Option<Arc<WaveformCache>>,
        cache_enabled: bool,
    ) {
        self.poll();
        if self
            .ready
            .as_ref()
            .is_some_and(|source| source.path == path && source.session.generation() == generation)
            || self
                .job
                .as_ref()
                .is_some_and(|job| job.request.matches(&path, generation, cache_enabled))
        {
            return;
        }
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Release);
        }
        self.ready = None;
        self.wanted = Some(Request {
            path,
            generation,
            cache,
            cache_enabled,
        });
        self.start();
    }

    pub(super) fn poll(&mut self) {
        let Some(job) = self.job.as_ref() else {
            self.start();
            return;
        };
        let result = match job.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Disconnected) => None,
            Err(TryRecvError::Empty) => return,
        };
        let job = self.job.take().unwrap();
        let _ = job.worker.join();
        let current = self.wanted.as_ref().is_some_and(|wanted| {
            wanted.matches(
                &job.request.path,
                job.request.generation,
                job.request.cache_enabled,
            )
        });
        if current {
            self.wanted = None;
            if let Some(Ok(source)) = result {
                self.ready = Some(source);
            }
        }
        self.start();
    }

    pub(super) fn take(&mut self, path: &Path, generation: GenerationId) -> Option<PreparedSource> {
        self.poll();
        let ready = self
            .ready
            .take()
            .filter(|source| source.path == path && source.session.generation() == generation)
            .filter(|source| {
                super::waveform_fingerprint(&source.session)
                    .is_ok_and(|fingerprint| fingerprint == source.fingerprint)
            });
        self.cancel();
        ready
    }

    pub(super) fn pending(&self) -> bool {
        self.job.is_some() || self.wanted.is_some() && self.ready.is_none()
    }

    pub(super) fn cancel(&mut self) {
        self.wanted = None;
        self.ready = None;
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Release);
        }
    }

    pub(super) fn shutdown(&mut self) {
        self.cancel();
        if let Some(job) = self.job.take() {
            let _ = job.worker.join();
        }
    }

    fn start(&mut self) {
        if self.job.is_some() || self.ready.is_some() {
            return;
        }
        let Some(request) = self.wanted.clone() else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let worker_request = request.clone();
        let (sender, result) = sync_channel(1);
        let worker = thread::spawn(move || {
            let _ = sender.send(load(worker_request, &worker_cancel));
        });
        self.job = Some(Job {
            request,
            cancel,
            result,
            worker,
        });
    }
}

fn load(request: Request, cancelled: &AtomicBool) -> Result<PreparedSource, PlayerError> {
    let session = super::open_local_session(&request.path, request.generation, cancelled)?;
    let fingerprint = super::waveform_fingerprint(&session)?;
    let metadata = session.metadata();
    let cache = request
        .cache
        .as_ref()
        .filter(|_| request.cache_enabled)
        .map(|cache| {
            cache.load(
                fingerprint,
                request.generation,
                metadata.sample_rate,
                metadata.start_frame..metadata.end_frame_exclusive,
            )
        })
        .unwrap_or(CacheResult {
            status: CacheStatus::Disabled,
            snapshot: None,
            bytes: 0,
        });
    Ok(PreparedSource {
        path: request.path,
        session,
        fingerprint,
        cache,
        cache_enabled: request.cache_enabled,
    })
}
