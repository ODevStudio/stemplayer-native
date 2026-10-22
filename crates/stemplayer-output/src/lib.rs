#![deny(unsafe_code)]

mod audit;
mod buffer;
mod clock;
mod conversion;
mod device;
mod identity;
mod lifecycle;
mod presentation;
mod qualification;
mod resampler;
mod ring;

pub use audit::{CallbackAuditReport, ReopenAuditReport, callback_audit, reopen_audit};
pub use buffer::output_refill_interval;
pub use clock::{AudioClock, ClockSnapshot, LatencyConfidence};
pub use device::{
    DeviceOutput, DeviceReport, OutputTestOptions, OutputTestReport, enumerate_devices,
    run_output_test,
};
pub use lifecycle::{DeviceLifecycle, LifecycleAction, LifecycleState};
pub use presentation::PresentationEstimate;
pub use qualification::{
    HardwareReopenReport, OperatorRecoveryReport, hardware_reopen_test, operator_recovery_test,
};
pub use resampler::PostMixResampler;
pub use ring::{OutputConsumer, OutputMetrics, OutputProducer, StereoFrame, output_ring};
