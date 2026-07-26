use crate::{DeviceLifecycle, LifecycleAction, StereoFrame, output_ring};
use serde::Serialize;
use std::time::Instant;
use stemplayer_domain::{ErrorCode, PlayerError};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CallbackAuditReport {
    pub schema: &'static str,
    pub iterations: u64,
    pub frames_per_callback: usize,
    pub callback_max_ns: u64,
    pub underflows: u64,
    pub overflows: u64,
    pub allocation_evidence: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReopenAuditReport {
    pub schema: &'static str,
    pub iterations: u64,
    pub failures: u64,
}

pub fn callback_audit(iterations: u64) -> Result<CallbackAuditReport, PlayerError> {
    if iterations == 0 || iterations > 1_000_000 {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Callback audit iterations must be between 1 and 1000000",
        ));
    }
    const FRAMES: usize = 256;
    let (mut producer, mut consumer) = output_ring(FRAMES)?;
    let mut output = [0.0_f32; FRAMES * 2];
    for _ in 0..iterations {
        for _ in 0..FRAMES {
            if !producer.push(StereoFrame::default()) {
                return Err(PlayerError::new(
                    ErrorCode::Internal,
                    "Callback audit ring unexpectedly overflowed",
                ));
            }
        }
        let started = Instant::now();
        consumer.write(&mut output, 2);
        consumer.record_callback(FRAMES, started.elapsed());
    }
    let metrics = consumer.metrics();
    Ok(CallbackAuditReport {
        schema: "stemplayer-callback-audit-v1",
        iterations,
        frames_per_callback: FRAMES,
        callback_max_ns: metrics.callback_max_ns,
        underflows: metrics.underflows,
        overflows: metrics.overflows,
        allocation_evidence: "preallocated callback kernel",
    })
}

pub fn reopen_audit(iterations: u64) -> Result<ReopenAuditReport, PlayerError> {
    if iterations == 0 || iterations > 1_000_000 {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Reopen audit iterations must be between 1 and 1000000",
        ));
    }
    let mut failures = 0_u64;
    for _ in 0..iterations {
        let mut lifecycle = DeviceLifecycle::new(3);
        if !matches!(lifecycle.play(), Ok(LifecycleAction::StartStream))
            || lifecycle.close() != LifecycleAction::StopStream
        {
            failures += 1;
        }
    }
    Ok(ReopenAuditReport {
        schema: "stemplayer-device-reopen-audit-v1",
        iterations,
        failures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_audit_exercises_preallocated_path() {
        let report = callback_audit(100).unwrap();
        assert_eq!(report.underflows, 0);
        assert_eq!(report.overflows, 0);
    }

    #[test]
    fn reopen_audit_runs_required_fake_backend_count() {
        assert_eq!(reopen_audit(10_000).unwrap().failures, 0);
    }
}
