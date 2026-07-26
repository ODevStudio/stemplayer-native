use crate::{
    DeviceLifecycle, LifecycleAction, OutputTestOptions, OutputTestReport, run_output_test,
};
use serde::Serialize;
use std::thread;
use stemplayer_domain::{ErrorCode, PlayerError};

#[derive(Clone, Debug, Serialize)]
pub struct OperatorRecoveryReport {
    pub schema: &'static str,
    pub fault_observed: bool,
    pub recovered_paused: bool,
    pub recovery_attempts: u8,
    pub active_output: Option<OutputTestReport>,
    pub recovery_output: Option<OutputTestReport>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HardwareReopenReport {
    pub schema: &'static str,
    pub iterations: u32,
    pub successful_cycles: u32,
    pub failed_cycles: u32,
    pub underflows: u64,
    pub overflows: u64,
    pub stream_errors: u64,
    pub callback_max_ns: u64,
    pub shutdown_max_ms: u128,
}

pub fn operator_recovery_test(wait_seconds: u64) -> Result<OperatorRecoveryReport, PlayerError> {
    if wait_seconds == 0 || wait_seconds > 600 {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Recovery observation must be between 1 and 600 seconds",
        ));
    }
    let active = run_output_test(OutputTestOptions {
        seconds: wait_seconds,
        ..OutputTestOptions::default()
    });
    let fault_observed = active
        .as_ref()
        .map_or(true, |result| result.stream_errors > 0);
    if !fault_observed {
        return Ok(OperatorRecoveryReport {
            schema: "stemplayer-device-recovery-test-v1",
            fault_observed: false,
            recovered_paused: false,
            recovery_attempts: 0,
            active_output: active.ok(),
            recovery_output: None,
        });
    }

    let mut lifecycle = DeviceLifecycle::new(3);
    lifecycle.device_lost();
    let mut recovery_attempts = 0;
    while let LifecycleAction::ReopenAfter(delay) = lifecycle.retry_reopen() {
        recovery_attempts += 1;
        thread::sleep(delay);
        let recovery = run_output_test(OutputTestOptions {
            seconds: 1,
            amplitude: 0.0,
            ..OutputTestOptions::default()
        });
        if recovery
            .as_ref()
            .is_ok_and(|result| result.stream_errors == 0)
        {
            lifecycle.reopened();
            return Ok(OperatorRecoveryReport {
                schema: "stemplayer-device-recovery-test-v1",
                fault_observed: true,
                recovered_paused: lifecycle.state() == crate::LifecycleState::Paused,
                recovery_attempts,
                active_output: active.ok(),
                recovery_output: recovery.ok(),
            });
        }
    }
    Ok(OperatorRecoveryReport {
        schema: "stemplayer-device-recovery-test-v1",
        fault_observed: true,
        recovered_paused: false,
        recovery_attempts,
        active_output: active.ok(),
        recovery_output: None,
    })
}

pub fn hardware_reopen_test(
    options: OutputTestOptions,
    iterations: u32,
) -> Result<HardwareReopenReport, PlayerError> {
    if iterations == 0 || iterations > 100 {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Hardware reopen iterations must be between 1 and 100",
        ));
    }
    let mut report = HardwareReopenReport {
        schema: "stemplayer-device-reopen-test-v1",
        iterations,
        successful_cycles: 0,
        failed_cycles: 0,
        underflows: 0,
        overflows: 0,
        stream_errors: 0,
        callback_max_ns: 0,
        shutdown_max_ms: 0,
    };
    for _ in 0..iterations {
        match run_output_test(options.clone()) {
            Ok(result) => {
                report.successful_cycles += 1;
                report.underflows = report.underflows.saturating_add(result.metrics.underflows);
                report.overflows = report.overflows.saturating_add(result.metrics.overflows);
                report.stream_errors = report.stream_errors.saturating_add(result.stream_errors);
                report.callback_max_ns = report.callback_max_ns.max(result.metrics.callback_max_ns);
                report.shutdown_max_ms = report.shutdown_max_ms.max(result.shutdown_time_ms);
            }
            Err(_) => report.failed_cycles += 1,
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_observation_is_bounded_before_device_access() {
        assert!(operator_recovery_test(0).is_err());
        assert!(operator_recovery_test(601).is_err());
    }

    #[test]
    fn hardware_iteration_count_is_bounded_before_device_access() {
        assert!(hardware_reopen_test(OutputTestOptions::default(), 0).is_err());
        assert!(hardware_reopen_test(OutputTestOptions::default(), 101).is_err());
    }
}
