use crate::{ErrorCode, PlayerError};
use serde::Serialize;
use std::fmt::{Display, Formatter};

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct BoundedText<const LIMIT: usize>(String);

impl<const LIMIT: usize> BoundedText<LIMIT> {
    pub fn new(value: impl Into<String>) -> Result<Self, PlayerError> {
        let value = value.into();
        if value.len() > LIMIT {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Text exceeds its configured limit",
            ));
        }
        Ok(Self(value))
    }

    pub fn truncated(value: impl AsRef<str>) -> Self {
        let value = value.as_ref();
        if value.len() <= LIMIT {
            return Self(value.to_owned());
        }
        let end = value
            .char_indices()
            .map(|(index, _)| index)
            .take_while(|index| *index <= LIMIT)
            .last()
            .unwrap_or(0);
        Self(value[..end].to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<const LIMIT: usize> Display for BoundedText<LIMIT> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

pub type DisplayName = BoundedText<256>;
pub type DeviceId = BoundedText<128>;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SourceFrame(i64);

impl SourceFrame {
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FrameCount(u64);

impl FrameCount {
    pub fn new(value: u64) -> Result<Self, PlayerError> {
        if value > i64::MAX as u64 {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Frame count exceeds the signed timeline",
            ));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SampleRate(u32);

impl SampleRate {
    pub fn new(value: u32) -> Result<Self, PlayerError> {
        if value == 0 {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Sample rate must be positive",
            ));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ChannelCount(u8);

impl ChannelCount {
    pub fn new(value: u8) -> Result<Self, PlayerError> {
        if value == 0 {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Channel count must be positive",
            ));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Microseconds(i64);

impl Microseconds {
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GenerationId(u64);

impl GenerationId {
    pub const INITIAL: Self = Self(0);

    pub fn next(self) -> Result<Self, PlayerError> {
        self.0.checked_add(1).map(Self).ok_or_else(|| {
            PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Generation identifier is exhausted",
            )
        })
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CommandId(u64);

impl CommandId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct GainDb(f32);

impl GainDb {
    pub const MIN: f32 = -60.0;
    pub const MAX: f32 = 6.0;
    pub const UNITY: Self = Self(0.0);
    pub const SILENCE: Self = Self(Self::MIN);

    pub fn new(value: f32) -> Result<Self, PlayerError> {
        if !value.is_finite() || !(Self::MIN..=Self::MAX).contains(&value) {
            return Err(PlayerError::new(
                ErrorCode::MixerGainInvalid,
                "Gain must be finite and between -60 dB and +6 dB",
            ));
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> f32 {
        self.0
    }

    pub fn linear(self) -> f32 {
        if self.0 == Self::MIN {
            0.0
        } else if self.0 == 0.0 {
            1.0
        } else {
            10.0_f32.powf(self.0 / 20.0)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PlaybackPosition {
    absolute_frame: SourceFrame,
    display_microseconds: Microseconds,
}

impl PlaybackPosition {
    pub const fn new(absolute_frame: SourceFrame, display_microseconds: Microseconds) -> Self {
        Self {
            absolute_frame,
            display_microseconds,
        }
    }

    pub const fn absolute_frame(self) -> SourceFrame {
        self.absolute_frame
    }

    pub const fn display_microseconds(self) -> Microseconds {
        self.display_microseconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_boundaries_are_checked() {
        assert!(SampleRate::new(0).is_err());
        assert!(SampleRate::new(48_000).is_ok());
        assert!(ChannelCount::new(0).is_err());
        assert!(FrameCount::new(i64::MAX as u64).is_ok());
        assert!(FrameCount::new(i64::MAX as u64 + 1).is_err());
        assert!(GainDb::new(f32::NAN).is_err());
        assert!(GainDb::new(GainDb::MIN - 0.1).is_err());
        assert!(GainDb::new(GainDb::MAX + 0.1).is_err());
        assert_eq!(GainDb::SILENCE.linear(), 0.0);
        assert_eq!(GainDb::UNITY.linear(), 1.0);
    }

    #[test]
    fn generation_exhaustion_is_checked() {
        assert_eq!(GenerationId::INITIAL.next().unwrap().get(), 1);
        assert!(GenerationId(u64::MAX).next().is_err());
    }

    #[test]
    fn bounded_text_truncates_on_character_boundaries() {
        let value = BoundedText::<3>::truncated("a\u{e9}bc");
        assert_eq!(value.as_str(), "a\u{e9}");
        assert!(BoundedText::<4>::new("abcde").is_err());
    }
}
