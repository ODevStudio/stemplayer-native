#![deny(unsafe_code)]

mod command_adapter;
mod playhead;
mod presentation;

#[cfg(feature = "development")]
mod mock;

pub use command_adapter::CommandAdapter;
pub use playhead::Playhead;
pub use presentation::{
    MixerRow, PresentationModel, TransportAvailability, gain_db_from_position,
    gain_position_from_db,
};

#[cfg(feature = "development")]
pub use mock::MockController;

use stemplayer_domain::PlayerSnapshot;

pub trait UiPort {
    fn present(&self, snapshot: PlayerSnapshot);
}
