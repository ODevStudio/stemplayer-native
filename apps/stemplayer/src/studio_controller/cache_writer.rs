use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::thread::{self, JoinHandle};
use stemplayer_domain::{ErrorCode, PlayerError};
use stemplayer_waveform::{SourceFingerprint, WaveformCache, WaveformSnapshot};

const CAPACITY: usize = 2;
const RESULT_CAPACITY: usize = CAPACITY + 1;

pub(super) struct CacheWriter {
    sender: SyncSender<Operation>,
    results: Receiver<Completion>,
    worker: Option<JoinHandle<()>>,
    pending_operations: usize,
}

enum Operation {
    Write(WaveformSnapshot, u64),
    ClearCurrent(SourceFingerprint),
    ClearAll,
    SetCapacity(u64),
    Shutdown,
}

struct Completion {
    source_epoch: Option<u64>,
    result: Result<(), PlayerError>,
}

impl CacheWriter {
    pub fn new(cache: Arc<WaveformCache>) -> Self {
        let (sender, operations) = sync_channel(CAPACITY);
        let (result_sender, results) = sync_channel(RESULT_CAPACITY);
        let worker = thread::spawn(move || {
            while let Ok(operation) = operations.recv() {
                let (source_epoch, result) = match operation {
                    Operation::Write(snapshot, epoch) => (Some(epoch), cache.write(&snapshot)),
                    Operation::ClearCurrent(fingerprint) => {
                        (None, cache.clear_current(fingerprint))
                    }
                    Operation::ClearAll => (None, cache.clear_all()),
                    Operation::SetCapacity(capacity) => (None, cache.set_capacity_bytes(capacity)),
                    Operation::Shutdown => break,
                };
                if result_sender
                    .send(Completion {
                        source_epoch,
                        result,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            sender,
            results,
            worker: Some(worker),
            pending_operations: 0,
        }
    }

    pub fn write(
        &mut self,
        snapshot: WaveformSnapshot,
        source_epoch: u64,
    ) -> Result<(), PlayerError> {
        self.enqueue(Operation::Write(snapshot, source_epoch))
    }

    fn enqueue(&mut self, operation: Operation) -> Result<(), PlayerError> {
        if self.pending_operations >= RESULT_CAPACITY {
            return Err(cache_writer_full());
        }
        self.sender
            .try_send(operation)
            .map_err(|error| match error {
                TrySendError::Full(_) => cache_writer_full(),
                TrySendError::Disconnected(_) => cache_writer_closed(),
            })?;
        self.pending_operations += 1;
        Ok(())
    }

    pub fn clear_current(&mut self, fingerprint: SourceFingerprint) -> Result<(), PlayerError> {
        self.enqueue(Operation::ClearCurrent(fingerprint))
    }

    pub fn clear_all(&mut self) -> Result<(), PlayerError> {
        self.enqueue(Operation::ClearAll)
    }

    pub fn set_capacity(&mut self, capacity: u64) -> Result<(), PlayerError> {
        self.enqueue(Operation::SetCapacity(capacity))
    }

    pub fn pending(&self) -> bool {
        self.pending_operations > 0
    }

    pub fn poll(&mut self, source_epoch: u64) -> Result<(), PlayerError> {
        let mut failure = None;
        loop {
            match self.results.try_recv() {
                Ok(result) => {
                    self.pending_operations = self.pending_operations.saturating_sub(1);
                    if failure.is_none()
                        && result
                            .source_epoch
                            .is_none_or(|epoch| epoch == source_epoch)
                    {
                        failure = result.result.err();
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) if self.pending_operations == 0 => break,
                Err(TryRecvError::Disconnected) => {
                    self.pending_operations = 0;
                    return Err(cache_writer_closed());
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub fn shutdown(&mut self) -> Result<(), PlayerError> {
        let Some(worker) = self.worker.take() else {
            return self.poll(u64::MAX);
        };
        let send = self
            .sender
            .send(Operation::Shutdown)
            .map_err(|_| cache_writer_closed());
        let join = worker
            .join()
            .map_err(|_| PlayerError::new(ErrorCode::Internal, "Waveform cache writer terminated"));
        send.and(join).and_then(|_| self.poll(u64::MAX))
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

fn cache_writer_full() -> PlayerError {
    PlayerError::new(
        ErrorCode::CommandQueueFull,
        "Waveform cache writer queue is full",
    )
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
    fn unacknowledged_writes_remain_bounded_when_the_request_queue_has_space() {
        let (sender, operations) = sync_channel(CAPACITY);
        let (_result_sender, results) = sync_channel(RESULT_CAPACITY);
        let mut writer = CacheWriter {
            sender,
            results,
            worker: None,
            pending_operations: RESULT_CAPACITY,
        };

        let error = writer
            .write(snapshot(SourceFingerprint::from_bytes([3; 32])), 0)
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::CommandQueueFull);
        assert_eq!(writer.pending_operations, RESULT_CAPACITY);
        assert!(matches!(operations.try_recv(), Err(TryRecvError::Empty)));
    }

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
        writer.write(snapshot(first), 0).unwrap();
        writer.clear_all().unwrap();
        wait_for_completion(&mut writer);
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
        writer.write(snapshot(second), 0).unwrap();
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
        drop(cache);
        fs::remove_dir_all(root).unwrap();
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

    fn wait_for_completion(writer: &mut CacheWriter) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while writer.pending() {
            assert!(std::time::Instant::now() < deadline);
            writer.poll(0).unwrap();
            thread::yield_now();
        }
    }

    #[test]
    fn management_requests_never_wait_for_the_worker() {
        let (sender, operations) = sync_channel(CAPACITY);
        let (_results, results) = sync_channel(RESULT_CAPACITY);
        let mut writer = CacheWriter {
            sender,
            results,
            worker: None,
            pending_operations: 0,
        };
        writer.clear_all().unwrap();
        writer.set_capacity(1).unwrap();
        assert_eq!(
            writer.clear_all().unwrap_err().code,
            ErrorCode::CommandQueueFull
        );
        assert!(matches!(operations.try_recv(), Ok(Operation::ClearAll)));
        assert!(matches!(
            operations.try_recv(),
            Ok(Operation::SetCapacity(1))
        ));
        assert_eq!(writer.pending_operations, 2);
    }

    #[test]
    fn old_source_failures_are_acknowledged_without_misattribution() {
        let (sender, _operations) = sync_channel(CAPACITY);
        let (completed, results) = sync_channel(RESULT_CAPACITY);
        let mut writer = CacheWriter {
            sender,
            results,
            worker: None,
            pending_operations: 1,
        };
        completed
            .send(Completion {
                source_epoch: Some(1),
                result: Err(cache_writer_closed()),
            })
            .unwrap();
        writer.poll(2).unwrap();
        assert!(!writer.pending());
    }
}
