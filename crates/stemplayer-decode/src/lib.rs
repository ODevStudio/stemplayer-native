#![deny(unsafe_code)]

use stemplayer_domain::{GenerationId, PlayerError};

pub trait Decoder: Send {
    fn cancel(&mut self, generation: GenerationId) -> Result<(), PlayerError>;
}
