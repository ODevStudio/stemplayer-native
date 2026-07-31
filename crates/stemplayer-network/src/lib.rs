#![deny(unsafe_code)]

#[cfg(feature = "hls")]
mod client;
#[cfg(feature = "hls")]
mod http;
#[cfg(feature = "hls")]
mod manifest;
#[cfg(feature = "hls")]
mod playlist;
#[cfg(feature = "hls")]
mod source;
#[cfg(feature = "hls")]
mod url_policy;

#[cfg(feature = "hls")]
pub use client::{HlsClient, HttpTransport};
#[cfg(feature = "hls")]
pub use http::UreqTransport;
#[cfg(feature = "hls")]
pub use manifest::{HlsManifest, HlsRole, HlsWaveform};
#[cfg(feature = "hls")]
pub use playlist::{MediaPlaylist, SegmentDescriptor};
#[cfg(feature = "hls")]
pub use source::{HlsDiagnostics, HlsPlaybackSource};

use stemplayer_domain::{PlayerError, SourceLocator};

pub trait NetworkService: Send + Sync {
    fn validate_source(&self, source: &SourceLocator) -> Result<(), PlayerError>;
}
