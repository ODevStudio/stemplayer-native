use std::sync::{Arc, Mutex};
use stemplayer_domain::{
    CommandId, GainDb, PlayerCommand, PlayerController, PlayerError, SourceFrame, StemRole,
};

#[derive(Default)]
struct PendingCommands {
    gains: [Option<GainDb>; 4],
    seek: Option<SourceFrame>,
}

pub struct CommandAdapter {
    controller: Arc<dyn PlayerController>,
    pending: Mutex<PendingCommands>,
}

impl CommandAdapter {
    pub fn new(controller: Arc<dyn PlayerController>) -> Self {
        Self {
            controller,
            pending: Mutex::new(PendingCommands::default()),
        }
    }

    pub fn submit(&self, command: PlayerCommand) -> Result<CommandId, PlayerError> {
        self.controller.submit(command)
    }

    pub fn preview_gain(&self, role: StemRole, gain: GainDb) {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .gains[role.index()] = Some(gain);
    }

    pub fn preview_seek(&self, frame: SourceFrame) {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .seek = Some(frame);
    }

    pub fn flush(&self) -> Vec<Result<CommandId, PlayerError>> {
        let pending = {
            let mut locked = self
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *locked)
        };
        StemRole::ALL
            .into_iter()
            .zip(pending.gains)
            .filter_map(|(role, gain)| gain.map(|gain_db| PlayerCommand::SetGain { role, gain_db }))
            .chain(pending.seek.map(PlayerCommand::SeekFrame))
            .map(|command| self.controller.submit(command))
            .collect()
    }

    pub fn close(&self) -> Result<CommandId, PlayerError> {
        *self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = PendingCommands::default();
        self.controller.submit(PlayerCommand::Shutdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use stemplayer_domain::{PlayerSnapshot, SnapshotSubscription, TransportState};

    struct Recorder(Mutex<Vec<PlayerCommand>>);

    impl PlayerController for Recorder {
        fn submit(&self, command: PlayerCommand) -> Result<CommandId, PlayerError> {
            let mut commands = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            commands.push(command);
            Ok(CommandId::new(commands.len() as u64))
        }

        fn latest_snapshot(&self) -> PlayerSnapshot {
            PlayerSnapshot::empty(TransportState::Empty)
        }

        fn subscribe(&self) -> SnapshotSubscription {
            SnapshotSubscription::coalescing().1
        }
    }

    #[test]
    fn slider_and_seek_floods_coalesce() {
        let recorder = Arc::new(Recorder(Mutex::new(Vec::new())));
        let adapter = CommandAdapter::new(recorder.clone());
        for value in -30..=0 {
            adapter.preview_gain(StemRole::Vocals, GainDb::new(value as f32).unwrap());
            adapter.preview_seek(SourceFrame::new(value as i64 + 30));
        }
        assert_eq!(adapter.flush().len(), 2);
        let commands = recorder
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(commands.len(), 2);
        assert!(matches!(
            commands[0],
            PlayerCommand::SetGain {
                role: StemRole::Vocals,
                gain_db
            } if gain_db == GainDb::UNITY
        ));
        assert_eq!(commands[1], PlayerCommand::SeekFrame(SourceFrame::new(30)));
    }
}
