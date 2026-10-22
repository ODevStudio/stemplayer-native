#![deny(unsafe_code)]

mod analyzer;
mod cache;
mod cache_io;
mod cache_root;
mod fingerprint;
mod priority;
mod viewport;

pub use analyzer::{
    CacheStatus, ConsistencySummary, Peak, PeakAnalyzer, PeakLevel, WaveformSnapshot,
};
pub use cache::{CacheConfig, CacheResult, WaveformCache};
pub use fingerprint::{SourceEvidence, SourceFingerprint};
pub use priority::{AnalysisDecision, AnalysisPriority};
pub use viewport::{PeakColumn, ViewportGeometry, project_viewport};
