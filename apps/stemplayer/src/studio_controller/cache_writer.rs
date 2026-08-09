use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::thread::{self, JoinHandle};
use stemplayer_domain::{ErrorCode, PlayerError};
use stemplayer_waveform::{SourceFingerprint, WaveformCache, WaveformSnapshot};

const CAPACITY: usize = 2;

pub(super) struct CacheWriter {
    sender: SyncSender<Operation>,
    results: Receiver<Result<(), PlayerError>>,
    worker: Option<JoinHandle<()>>,
    pending_writes: usize,
}

enum Operation {
    Write(WaveformSnapshot),
    ClearCurrent(SourceFingerprint, SyncSender<Result<(), PlayerError>>),
    ClearAll(SyncSender<Result<(), PlayerError>>),
    SetCapacity(u64, SyncSender<Result<(), PlayerError>>),
    Shutdown,
}

impl CacheWriter {
    pub fn new(cache: Arc<WaveformCache>) -> Self {
        let (sender, operations) = sync_channel(CAPACITY);
        let (result_sender, results) = sync_channel(CAPACITY + 1);
        let worker = thread::spawn(move || {
            while let Ok(operation) = operations.recv() {
                match operation {
                    Operation::Write(snapshot) => {
                        if result_sender.send(cache.write(&snapshot)).is_err() {
                            break;
                        }
                    }
                    Operation::ClearCurrent(fingerprint, response) => {
                        let _ = response.send(cache.clear_current(fingerprint));
                    }
                    Operation::ClearAll(response) => {
                        let _ = response.send(cache.clear_all());
                    }
                    Operation::SetCapacity(capacity, response) => {
                        let _ = response.send(cache.set_capacity_bytes(capacity));
                    }
                    Operation::Shutdown => break,
                }
            }
        });
        Self {
            sender,
            results,
            worker: Some(worker),
            pending_writes: 0,
        }
    }

    pub fn write(&mut self, snapshot: WaveformSnapshot) -> Result<(), PlayerError> {
        self.poll()?;
        self.sender
            .try_send(Operation::Write(snapshot))
            .map_err(|error| match error {
                TrySendError::Full(_) => PlayerError::new(
                    ErrorCode::CommandQueueFull,
                    "Waveform cache writer queue is full",
                ),
                TrySendError::Disconnected(_) => cache_writer_closed(),
            })?;
        self.pending_writes += 1;
        Ok(())
    }

    pub fn clear_current(&mut self, fingerprint: SourceFingerprint) -> Result<(), PlayerError> {
        let (response, result) = sync_channel(0);
        self.sender
            .send(Operation::ClearCurrent(fingerprint, response))
            .map_err(|_| cache_writer_closed())?;
        result.recv().map_err(|_| cache_writer_closed())??;
        self.poll()
    }

    pub fn clear_all(&mut self) -> Result<(), PlayerError> {
        let (response, result) = sync_channel(0);
        self.sender
            .send(Operation::ClearAll(response))
            .map_err(|_| cache_writer_closed())?;
        result.recv().map_err(|_| cache_writer_closed())??;
        self.poll()
    }

    pub fn set_capacity(&mut self, capacity: u64) -> Result<(), PlayerError> {
        let (response, result) = sync_channel(0);
        self.sender
            .send(Operation::SetCapacity(capacity, response))
            .map_err(|_| cache_writer_closed())?;
        result.recv().map_err(|_| cache_writer_closed())??;
        self.poll()
    }

    pub fn pending(&self) -> bool {
        self.pending_writes > 0
    }

    pub fn poll(&mut self) -> Result<(), PlayerError> {
        let mut failure = None;
        loop {
            match self.results.try_recv() {
                Ok(result) => {
                    self.pending_writes = self.pending_writes.saturating_sub(1);
                    if failure.is_none() {
                        failure = result.err();
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) if self.pending_writes == 0 => break,
                Err(TryRecvError::Disconnected) => return Err(cache_writer_closed()),
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub fn shutdown(&mut self) -> Result<(), PlayerError> {
        let Some(worker) = self.worker.take() else {
            return self.poll();
        };
        let send = self
            .sender
            .send(Operation::Shutdown)
            .map_err(|_| cache_writer_closed());
        let join = worker
            .join()
            .map_err(|_| PlayerError::new(ErrorCode::Internal, "Waveform cache writer terminated"));
        send.and(join).and_then(|_| self.poll())
    }
}

impl Drop for CacheWriter {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn cache_writer_closed() -> PlayerError {
    PlayerError::new(ErrorCode::Internal, "Waveform cache writer is closed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use stemplayer_domain::{GenerationId, SampleRate, SourceFrame};
    use stemplayer_waveform::{CacheConfig, CacheStatus, PeakAnalyzer};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn clear_and_shutdown_are_ordered_after_writes() {
        let root = std::env::temp_dir().join(format!(
            "stemplayer-cache-writer-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let cache = Arc::new(WaveformCache::new(root.clone(), CacheConfig::default()).unwrap());
        let first = SourceFingerprint::from_bytes([1; 32]);
        let second = SourceFingerprint::from_bytes([2; 32]);
        let mut writer = CacheWriter::new(Arc::clone(&cache));
        writer.write(snapshot(first)).unwrap();
        writer.clear_all().unwrap();
        assert_eq!(
            cache
                .load(
                    first,
                    GenerationId::INITIAL,
                    SampleRate::new(48_000).unwrap(),
                    SourceFrame::new(0)..SourceFrame::new(8),
                )
                .status,
            CacheStatus::Miss
        );
        writer.write(snapshot(second)).unwrap();
        writer.shutdown().unwrap();
        assert_eq!(
            cache
                .load(
                    second,
                    GenerationId::INITIAL,
                    SampleRate::new(48_000).unwrap(),
                    SourceFrame::new(0)..SourceFrame::new(8),
                )
                .status,
            CacheStatus::Hit
        );
        let _ = fs::remove_dir_all(root);
    }

    fn snapshot(fingerprint: SourceFingerprint) -> WaveformSnapshot {
        let mut analyzer = PeakAnalyzer::new(
            GenerationId::INITIAL,
            fingerprint,
            SourceFrame::new(0),
            SourceFrame::new(8),
            SampleRate::new(48_000).unwrap(),
        )
        .unwrap();
        analyzer
            .ingest(SourceFrame::new(0), 8, 8, &[0.25; 64])
            .unwrap();
        analyzer.finish(CacheStatus::Miss).unwrap()
    }
}
