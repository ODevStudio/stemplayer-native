#![deny(unsafe_code)]

mod audit;
mod clock;
mod device;
mod lifecycle;
mod qualification;
mod resampler;
mod ring;

pub use audit::{CallbackAuditReport, ReopenAuditReport, callback_audit, reopen_audit};
pub use clock::{AudioClock, ClockSnapshot, LatencyConfidence};
pub use device::{
    DeviceOutput, DeviceReport, OutputTestOptions, OutputTestReport, enumerate_devices,
    run_output_test,
};
pub use lifecycle::{DeviceLifecycle, LifecycleAction, LifecycleState};
pub use qualification::{
    HardwareReopenReport, OperatorRecoveryReport, hardware_reopen_test, operator_recovery_test,
};
pub use resampler::PostMixResampler;
pub use ring::{OutputConsumer, OutputMetrics, OutputProducer, StereoFrame, output_ring};
