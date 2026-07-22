use crate::{CommandId, PlayerCommand, PlayerError, PlayerSnapshot};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

pub trait PlayerController: Send + Sync {
    fn submit(&self, command: PlayerCommand) -> Result<CommandId, PlayerError>;
    fn latest_snapshot(&self) -> PlayerSnapshot;
    fn subscribe(&self) -> SnapshotSubscription;
}

struct SnapshotQueue {
    values: Mutex<VecDeque<PlayerSnapshot>>,
    ready: Condvar,
}

#[derive(Clone)]
pub struct SnapshotPublisher {
    queue: Weak<SnapshotQueue>,
}

impl SnapshotPublisher {
    pub fn publish(&self, snapshot: PlayerSnapshot) -> bool {
        let Some(queue) = self.queue.upgrade() else {
            return false;
        };
        let mut values = queue
            .values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        values.clear();
        values.push_back(snapshot);
        queue.ready.notify_one();
        true
    }
}

pub struct SnapshotSubscription {
    queue: Arc<SnapshotQueue>,
}

impl SnapshotSubscription {
    pub fn coalescing() -> (SnapshotPublisher, Self) {
        let queue = Arc::new(SnapshotQueue {
            values: Mutex::new(VecDeque::with_capacity(1)),
            ready: Condvar::new(),
        });
        (
            SnapshotPublisher {
                queue: Arc::downgrade(&queue),
            },
            Self { queue },
        )
    }

    pub fn try_recv(&self) -> Option<PlayerSnapshot> {
        self.queue
            .values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Option<PlayerSnapshot> {
        let deadline = Instant::now() + timeout;
        let mut values = self
            .queue
            .values
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if let Some(snapshot) = values.pop_front() {
                return Some(snapshot);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let result = self.queue.ready.wait_timeout(values, remaining);
            let (next_values, wait_result) =
                result.unwrap_or_else(std::sync::PoisonError::into_inner);
            values = next_values;
            if wait_result.timed_out() {
                return values.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorCode, TransportState};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};

    struct FakeController {
        sender: SyncSender<PlayerCommand>,
        snapshot: PlayerSnapshot,
        next_id: AtomicU64,
    }

    impl PlayerController for FakeController {
        fn submit(&self, command: PlayerCommand) -> Result<CommandId, PlayerError> {
            self.sender.try_send(command).map_err(|error| match error {
                TrySendError::Full(_) => {
                    PlayerError::new(ErrorCode::CommandQueueFull, "Command queue is full")
                }
                TrySendError::Disconnected(_) => {
                    PlayerError::new(ErrorCode::PlaybackSessionClosed, "Controller is closed")
                }
            })?;
            Ok(CommandId::new(self.next_id.fetch_add(1, Ordering::Relaxed)))
        }

        fn latest_snapshot(&self) -> PlayerSnapshot {
            self.snapshot.clone()
        }

        fn subscribe(&self) -> SnapshotSubscription {
            SnapshotSubscription::coalescing().1
        }
    }

    #[test]
    fn fake_controller_command_queue_is_bounded() {
        let (sender, _receiver) = sync_channel(1);
        let controller = FakeController {
            sender,
            snapshot: PlayerSnapshot::empty(TransportState::Empty),
            next_id: AtomicU64::new(1),
        };
        assert_eq!(controller.submit(PlayerCommand::Shutdown).unwrap().get(), 1);
        let error = controller.submit(PlayerCommand::Shutdown).unwrap_err();
        assert_eq!(error.code, ErrorCode::CommandQueueFull);
    }

    #[test]
    fn snapshot_delivery_coalesces_to_the_latest_value() {
        let (publisher, subscription) = SnapshotSubscription::coalescing();
        publisher.publish(PlayerSnapshot::empty(TransportState::Ready));
        publisher.publish(PlayerSnapshot::empty(TransportState::Playing));
        assert_eq!(
            subscription.try_recv().unwrap().transport_state(),
            TransportState::Playing
        );
        assert!(subscription.try_recv().is_none());
    }
}
