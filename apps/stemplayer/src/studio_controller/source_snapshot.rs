use super::{Shared, StudioController, WorkerCommand};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};
use stemplayer_domain::{PlayerError, PlayerSnapshot, SourceLocator};

#[derive(Clone, Debug)]
pub struct SourceSnapshot {
    snapshot: PlayerSnapshot,
    epoch: u64,
    transport_revision: u64,
    cancelled: Arc<AtomicBool>,
}

impl SourceSnapshot {
    pub fn snapshot(&self) -> &PlayerSnapshot {
        &self.snapshot
    }

    pub(super) fn matches_admission(
        &self,
        epoch: u64,
        transport_revision: u64,
        cancelled: &Arc<AtomicBool>,
    ) -> bool {
        self.epoch == epoch
            && self.transport_revision == transport_revision
            && Arc::ptr_eq(&self.cancelled, cancelled)
    }
}

struct SourceSnapshotQueue {
    value: Mutex<Option<SourceSnapshot>>,
    ready: Condvar,
}

pub(super) struct SourceSnapshotPublisher {
    queue: Weak<SourceSnapshotQueue>,
}

impl SourceSnapshotPublisher {
    fn publish(&self, snapshot: SourceSnapshot) -> bool {
        let Some(queue) = self.queue.upgrade() else {
            return false;
        };
        *queue
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(snapshot);
        queue.ready.notify_one();
        true
    }
}

pub struct SourceSnapshotSubscription {
    queue: Arc<SourceSnapshotQueue>,
}

impl SourceSnapshotSubscription {
    pub fn recv_timeout(&self, timeout: Duration) -> Option<SourceSnapshot> {
        let deadline = Instant::now().checked_add(timeout)?;
        let mut value = self
            .queue
            .value
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if let Some(snapshot) = value.take() {
                return Some(snapshot);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, result) = self
                .queue
                .ready
                .wait_timeout(value, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            value = next;
            if result.timed_out() {
                return value.take();
            }
        }
    }
}

impl StudioController {
    pub fn subscribe_source_snapshots(&self) -> SourceSnapshotSubscription {
        let queue = Arc::new(SourceSnapshotQueue {
            value: Mutex::new(None),
            ready: Condvar::new(),
        });
        self.shared
            .source_subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(SourceSnapshotPublisher {
                queue: Arc::downgrade(&queue),
            });
        SourceSnapshotSubscription { queue }
    }

    pub fn submit_source_recovery(
        &self,
        evidence: &SourceSnapshot,
        source: Option<SourceLocator>,
        play: bool,
    ) -> Result<bool, PlayerError> {
        let play = source.is_some() && play;
        self.enqueue_if_current(
            WorkerCommand::SourceRecovery { source, play },
            true,
            Some(evidence),
        )
    }
}

pub(super) fn publish(shared: &Shared, snapshot: PlayerSnapshot, transport_revision: u64) {
    let evidence = SourceSnapshot {
        snapshot: snapshot.clone(),
        epoch: shared.epoch.load(Ordering::Acquire),
        transport_revision,
        cancelled: Arc::clone(
            &shared
                .cancel
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        ),
    };
    *shared
        .snapshot
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot.clone();
    shared
        .subscribers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|publisher| publisher.publish(snapshot.clone()));
    shared
        .source_subscribers
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|publisher| publisher.publish(evidence.clone()));
}
