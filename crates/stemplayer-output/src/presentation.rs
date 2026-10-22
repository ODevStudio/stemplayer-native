use crate::{LatencyConfidence, OutputMetrics};
use serde::Serialize;
use std::time::Duration;
use stemplayer_domain::{ErrorCode, PlayerError};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct PresentationEstimate {
    pub generation: u64,
    pub produced_device_frames: u64,
    pub submitted_device_frames: u64,
    pub remaining: Option<Duration>,
    pub confidence: LatencyConfidence,
}

impl PresentationEstimate {
    pub(crate) fn new(
        metrics: OutputMetrics,
        submission: Option<(u64, u64)>,
        now_ns: u128,
    ) -> Self {
        Self {
            generation: metrics.generation,
            produced_device_frames: metrics.generation_produced_frames,
            submitted_device_frames: submission.map_or(0, |(frames, _)| frames),
            remaining: submission.map(|(_, end)| {
                Duration::from_nanos(
                    u64::try_from(u128::from(end).saturating_sub(now_ns)).unwrap_or(u64::MAX),
                )
            }),
            confidence: if submission.is_some() {
                LatencyConfidence::Estimated
            } else {
                LatencyConfidence::Unknown
            },
        }
    }

    pub fn drain_complete(
        &self,
        elapsed: Duration,
        timeout: Duration,
    ) -> Result<bool, PlayerError> {
        if self.submitted_device_frames == self.produced_device_frames
            && self.remaining == Some(Duration::ZERO)
        {
            return Ok(true);
        }
        if elapsed >= timeout {
            return Err(PlayerError::new(
                ErrorCode::PlaybackNotReady,
                "Audio output presentation deadline could not be reached",
            ));
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StereoFrame, output_ring};

    #[test]
    fn empty_software_queue_does_not_remove_pending_backend_time() {
        let (mut producer, mut consumer) = output_ring(2).unwrap();
        producer.push(StereoFrame::default());
        consumer.write(&mut [0.0_f32; 2], 2);
        consumer.record_presentation(1_000_000_000, 48_000);
        assert_eq!(producer.occupancy(), 0);
        let estimate = PresentationEstimate::new(
            producer.metrics(),
            producer.presentation_submission(),
            1_000_000_000,
        );
        assert_eq!(estimate.remaining, Some(Duration::from_nanos(20_834)));
        assert_eq!(estimate.confidence, LatencyConfidence::Estimated);
        assert!(
            !estimate
                .drain_complete(Duration::ZERO, Duration::from_secs(1))
                .unwrap()
        );
        let later = PresentationEstimate::new(
            producer.metrics(),
            producer.presentation_submission(),
            2_000_000_000,
        );
        assert_eq!(later.remaining, Some(Duration::ZERO));
        assert_eq!(later.confidence, LatencyConfidence::Estimated);
        assert!(
            later
                .drain_complete(Duration::ZERO, Duration::from_secs(1))
                .unwrap()
        );
    }

    #[test]
    fn silence_and_new_generations_do_not_reuse_presentation_time() {
        let (mut producer, mut consumer) = output_ring(2).unwrap();
        producer.push(StereoFrame::default());
        consumer.write(&mut [0.0_f32; 4], 2);
        consumer.record_presentation(1_000_000_000, 48_000);
        let end = producer.presentation_submission();
        consumer.write(&mut [0.0_f32; 4], 2);
        consumer.record_presentation(2_000_000_000, 48_000);
        assert_eq!(producer.presentation_submission(), end);
        producer.reset();
        assert_eq!(producer.presentation_submission(), None);
        assert_eq!(producer.metrics().generation_consumed_frames, 0);
    }

    #[test]
    fn an_old_deadline_never_covers_a_later_submission() {
        let (mut producer, mut consumer) = output_ring(2).unwrap();
        producer.push(StereoFrame::default());
        consumer.write(&mut [0.0_f32; 2], 2);
        consumer.record_presentation(1_000_000_000, 48_000);
        let old_submission = producer.presentation_submission();
        producer.push(StereoFrame::default());
        consumer.write(&mut [0.0_f32; 2], 2);
        assert_eq!(producer.presentation_submission(), None);
        let stale = PresentationEstimate::new(producer.metrics(), old_submission, 2_000_000_000);
        assert_eq!(stale.remaining, Some(Duration::ZERO));
        assert_eq!(stale.submitted_device_frames, 1);
        assert_eq!(stale.produced_device_frames, 2);
        assert!(
            !stale
                .drain_complete(Duration::ZERO, Duration::from_secs(1))
                .unwrap()
        );
        let unknown = PresentationEstimate::new(producer.metrics(), None, 2_000_000_000);
        assert!(
            unknown
                .drain_complete(Duration::from_secs(1), Duration::from_secs(1))
                .is_err()
        );
    }
}
