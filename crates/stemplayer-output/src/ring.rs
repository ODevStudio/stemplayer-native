use rtrb::{Consumer, Producer, RingBuffer};
use serde::Serialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use stemplayer_domain::{ErrorCode, PlayerError};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StereoFrame {
    pub left: f32,
    pub right: f32,
}

#[derive(Clone, Copy, Default)]
struct TaggedFrame {
    generation: u64,
    frame: StereoFrame,
}

#[derive(Debug, Default)]
struct Counters {
    generation: AtomicU64,
    draining: std::sync::atomic::AtomicBool,
    produced_frames: AtomicU64,
    consumed_frames: AtomicU64,
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
}

pub fn output_ring(
    capacity_frames: usize,
) -> Result<(OutputProducer, OutputConsumer), PlayerError> {
    if capacity_frames == 0 {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Output ring capacity must be at least one frame",
        ));
    }
    let (producer, consumer) = RingBuffer::new(capacity_frames);
    let counters = Arc::new(Counters {
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
        },
    ))
}

impl OutputProducer {
    pub fn push(&mut self, frame: StereoFrame) -> bool {
        let tagged = TaggedFrame {
            generation: self.counters.generation.load(Ordering::Acquire),
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
            let mut chunk = self
                .inner
                .write_chunk(count)
                .expect("available slots changed");
            let (first, second) = chunk.as_mut_slices();
            for (target, frame) in first.iter_mut().chain(second).zip(frames) {
                *target = TaggedFrame {
                    generation,
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
}

impl OutputConsumer {
    pub fn pop(&mut self) -> Option<StereoFrame> {
        let generation = self.counters.generation.load(Ordering::Acquire);
        let mut discarded = 0;
        let frame = self.pop_for_generation(generation, &mut discarded);
        if discarded > 0 {
            self.counters
                .discarded_frames
                .fetch_add(discarded, Ordering::Relaxed);
        }
        if frame.is_none() && !self.counters.draining.load(Ordering::Acquire) {
            self.counters.underflows.fetch_add(1, Ordering::Relaxed);
        }
        frame
    }

    pub fn write<T: OutputSample>(&mut self, output: &mut [T], channels: usize) {
        if channels < 2 {
            output.fill(T::silence());
            return;
        }
        let generation = self.counters.generation.load(Ordering::Acquire);
        let mut discarded = 0;
        let mut underflows = 0;
        let mut consumed = 0;
        for output_frame in output.chunks_mut(channels) {
            let frame = self
                .pop_for_generation(generation, &mut discarded)
                .unwrap_or_else(|| {
                    underflows += 1;
                    StereoFrame::default()
                });
            output_frame.fill(T::silence());
            output_frame[0] = T::from_float(frame.left);
            output_frame[1] = T::from_float(frame.right);
            consumed += 1;
        }
        self.counters
            .consumed_frames
            .fetch_add(consumed, Ordering::Relaxed);
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

    pub fn metrics(&self) -> OutputMetrics {
        snapshot(&self.counters, self.capacity)
    }

    fn pop_for_generation(&mut self, generation: u64, discarded: &mut u64) -> Option<StereoFrame> {
        loop {
            match self.inner.pop() {
                Ok(tagged) if tagged.generation == generation => return Some(tagged.frame),
                Ok(_) => *discarded = discarded.saturating_add(1),
                Err(_) => return None,
            }
        }
    }
}

pub trait OutputSample: Copy {
    fn silence() -> Self;
    fn from_float(value: f32) -> Self;
}

impl OutputSample for f32 {
    fn silence() -> Self {
        0.0
    }

    fn from_float(value: f32) -> Self {
        value
    }
}

impl OutputSample for f64 {
    fn silence() -> Self {
        0.0
    }

    fn from_float(value: f32) -> Self {
        f64::from(value)
    }
}

impl OutputSample for i16 {
    fn silence() -> Self {
        0
    }

    fn from_float(value: f32) -> Self {
        if !value.is_finite() {
            Self::silence()
        } else if value <= -1.0 {
            Self::MIN
        } else if value >= 1.0 {
            Self::MAX
        } else {
            (value * Self::MAX as f32).round() as Self
        }
    }
}

impl OutputSample for u16 {
    fn silence() -> Self {
        Self::MAX / 2 + 1
    }

    fn from_float(value: f32) -> Self {
        if value.is_finite() {
            (((value.clamp(-1.0, 1.0) + 1.0) * 0.5) * Self::MAX as f32).round() as Self
        } else {
            Self::silence()
        }
    }
}

impl OutputSample for i32 {
    fn silence() -> Self {
        0
    }

    fn from_float(value: f32) -> Self {
        if !value.is_finite() {
            Self::silence()
        } else if value <= -1.0 {
            Self::MIN
        } else if value >= 1.0 {
            Self::MAX
        } else {
            (f64::from(value) * f64::from(Self::MAX)).round() as Self
        }
    }
}

impl OutputSample for u32 {
    fn silence() -> Self {
        Self::MAX / 2 + 1
    }

    fn from_float(value: f32) -> Self {
        if value.is_finite() {
            (((f64::from(value.clamp(-1.0, 1.0)) + 1.0) * 0.5) * f64::from(Self::MAX)).round()
                as Self
        } else {
            Self::silence()
        }
    }
}

fn snapshot(counters: &Counters, capacity_frames: usize) -> OutputMetrics {
    let callback_min_frames = counters.callback_min_frames.load(Ordering::Relaxed);
    OutputMetrics {
        capacity_frames,
        generation: counters.generation.load(Ordering::Acquire),
        produced_frames: counters.produced_frames.load(Ordering::Relaxed),
        consumed_frames: counters.consumed_frames.load(Ordering::Relaxed),
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
