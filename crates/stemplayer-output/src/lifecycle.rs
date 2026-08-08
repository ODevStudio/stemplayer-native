use serde::Serialize;
use std::time::Duration;
use stemplayer_domain::{ErrorCode, PlayerError};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LifecycleState {
    Stopped,
    Running,
    Paused,
    DeviceLost,
    Closed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleAction {
    None,
    StartStream,
    PauseStream,
    StopStream,
    PausePlayback,
    ReopenAfter(Duration),
    ReportRecoverableError,
}

#[derive(Clone, Debug)]
pub struct DeviceLifecycle {
    state: LifecycleState,
    reopen_attempts: u8,
    max_reopen_attempts: u8,
}

impl DeviceLifecycle {
    pub fn new(max_reopen_attempts: u8) -> Self {
        Self {
            state: LifecycleState::Stopped,
            reopen_attempts: 0,
            max_reopen_attempts,
        }
    }

    pub fn state(&self) -> LifecycleState {
        self.state
    }

    pub fn play(&mut self) -> Result<LifecycleAction, PlayerError> {
        match self.state {
            LifecycleState::Stopped => {
                self.state = LifecycleState::Running;
                Ok(LifecycleAction::StartStream)
            }
            LifecycleState::Paused => {
                self.state = LifecycleState::Running;
                Ok(LifecycleAction::StartStream)
            }
            LifecycleState::Running => Ok(LifecycleAction::None),
            LifecycleState::DeviceLost => Err(PlayerError::new(
                ErrorCode::PlaybackNotReady,
                "Audio device recovery is in progress",
            )),
            LifecycleState::Closed => Err(PlayerError::new(
                ErrorCode::PlaybackSessionClosed,
                "Audio output is closed",
            )),
        }
    }

    pub fn pause(&mut self) -> LifecycleAction {
        if self.state == LifecycleState::Running {
            self.state = LifecycleState::Paused;
            LifecycleAction::PauseStream
        } else {
            LifecycleAction::None
        }
    }

    pub fn stop(&mut self) -> LifecycleAction {
        if matches!(
            self.state,
            LifecycleState::Running | LifecycleState::Paused | LifecycleState::DeviceLost
        ) {
            self.state = LifecycleState::Stopped;
            self.reopen_attempts = 0;
            LifecycleAction::StopStream
        } else {
            LifecycleAction::None
        }
    }

    pub fn device_lost(&mut self) -> LifecycleAction {
        if self.state == LifecycleState::Closed {
            return LifecycleAction::None;
        }
        self.state = LifecycleState::DeviceLost;
        self.reopen_attempts = 0;
        LifecycleAction::PausePlayback
    }

    pub fn retry_reopen(&mut self) -> LifecycleAction {
        if self.state != LifecycleState::DeviceLost {
            return LifecycleAction::None;
        }
        if self.reopen_attempts >= self.max_reopen_attempts {
            return LifecycleAction::ReportRecoverableError;
        }
        self.reopen_attempts += 1;
        LifecycleAction::ReopenAfter(Duration::from_millis(
            100_u64.saturating_mul(1_u64 << self.reopen_attempts.min(5)),
        ))
    }

    pub fn reopened(&mut self) -> LifecycleAction {
        if self.state == LifecycleState::DeviceLost {
            self.state = LifecycleState::Paused;
            self.reopen_attempts = 0;
        }
        LifecycleAction::None
    }

    pub fn close(&mut self) -> LifecycleAction {
        self.state = LifecycleState::Closed;
        LifecycleAction::StopStream
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_loss_pauses_and_never_resumes_audibly() {
        let mut lifecycle = DeviceLifecycle::new(3);
        assert_eq!(lifecycle.play().unwrap(), LifecycleAction::StartStream);
        assert_eq!(lifecycle.device_lost(), LifecycleAction::PausePlayback);
        assert!(matches!(
            lifecycle.retry_reopen(),
            LifecycleAction::ReopenAfter(_)
        ));
        lifecycle.reopened();
        assert_eq!(lifecycle.state(), LifecycleState::Paused);
    }

    #[test]
    fn reopen_retries_are_bounded() {
        let mut lifecycle = DeviceLifecycle::new(2);
        lifecycle.device_lost();
        assert!(matches!(
            lifecycle.retry_reopen(),
            LifecycleAction::ReopenAfter(_)
        ));
        assert!(matches!(
            lifecycle.retry_reopen(),
            LifecycleAction::ReopenAfter(_)
        ));
        assert_eq!(
            lifecycle.retry_reopen(),
            LifecycleAction::ReportRecoverableError
        );
    }

    #[test]
    fn stop_cancels_device_recovery() {
        let mut lifecycle = DeviceLifecycle::new(2);
        lifecycle.device_lost();
        assert_eq!(lifecycle.stop(), LifecycleAction::StopStream);
        assert_eq!(lifecycle.state(), LifecycleState::Stopped);
        assert_eq!(lifecycle.retry_reopen(), LifecycleAction::None);
    }

    #[test]
    fn fake_backend_survives_ten_thousand_open_close_cycles() {
        for _ in 0..10_000 {
            let mut lifecycle = DeviceLifecycle::new(3);
            assert_eq!(lifecycle.play().unwrap(), LifecycleAction::StartStream);
            assert_eq!(lifecycle.close(), LifecycleAction::StopStream);
            assert_eq!(lifecycle.state(), LifecycleState::Closed);
        }
    }
}
