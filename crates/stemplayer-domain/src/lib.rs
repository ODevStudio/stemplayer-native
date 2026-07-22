#![deny(unsafe_code)]

mod command;
mod controller;
mod diagnostic;
mod error;
mod role;
mod snapshot;
mod transport;
mod value;

pub use command::{CacheCommand, PlayerCommand, SourceLocator};
pub use controller::{PlayerController, SnapshotPublisher, SnapshotSubscription};
pub use diagnostic::{DiagnosticDetail, PublicDiagnostic};
pub use error::{ErrorCategory, ErrorCode, ErrorDetail, PlayerError, Recoverability, Severity};
pub use role::{OutputMode, OutputPreference, SourceKind, StemRole, TrackRole};
pub use snapshot::{
    MAX_SNAPSHOT_DIAGNOSTICS, MixerSnapshot, PlayerSnapshot, SnapshotDetails, SourceMetadata,
    StemSnapshot, WaveformState,
};
pub use transport::{TransportMachine, TransportState};
pub use value::{
    BoundedText, ChannelCount, CommandId, DeviceId, DisplayName, FrameCount, GainDb, GenerationId,
    Microseconds, PlaybackPosition, SampleRate, SourceFrame,
};
