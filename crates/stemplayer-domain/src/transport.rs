use crate::{ErrorCode, PlayerError};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransportState {
    #[default]
    Empty,
    Inspecting,
    Ready,
    Priming,
    Playing,
    Paused,
    Seeking,
    Ended,
    Error,
    Closed,
}

impl TransportState {
    pub const ALL: [Self; 10] = [
        Self::Empty,
        Self::Inspecting,
        Self::Ready,
        Self::Priming,
        Self::Playing,
        Self::Paused,
        Self::Seeking,
        Self::Ended,
        Self::Error,
        Self::Closed,
    ];

    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Empty, Self::Inspecting)
                | (Self::Inspecting, Self::Ready | Self::Error)
                | (Self::Ready, Self::Priming | Self::Seeking)
                | (
                    Self::Priming,
                    Self::Playing | Self::Paused | Self::Seeking | Self::Error
                )
                | (
                    Self::Playing,
                    Self::Paused | Self::Seeking | Self::Ended | Self::Error
                )
                | (
                    Self::Paused,
                    Self::Priming | Self::Seeking | Self::Ready | Self::Error
                )
                | (
                    Self::Seeking,
                    Self::Priming | Self::Playing | Self::Paused | Self::Error
                )
                | (Self::Ended, Self::Priming | Self::Seeking | Self::Ready)
                | (Self::Error, Self::Empty | Self::Closed)
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct TransportMachine {
    state: TransportState,
}

impl TransportMachine {
    pub const fn new() -> Self {
        Self {
            state: TransportState::Empty,
        }
    }

    pub const fn state(self) -> TransportState {
        self.state
    }

    pub fn transition(&mut self, next: TransportState) -> Result<(), PlayerError> {
        if self.state == TransportState::Closed {
            return Err(PlayerError::new(
                ErrorCode::PlaybackSessionClosed,
                "Playback session is closed",
            ));
        }
        if !self.state.can_transition_to(next) {
            return Err(PlayerError::new(
                ErrorCode::TransportStateInvalid,
                "Transport transition is not allowed",
            ));
        }
        self.state = next;
        Ok(())
    }

    pub fn close(&mut self) {
        self.state = TransportState::Closed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected(from: TransportState, to: TransportState) -> bool {
        match from {
            TransportState::Empty => matches!(to, TransportState::Inspecting),
            TransportState::Inspecting => {
                matches!(to, TransportState::Ready | TransportState::Error)
            }
            TransportState::Ready => {
                matches!(to, TransportState::Priming | TransportState::Seeking)
            }
            TransportState::Priming => matches!(
                to,
                TransportState::Playing
                    | TransportState::Paused
                    | TransportState::Seeking
                    | TransportState::Error
            ),
            TransportState::Playing => matches!(
                to,
                TransportState::Paused
                    | TransportState::Seeking
                    | TransportState::Ended
                    | TransportState::Error
            ),
            TransportState::Paused => matches!(
                to,
                TransportState::Priming
                    | TransportState::Seeking
                    | TransportState::Ready
                    | TransportState::Error
            ),
            TransportState::Seeking => matches!(
                to,
                TransportState::Priming
                    | TransportState::Playing
                    | TransportState::Paused
                    | TransportState::Error
            ),
            TransportState::Ended => matches!(
                to,
                TransportState::Priming | TransportState::Seeking | TransportState::Ready
            ),
            TransportState::Error => {
                matches!(to, TransportState::Empty | TransportState::Closed)
            }
            TransportState::Closed => false,
        }
    }

    #[test]
    fn every_state_pair_matches_the_contract_table() {
        for from in TransportState::ALL {
            for to in TransportState::ALL {
                assert_eq!(
                    from.can_transition_to(to),
                    expected(from, to),
                    "{from:?} -> {to:?}"
                );
            }
        }
    }

    #[test]
    fn rejected_transition_does_not_change_state() {
        let mut machine = TransportMachine::new();
        let error = machine.transition(TransportState::Playing).unwrap_err();
        assert_eq!(error.code, ErrorCode::TransportStateInvalid);
        assert_eq!(machine.state(), TransportState::Empty);
    }

    #[test]
    fn close_is_idempotent_and_terminal() {
        let mut machine = TransportMachine::new();
        machine.close();
        machine.close();
        assert_eq!(machine.state(), TransportState::Closed);
        let error = machine.transition(TransportState::Empty).unwrap_err();
        assert_eq!(error.code, ErrorCode::PlaybackSessionClosed);
    }
}
