#![deny(unsafe_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::sync::{Mutex, RwLock};
use stemplayer_domain::{
    CommandId, ErrorCode, PlayerCommand, PlayerController, PlayerError, PlayerSnapshot,
    SnapshotPublisher, SnapshotSubscription, TransportState,
};

#[derive(Debug)]
pub struct QueuedCommand {
    pub id: CommandId,
    pub command: PlayerCommand,
}

pub struct HeadlessController {
    sender: SyncSender<QueuedCommand>,
    receiver: Mutex<Receiver<QueuedCommand>>,
    snapshot: RwLock<PlayerSnapshot>,
    subscribers: Mutex<Vec<SnapshotPublisher>>,
    next_id: AtomicU64,
}

impl HeadlessController {
    pub fn new(command_capacity: usize) -> Result<Self, PlayerError> {
        if command_capacity == 0 {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Command queue capacity must be positive",
            ));
        }
        let (sender, receiver) = sync_channel(command_capacity);
        Ok(Self {
            sender,
            receiver: Mutex::new(receiver),
            snapshot: RwLock::new(PlayerSnapshot::empty(TransportState::Empty)),
            subscribers: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        })
    }

    pub fn try_take_command(&self) -> Result<Option<QueuedCommand>, PlayerError> {
        let receiver = self
            .receiver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match receiver.try_recv() {
            Ok(command) => Ok(Some(command)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(PlayerError::new(
                ErrorCode::PlaybackSessionClosed,
                "Controller is closed",
            )),
        }
    }

    pub fn publish_snapshot(&self, snapshot: PlayerSnapshot) {
        *self
            .snapshot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = snapshot.clone();
        self.subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|publisher| publisher.publish(snapshot.clone()));
    }

    pub fn unsupported(command: &QueuedCommand) -> PlayerError {
        PlayerError::new(
            ErrorCode::FeatureUnsupported,
            format!(
                "Command {:?} is not implemented in Phase 1",
                command.command
            ),
        )
    }
}

impl PlayerController for HeadlessController {
    fn submit(&self, command: PlayerCommand) -> Result<CommandId, PlayerError> {
        let id = self
            .next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map(CommandId::new)
            .map_err(|_| {
                PlayerError::new(
                    ErrorCode::ValueOutOfRange,
                    "Command identifier is exhausted",
                )
            })?;
        self.sender
            .try_send(QueuedCommand { id, command })
            .map_err(|error| match error {
                TrySendError::Full(_) => {
                    PlayerError::new(ErrorCode::CommandQueueFull, "Command queue is full")
                }
                TrySendError::Disconnected(_) => {
                    PlayerError::new(ErrorCode::PlaybackSessionClosed, "Controller is closed")
                }
            })?;
        Ok(id)
    }

    fn latest_snapshot(&self) -> PlayerSnapshot {
        self.snapshot
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn subscribe(&self) -> SnapshotSubscription {
        let (publisher, subscription) = SnapshotSubscription::coalescing();
        self.subscribers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(publisher);
        subscription
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_has_bounded_commands_and_no_worker_thread() {
        let controller = HeadlessController::new(1).unwrap();
        let id = controller.submit(PlayerCommand::Play).unwrap();
        assert_eq!(id.get(), 1);
        assert_eq!(
            controller.submit(PlayerCommand::Pause).unwrap_err().code,
            ErrorCode::CommandQueueFull
        );
        let command = controller.try_take_command().unwrap().unwrap();
        assert_eq!(command.id, id);
        assert_eq!(
            HeadlessController::unsupported(&command).code,
            ErrorCode::FeatureUnsupported
        );
    }

    #[test]
    fn subscribers_receive_only_the_latest_snapshot() {
        let controller = HeadlessController::new(1).unwrap();
        let subscription = controller.subscribe();
        controller.publish_snapshot(PlayerSnapshot::empty(TransportState::Ready));
        controller.publish_snapshot(PlayerSnapshot::empty(TransportState::Playing));
        assert_eq!(
            subscription.try_recv().unwrap().transport_state(),
            TransportState::Playing
        );
    }
}
