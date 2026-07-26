use serde::Serialize;
use stemplayer_domain::{ErrorCode, PlayerError};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LatencyConfidence {
    Unknown,
    Estimated,
    Reliable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ClockSnapshot {
    pub generation: u64,
    pub source_frames: u64,
    pub rendered_device_frames: u64,
    pub queued_device_frames: u64,
    pub consumed_device_frames: u64,
    pub audible_device_frames: u64,
    pub latency_frames: Option<u64>,
    pub latency_confidence: LatencyConfidence,
}

#[derive(Clone, Debug)]
pub struct AudioClock {
    source_rate: u32,
    device_rate: u32,
    generation: u64,
    rendered_device_frames: u64,
    queued_device_frames: u64,
    consumed_device_frames: u64,
    latency_frames: Option<u64>,
    latency_confidence: LatencyConfidence,
}

impl AudioClock {
    pub fn new(source_rate: u32, device_rate: u32) -> Result<Self, PlayerError> {
        if source_rate == 0 || device_rate == 0 {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Audio clock rates must be nonzero",
            ));
        }
        Ok(Self {
            source_rate,
            device_rate,
            generation: 0,
            rendered_device_frames: 0,
            queued_device_frames: 0,
            consumed_device_frames: 0,
            latency_frames: None,
            latency_confidence: LatencyConfidence::Unknown,
        })
    }

    pub fn record_rendered(&mut self, frames: u64, queued_frames: u64) {
        self.rendered_device_frames = self.rendered_device_frames.saturating_add(frames);
        self.queued_device_frames = queued_frames;
    }

    pub fn record_consumed(&mut self, frames: u64, queued_frames: u64) {
        self.consumed_device_frames = self.consumed_device_frames.saturating_add(frames);
        self.queued_device_frames = queued_frames;
    }

    pub fn set_latency(&mut self, frames: Option<u64>, confidence: LatencyConfidence) {
        self.latency_frames = frames;
        self.latency_confidence = confidence;
    }

    pub fn reset(&mut self, generation: u64) {
        self.generation = generation;
        self.rendered_device_frames = 0;
        self.queued_device_frames = 0;
        self.consumed_device_frames = 0;
    }

    pub fn snapshot(&self) -> ClockSnapshot {
        let audible_device_frames = self
            .consumed_device_frames
            .saturating_sub(self.latency_frames.unwrap_or_default());
        let source_frames = ((u128::from(audible_device_frames) * u128::from(self.source_rate))
            / u128::from(self.device_rate))
        .min(u128::from(u64::MAX)) as u64;
        ClockSnapshot {
            generation: self.generation,
            source_frames,
            rendered_device_frames: self.rendered_device_frames,
            queued_device_frames: self.queued_device_frames,
            consumed_device_frames: self.consumed_device_frames,
            audible_device_frames,
            latency_frames: self.latency_frames,
            latency_confidence: self.latency_confidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_uses_consumed_frames_not_wall_time() {
        let mut clock = AudioClock::new(44_100, 48_000).unwrap();
        clock.record_rendered(96_000, 48_000);
        clock.record_consumed(48_000, 48_000);
        clock.set_latency(Some(480), LatencyConfidence::Estimated);
        let snapshot = clock.snapshot();
        assert_eq!(snapshot.audible_device_frames, 47_520);
        assert_eq!(snapshot.source_frames, 43_659);
    }

    #[test]
    fn reset_atomically_rebases_all_frame_domains() {
        let mut clock = AudioClock::new(48_000, 48_000).unwrap();
        clock.record_rendered(100, 50);
        clock.record_consumed(50, 50);
        clock.reset(9);
        assert_eq!(
            clock.snapshot(),
            ClockSnapshot {
                generation: 9,
                source_frames: 0,
                rendered_device_frames: 0,
                queued_device_frames: 0,
                consumed_device_frames: 0,
                audible_device_frames: 0,
                latency_frames: None,
                latency_confidence: LatencyConfidence::Unknown,
            }
        );
    }
}
