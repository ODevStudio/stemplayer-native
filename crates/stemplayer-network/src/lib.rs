#![deny(unsafe_code)]

use stemplayer_domain::{PlayerError, SourceLocator};

pub trait NetworkService: Send + Sync {
    fn validate_source(&self, source: &SourceLocator) -> Result<(), PlayerError>;
}
