#![deny(unsafe_code)]

use stemplayer_domain::PlayerError;

pub trait OutputPort: Send {
    fn stop(&mut self) -> Result<(), PlayerError>;
}
