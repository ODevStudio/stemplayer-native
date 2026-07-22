#![deny(unsafe_code)]

use stemplayer_domain::{PlayerError, SourceLocator, SourceMetadata};

pub trait SourceInspector: Send + Sync {
    fn inspect(&self, source: &SourceLocator) -> Result<SourceMetadata, PlayerError>;
}
