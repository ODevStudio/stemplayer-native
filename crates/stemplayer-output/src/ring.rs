use rtrb::{Consumer, Producer, RingBuffer};
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use stemplayer_domain::{ErrorCode, PlayerError};

pub(crate) use crate::conversion::OutputSample;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StereoFrame {
    pub left: f32,
    pub right: f32,
}

#[derive(Clone, Copy, Default)]
struct TaggedFrame {
    generation: u64,
    end_frame: u64,
    frame: StereoFrame,
}

#[derive(Debug, Default)]
struct Counters {
    generation: AtomicU64,
    draining: std::sync::atomic::AtomicBool,
    produced_frames: AtomicU64,
    consumed_frames: AtomicU64,
    consumed_audio_frames: AtomicU64,
    generation_start_frame: AtomicU64,
    clipped_samples: AtomicU64,
    presentation_end_frame: AtomicU64,
    presentation_end_ns: AtomicU64,
    underflows: AtomicU64,
    overflows: AtomicU64,
    discarded_frames: AtomicU64,
    callback_count: AtomicU64,
    callback_max_ns: AtomicU64,
    callback_min_frames: AtomicUsize,
    callback_max_frames: AtomicUsize,
    maximum_occupancy: AtomicUsize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct OutputMetrics {
    pub capacity_frames: usize,
    pub generation: u64,
    pub produced_frames: u64,
    pub consumed_frames: u64,
    pub generation_produced_frames: u64,
    pub generation_consumed_frames: u64,
    pub clipped_samples: u64,
    pub underflows: u64,
    pub overflows: u64,
    pub discarded_frames: u64,
    pub callback_count: u64,
    pub callback_max_ns: u64,
    pub callback_min_frames: usize,
    pub callback_max_frames: usize,
    pub maximum_occupancy_frames: usize,
}

pub struct OutputProducer {
    inner: Producer<TaggedFrame>,
    counters: Arc<Counters>,
    capacity: usize,
}

pub struct OutputConsumer {
    inner: Consumer<TaggedFrame>,
    counters: Arc<Counters>,
    capacity: usize,
    last_submission: Option<(u64, usize)>,
}

pub fn output_ring(
    capacity_frames: usize,
) -> Result<(OutputProducer, OutputConsumer), PlayerError> {
    output_ring_at_generation(capacity_frames, 0)
}

pub(crate) fn output_ring_at_generation(
    capacity_frames: usize,
    generation: u64,
) -> Result<(OutputProducer, OutputConsumer), PlayerError> {
    if capacity_frames == 0 {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Output ring capacity must be at least one frame",
        ));
    }
    let (producer, consumer) = RingBuffer::new(capacity_frames);
    let counters = Arc::new(Counters {
        generation: AtomicU64::new(generation),
        callback_min_frames: AtomicUsize::new(usize::MAX),
        ..Counters::default()
    });
    Ok((
        OutputProducer {
            inner: producer,
            counters: Arc::clone(&counters),
            capacity: capacity_frames,
        },
        OutputConsumer {
            inner: consumer,
            counters,
            capacity: capacity_frames,
            last_submission: None,
        },
    ))
}

