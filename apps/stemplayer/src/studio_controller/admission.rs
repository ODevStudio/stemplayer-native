use super::*;

impl StudioController {
    pub(super) fn signal_shutdown(&self) {
        let _admission = self
            .shared
            .command_admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.shared.shutdown.store(true, Ordering::Release);
        let cancelled = self
            .shared
            .cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cancelled.store(true, Ordering::Release);
        let _ = self.sender.try_send(QueuedCommand {
            command: WorkerCommand::Player(PlayerCommand::Shutdown),
            epoch: self.shared.epoch.load(Ordering::Acquire),
            transport_revision: self.shared.transport_revision.load(Ordering::Acquire),
            cancelled: Arc::clone(&cancelled),
        });
    }

    pub(super) fn next_id(&self) -> Result<CommandId, PlayerError> {
        self.next_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map(CommandId::new)
            .map_err(|_| {
                PlayerError::new(
                    ErrorCode::ValueOutOfRange,
                    "Command identifier is exhausted",
                )
            })
    }

    pub(super) fn enqueue(
        &self,
        command: WorkerCommand,
        invalidates: bool,
    ) -> Result<(), PlayerError> {
        self.enqueue_if_current(command, invalidates, None)
            .map(|_| ())
    }

    pub(super) fn enqueue_if_current(
        &self,
        command: WorkerCommand,
        invalidates: bool,
        expected: Option<&SourceSnapshot>,
    ) -> Result<bool, PlayerError> {
        let _admission = self
            .shared
            .command_admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(PlayerError::new(
                ErrorCode::PlaybackSessionClosed,
                "Controller is closed",
            ));
        }
        let current_epoch = self.shared.epoch.load(Ordering::Acquire);
        let current_transport_revision = self.shared.transport_revision.load(Ordering::Acquire);
        let mut current_cancel = self
            .shared
            .cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if expected.is_some_and(|evidence| {
            !evidence.matches_admission(current_epoch, current_transport_revision, &current_cancel)
        }) {
            return Ok(false);
        }
        let changes_transport_intent = invalidates
            || matches!(
                &command,
                WorkerCommand::Player(
                    PlayerCommand::Play
                        | PlayerCommand::Pause
                        | PlayerCommand::Stop
                        | PlayerCommand::SeekFrame(_)
                        | PlayerCommand::SeekTime(_)
                )
            );
        let transport_revision = if changes_transport_intent {
            current_transport_revision.checked_add(1).ok_or_else(|| {
                PlayerError::new(
                    ErrorCode::ValueOutOfRange,
                    "Transport intent revision is exhausted",
                )
            })?
        } else {
            current_transport_revision
        };
        let epoch = if invalidates {
            current_epoch.checked_add(1).ok_or_else(|| {
                PlayerError::new(ErrorCode::ValueOutOfRange, "Source operation is exhausted")
            })?
        } else {
            current_epoch
        };
        let cancelled = if invalidates {
            Arc::new(AtomicBool::new(false))
        } else {
            Arc::clone(&current_cancel)
        };
        let play_requested = match &command {
            WorkerCommand::SourceRecovery { play, .. } => Some(*play),
            WorkerCommand::Player(PlayerCommand::Play) => Some(true),
            WorkerCommand::Player(
                PlayerCommand::Open(_)
                | PlayerCommand::Pause
                | PlayerCommand::Stop
                | PlayerCommand::Unload,
            ) => Some(false),
            _ => None,
        };
        self.sender
            .try_send(QueuedCommand {
                command,
                epoch,
                transport_revision,
                cancelled: Arc::clone(&cancelled),
            })
            .map_err(command_send_error)?;
        if let Some(play_requested) = play_requested {
            self.shared
                .play_requested
                .store(play_requested, Ordering::Release);
        }
        if invalidates {
            current_cancel.store(true, Ordering::Release);
            *current_cancel = cancelled;
            self.shared.epoch.store(epoch, Ordering::Release);
        }
        if changes_transport_intent {
            self.shared
                .transport_revision
                .store(transport_revision, Ordering::Release);
        }
        Ok(true)
    }
}
