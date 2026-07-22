#![deny(unsafe_code)]

use stemplayer_domain::PlayerError;

pub trait WaveformService: Send + Sync {
    fn clear_cache(&self) -> Result<(), PlayerError>;
}