impl OutputProducer {
    pub fn push(&mut self, frame: StereoFrame) -> bool {
        let tagged = TaggedFrame {
            generation: self.counters.generation.load(Ordering::Acquire),
            end_frame: self
                .counters
                .produced_frames
                .load(Ordering::Relaxed)
                .saturating_add(1),
            frame,
        };
        if self.inner.push(tagged).is_err() {
            self.counters.overflows.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.counters
            .produced_frames
            .fetch_add(1, Ordering::Relaxed);
        self.counters
            .maximum_occupancy
            .fetch_max(self.occupancy(), Ordering::Relaxed);
        true
    }

    pub fn push_slice(&mut self, frames: &[StereoFrame]) -> usize {
        let count = frames.len().min(self.inner.slots());
        if count > 0 {
            let generation = self.counters.generation.load(Ordering::Acquire);
            let start_frame = self.counters.produced_frames.load(Ordering::Relaxed);
            let mut chunk = self
                .inner
                .write_chunk(count)
                .expect("available slots changed");
            let (first, second) = chunk.as_mut_slices();
            for (index, (target, frame)) in first.iter_mut().chain(second).zip(frames).enumerate() {
                *target = TaggedFrame {
                    generation,
                    end_frame: start_frame.saturating_add(index as u64).saturating_add(1),
                    frame: *frame,
                };
            }
            chunk.commit_all();
            self.counters
                .produced_frames
                .fetch_add(count as u64, Ordering::Relaxed);
            self.counters
                .maximum_occupancy
                .fetch_max(self.occupancy(), Ordering::Relaxed);
        }
        if count < frames.len() {
            self.counters.overflows.fetch_add(1, Ordering::Relaxed);
        }
        count
    }

    pub fn reset(&self) -> u64 {
        self.counters.generation_start_frame.store(
            self.counters.produced_frames.load(Ordering::Relaxed),
            Ordering::Release,
        );
        self.counters.draining.store(false, Ordering::Release);
        self.counters.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn begin_drain(&self) {
        self.counters.draining.store(true, Ordering::Release);
    }

    pub fn generation(&self) -> u64 {
        self.counters.generation.load(Ordering::Acquire)
    }

    pub fn available(&self) -> usize {
        self.inner.slots()
    }

    pub fn occupancy(&self) -> usize {
        self.capacity - self.available()
    }

    pub fn metrics(&self) -> OutputMetrics {
        snapshot(&self.counters, self.capacity)
    }

    pub(crate) fn presentation_submission(&self) -> Option<(u64, u64)> {
        let end_frame = self.counters.presentation_end_frame.load(Ordering::Acquire);
        let end_ns = self.counters.presentation_end_ns.load(Ordering::Acquire);
        let submitted = self.counters.consumed_audio_frames.load(Ordering::Acquire);
        let start = self.counters.generation_start_frame.load(Ordering::Acquire);
        (end_ns > 0
            && end_frame > start
            && end_frame == submitted
            && end_frame == self.counters.presentation_end_frame.load(Ordering::Acquire))
        .then_some((end_frame.saturating_sub(start), end_ns))
    }
}

impl OutputConsumer {
    pub fn pop(&mut self) -> Option<StereoFrame> {
        let mut discarded = 0;
        let frame = self.pop_current_generation(&mut discarded);
        if let Some(tagged) = frame {
            self.counters
                .consumed_audio_frames
                .fetch_max(tagged.end_frame, Ordering::Release);
        }
        if discarded > 0 {
            self.counters
                .discarded_frames
                .fetch_add(discarded, Ordering::Relaxed);
        }
        if frame.is_none() && !self.counters.draining.load(Ordering::Acquire) {
            self.counters.underflows.fetch_add(1, Ordering::Relaxed);
        }
        frame.map(|tagged| tagged.frame)
    }

    pub fn write<T: OutputSample>(&mut self, output: &mut [T], channels: usize) {
        self.last_submission = None;
        if channels < 2 {
            output.fill(T::silence());
            return;
        }
        let mut discarded = 0;
        let mut underflows = 0;
        let mut consumed = 0;
        let mut clipped = 0;
        let mut output_frames = output.chunks_exact_mut(channels);
        for (index, output_frame) in (&mut output_frames).enumerate() {
            let frame = match self.pop_current_generation(&mut discarded) {
                Some(tagged) => {
                    self.last_submission = Some((tagged.end_frame, index + 1));
                    tagged.frame
                }
                None => {
                    underflows += 1;
                    StereoFrame::default()
                }
            };
            if T::CLIPS {
                clipped += u64::from(frame.left.is_finite() && frame.left.abs() > 1.0);
                clipped += u64::from(frame.right.is_finite() && frame.right.abs() > 1.0);
            }
            output_frame.fill(T::silence());
            output_frame[0] = T::from_float(frame.left);
            output_frame[1] = T::from_float(frame.right);
            consumed += 1;
        }
        output_frames.into_remainder().fill(T::silence());
        self.counters
            .consumed_frames
            .fetch_add(consumed, Ordering::Relaxed);
        if let Some((end_frame, _)) = self.last_submission {
            self.counters
                .consumed_audio_frames
                .fetch_max(end_frame, Ordering::Release);
        }
        if clipped > 0 {
            self.counters
                .clipped_samples
                .fetch_add(clipped, Ordering::Relaxed);
        }
        if discarded > 0 {
            self.counters
                .discarded_frames
                .fetch_add(discarded, Ordering::Relaxed);
        }
        if underflows > 0 && !self.counters.draining.load(Ordering::Acquire) {
            self.counters
                .underflows
                .fetch_add(underflows, Ordering::Relaxed);
        }
    }

    pub fn record_callback(&self, frames: usize, elapsed: Duration) {
        self.counters.callback_count.fetch_add(1, Ordering::Relaxed);
        self.counters.callback_max_ns.fetch_max(
            elapsed.as_nanos().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
        self.counters
            .callback_min_frames
            .fetch_min(frames, Ordering::Relaxed);
        self.counters
            .callback_max_frames
            .fetch_max(frames, Ordering::Relaxed);
    }

    pub(crate) fn record_presentation(&self, playback_ns: u128, sample_rate: u32) {
        if sample_rate == 0 {
            return;
        }
        if let Some((end_frame, frames)) = self.last_submission {
            let duration_ns = (frames as u128 * 1_000_000_000).div_ceil(u128::from(sample_rate));
            let end_ns = playback_ns
                .checked_add(duration_ns)
                .and_then(|value| u64::try_from(value).ok());
            self.counters
                .presentation_end_ns
                .store(end_ns.unwrap_or_default(), Ordering::Release);
            self.counters
                .presentation_end_frame
                .store(end_frame, Ordering::Release);
        }
    }

    pub fn metrics(&self) -> OutputMetrics {
        snapshot(&self.counters, self.capacity)
    }

    fn pop_current_generation(&mut self, discarded: &mut u64) -> Option<TaggedFrame> {
        loop {
            match self.inner.pop() {
                Ok(tagged)
                    if tagged.generation == self.counters.generation.load(Ordering::Acquire) =>
                {
                    return Some(tagged);
                }
                Ok(_) => *discarded = discarded.saturating_add(1),
                Err(_) => return None,
            }
        }
    }
}

fn snapshot(counters: &Counters, capacity_frames: usize) -> OutputMetrics {
    let callback_min_frames = counters.callback_min_frames.load(Ordering::Relaxed);
    let start_frame = counters.generation_start_frame.load(Ordering::Acquire);
    let produced_frames = counters.produced_frames.load(Ordering::Relaxed);
    let generation_produced_frames = produced_frames.saturating_sub(start_frame);
    OutputMetrics {
        capacity_frames,
        generation: counters.generation.load(Ordering::Acquire),
        produced_frames,
        consumed_frames: counters.consumed_frames.load(Ordering::Relaxed),
        generation_produced_frames,
        generation_consumed_frames: counters
            .consumed_audio_frames
            .load(Ordering::Acquire)
            .saturating_sub(start_frame)
            .min(generation_produced_frames),
        clipped_samples: counters.clipped_samples.load(Ordering::Relaxed),
        underflows: counters.underflows.load(Ordering::Relaxed),
        overflows: counters.overflows.load(Ordering::Relaxed),
        discarded_frames: counters.discarded_frames.load(Ordering::Relaxed),
        callback_count: counters.callback_count.load(Ordering::Relaxed),
        callback_max_ns: counters.callback_max_ns.load(Ordering::Relaxed),
        callback_min_frames: if callback_min_frames != usize::MAX {
            callback_min_frames
        } else {
            Default::default()
        },
        callback_max_frames: counters.callback_max_frames.load(Ordering::Relaxed),
        maximum_occupancy_frames: counters.maximum_occupancy.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    std::thread_local! {
        static RESET_PRODUCER: std::cell::RefCell<Option<OutputProducer>> = const { std::cell::RefCell::new(None) };
        static OUTPUT_CONVERSIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    struct ResetSample(f32);

    impl OutputSample for ResetSample {
        fn silence() -> Self {
            Self(0.0)
        }

        fn from_float(value: f32) -> Self {
            OUTPUT_CONVERSIONS.with(|conversions| {
                let next = conversions.get() + 1;
                conversions.set(next);
                if next == 2 {
                    RESET_PRODUCER.with(|producer| {
                        let mut producer = producer.borrow_mut();
                        let producer = producer.as_mut().unwrap();
                        producer.reset();
                        assert!(producer.push(StereoFrame {
                            left: 3.0,
                            right: -3.0,
                        }));
                    });
                }
            });
            Self(value)
        }
    }

    #[test]
    fn ring_never_overwrites_and_reports_underflow() {
        let (mut producer, mut consumer) = output_ring(2).unwrap();
        assert!(producer.push(StereoFrame {
            left: 1.0,
            right: 2.0
        }));
        assert!(producer.push(StereoFrame {
            left: 3.0,
            right: 4.0
        }));
        assert!(!producer.push(StereoFrame {
            left: 5.0,
            right: 6.0
        }));
        assert_eq!(consumer.pop().unwrap().left, 1.0);
        assert_eq!(consumer.pop().unwrap().left, 3.0);
        assert_eq!(consumer.pop(), None);
        let metrics = consumer.metrics();
        assert_eq!(metrics.overflows, 1);
        assert_eq!(metrics.underflows, 1);
        assert_eq!(metrics.maximum_occupancy_frames, 2);
    }

    #[test]
    fn batch_push_preserves_order_and_reports_partial_overflow_once() {
        let (mut producer, mut consumer) = output_ring(3).unwrap();
        let frames = [
            StereoFrame {
                left: 1.0,
                right: -1.0,
            },
            StereoFrame {
                left: 2.0,
                right: -2.0,
            },
            StereoFrame {
                left: 3.0,
                right: -3.0,
            },
            StereoFrame {
                left: 4.0,
                right: -4.0,
            },
        ];
        assert_eq!(producer.push_slice(&frames), 3);
        let mut output = [0.0_f32; 6];
        consumer.write(&mut output, 2);
        assert_eq!(output, [1.0, -1.0, 2.0, -2.0, 3.0, -3.0]);
        let metrics = consumer.metrics();
        assert_eq!(metrics.produced_frames, 3);
        assert_eq!(metrics.consumed_frames, 3);
        assert_eq!(metrics.overflows, 1);
        assert_eq!(metrics.underflows, 0);
    }

    #[test]
    fn expected_drain_writes_silence_without_an_underrun() {
        let (producer, mut consumer) = output_ring(2).unwrap();
        producer.begin_drain();
        assert_eq!(consumer.pop(), None);
        assert_eq!(consumer.metrics().underflows, 0);
        producer.reset();
        assert_eq!(consumer.pop(), None);
        assert_eq!(consumer.metrics().underflows, 1);
    }

    #[test]
    fn reset_discards_stale_frames() {
        let (mut producer, mut consumer) = output_ring(2).unwrap();
        assert!(producer.push(StereoFrame {
            left: 1.0,
            right: 1.0
        }));
        assert_eq!(producer.reset(), 1);
        assert_eq!(consumer.pop(), None);
        assert_eq!(consumer.metrics().discarded_frames, 1);
        assert_eq!(consumer.metrics().generation_consumed_frames, 0);
    }

    #[test]
    fn callback_reset_discards_remaining_stale_frames() {
        let (mut producer, mut consumer) = output_ring(3).unwrap();
        assert!(producer.push(StereoFrame {
            left: 1.0,
            right: -1.0,
        }));
        assert!(producer.push(StereoFrame {
            left: 2.0,
            right: -2.0,
        }));
        RESET_PRODUCER.with(|slot| *slot.borrow_mut() = Some(producer));
        OUTPUT_CONVERSIONS.with(|conversions| conversions.set(0));

        let mut output = [ResetSample::default(); 4];
        consumer.write(&mut output, 2);
        RESET_PRODUCER.with(|slot| slot.borrow_mut().take());

        assert_eq!(
            output,
            [
                ResetSample(1.0),
                ResetSample(-1.0),
                ResetSample(3.0),
                ResetSample(-3.0),
            ]
        );
        assert_eq!(consumer.metrics().discarded_frames, 1);
        assert_eq!(consumer.metrics().generation_consumed_frames, 1);
    }

    #[test]
    fn callback_conversion_saturates_integer_output() {
        let (mut producer, mut consumer) = output_ring(2).unwrap();
        assert!(producer.push(StereoFrame {
            left: -2.0,
            right: 2.0
        }));
        let mut output = [0_i16; 2];
        consumer.write(&mut output, 2);
        assert_eq!(output, [i16::MIN, i16::MAX]);
        assert_eq!(consumer.metrics().clipped_samples, 2);
        producer.push(StereoFrame {
            left: -1.0,
            right: 1.0,
        });
        consumer.write(&mut output, 2);
        assert_eq!(consumer.metrics().clipped_samples, 2);
    }

    #[test]
    fn every_integer_format_counts_only_finite_out_of_range_conversions() {
        fn clipping_count<T: OutputSample>() -> u64 {
            let (mut producer, mut consumer) = output_ring(4).unwrap();
            producer.push_slice(&[
                StereoFrame {
                    left: -2.0,
                    right: 2.0,
                },
                StereoFrame {
                    left: -1.0,
                    right: 1.0,
                },
                StereoFrame {
                    left: f32::NAN,
                    right: f32::INFINITY,
                },
                StereoFrame {
                    left: 0.5,
                    right: -0.5,
                },
            ]);
            consumer.write(&mut [T::silence(); 8], 2);
            consumer.metrics().clipped_samples
        }
        assert_eq!(clipping_count::<i16>(), 2);
        assert_eq!(clipping_count::<u16>(), 2);
        assert_eq!(clipping_count::<i32>(), 2);
        assert_eq!(clipping_count::<u32>(), 2);
        assert_eq!(clipping_count::<f32>(), 0);
        assert_eq!(clipping_count::<f64>(), 0);
    }

    #[test]
    fn generation_clock_excludes_discarded_frames_and_underrun_silence() {
        let (mut producer, mut consumer) = output_ring(4).unwrap();
        producer.push_slice(&[StereoFrame::default(); 3]);
        consumer.write(&mut [0.0_f32; 2], 2);
        assert_eq!(producer.metrics().generation_consumed_frames, 1);
        producer.reset();
        producer.push(StereoFrame::default());
        consumer.write(&mut [0.0_f32; 8], 2);
        let metrics = producer.metrics();
        assert_eq!(metrics.generation_produced_frames, 1);
        assert_eq!(metrics.generation_consumed_frames, 1);
        assert_eq!(metrics.discarded_frames, 2);
        assert_eq!(metrics.underflows, 3);
    }

    #[test]
    fn replay_generation_rearms_underrun_reporting() {
        let (mut producer, mut consumer) = output_ring(2).unwrap();
        producer.push(StereoFrame::default());
        producer.begin_drain();
        consumer.write(&mut [0.0_f32; 4], 2);
        assert_eq!(producer.metrics().underflows, 0);
        producer.reset();
        producer.push(StereoFrame::default());
        consumer.write(&mut [0.0_f32; 4], 2);
        assert_eq!(producer.metrics().underflows, 1);
        assert_eq!(producer.metrics().generation_consumed_frames, 1);
    }

    #[test]
    fn incomplete_callback_frame_is_silenced_without_consuming_audio() {
        let (mut producer, mut consumer) = output_ring(1).unwrap();
        let frame = StereoFrame {
            left: 1.0,
            right: -1.0,
        };
        assert!(producer.push(frame));
        let mut output = [1.0_f32];

        consumer.write(&mut output, 2);

        assert_eq!(output, [0.0]);
        assert_eq!(consumer.pop(), Some(frame));
    }

    #[test]
    fn non_finite_integer_output_is_silence() {
        assert_eq!(i16::from_float(f32::NAN), i16::silence());
        assert_eq!(u16::from_float(f32::INFINITY), u16::silence());
        assert_eq!(i32::from_float(f32::NEG_INFINITY), i32::silence());
        assert_eq!(u32::from_float(f32::NAN), u32::silence());
    }

    #[test]
    fn reduced_counter_wrap_simulation_preserves_distance() {
        fn distance(write: u8, read: u8) -> u8 {
            write.wrapping_sub(read)
        }
        assert_eq!(distance(1, 254), 3);
        assert_eq!(distance(0, 255), 1);
    }
}
