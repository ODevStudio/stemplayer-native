use std::collections::VecDeque;
use stemplayer_domain::{ErrorCode, GenerationId, PlayerError};

#[derive(Clone, Debug, PartialEq)]
pub struct SourceBlock {
    pub generation: GenerationId,
    pub start_frame: u64,
    pub frames: u32,
    pub channels: u8,
    pub samples: Box<[f32]>,
}

impl SourceBlock {
    pub fn new(
        generation: GenerationId,
        start_frame: u64,
        frames: u32,
        channels: u8,
        samples: Vec<f32>,
    ) -> Result<Self, PlayerError> {
        let expected = usize::try_from(frames)
            .ok()
            .and_then(|frames| frames.checked_mul(usize::from(channels)))
            .ok_or_else(invalid_block)?;
        if frames == 0 || channels == 0 || samples.len() != expected {
            return Err(invalid_block());
        }
        Ok(Self {
            generation,
            start_frame,
            frames,
            channels,
            samples: samples.into_boxed_slice(),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RingMetrics {
    pub capacity_frames: u64,
    pub buffered_frames: u64,
    pub published_blocks: u64,
    pub consumed_frames: u64,
    pub stale_blocks: u64,
    pub overflows: u64,
}

pub struct SynchronizedRing {
    blocks: VecDeque<SourceBlock>,
    generation: GenerationId,
    read_frame: u64,
    write_frame: u64,
    read_offset: u32,
    block_capacity: usize,
    capacity_frames: u64,
    channels: u8,
    metrics: RingMetrics,
}

impl SynchronizedRing {
    pub fn new(
        capacity_frames: u64,
        block_frames: u32,
        channels: u8,
        generation: GenerationId,
        start_frame: u64,
    ) -> Result<Self, PlayerError> {
        if capacity_frames == 0 || block_frames == 0 || channels == 0 {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Source ring dimensions must be positive",
            ));
        }
        let block_capacity = usize::try_from(
            capacity_frames
                .div_ceil(u64::from(block_frames))
                .saturating_add(1),
        )
        .map_err(|_| {
            PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Source ring block capacity is not representable",
            )
        })?;
        Ok(Self {
            blocks: VecDeque::with_capacity(block_capacity),
            generation,
            read_frame: start_frame,
            write_frame: start_frame,
            read_offset: 0,
            block_capacity,
            capacity_frames,
            channels,
            metrics: RingMetrics {
                capacity_frames,
                ..RingMetrics::default()
            },
        })
    }

    pub fn reset(&mut self, generation: GenerationId, start_frame: u64) {
        self.blocks.clear();
        self.generation = generation;
        self.read_frame = start_frame;
        self.write_frame = start_frame;
        self.read_offset = 0;
        self.metrics.buffered_frames = 0;
    }

    pub fn publish(&mut self, block: SourceBlock) -> Result<bool, PlayerError> {
        if block.generation != self.generation {
            self.metrics.stale_blocks = self.metrics.stale_blocks.saturating_add(1);
            return Ok(false);
        }
        if block.channels != self.channels || block.start_frame != self.write_frame {
            return Err(invalid_block());
        }
        if self.blocks.len() >= self.block_capacity
            || self
                .metrics
                .buffered_frames
                .saturating_add(u64::from(block.frames))
                > self.capacity_frames
        {
            self.metrics.overflows = self.metrics.overflows.saturating_add(1);
            return Ok(false);
        }
        self.write_frame = self.write_frame.saturating_add(u64::from(block.frames));
        self.metrics.buffered_frames = self
            .metrics
            .buffered_frames
            .saturating_add(u64::from(block.frames));
        self.metrics.published_blocks = self.metrics.published_blocks.saturating_add(1);
        self.blocks.push_back(block);
        Ok(true)
    }

    pub fn pop_frame(&mut self, output: &mut [f32]) -> bool {
        let Some(block) = self.blocks.front() else {
            return false;
        };
        if output.len() != usize::from(self.channels) {
            return false;
        }
        let start = self.read_offset as usize * usize::from(self.channels);
        output.copy_from_slice(&block.samples[start..start + usize::from(self.channels)]);
        self.read_offset += 1;
        self.read_frame = self.read_frame.saturating_add(1);
        self.metrics.buffered_frames = self.metrics.buffered_frames.saturating_sub(1);
        self.metrics.consumed_frames = self.metrics.consumed_frames.saturating_add(1);
        if self.read_offset == block.frames {
            self.blocks.pop_front();
            self.read_offset = 0;
        }
        true
    }

    pub const fn generation(&self) -> GenerationId {
        self.generation
    }

    pub const fn read_frame(&self) -> u64 {
        self.read_frame
    }

    pub const fn write_frame(&self) -> u64 {
        self.write_frame
    }

    pub const fn buffered_frames(&self) -> u64 {
        self.metrics.buffered_frames
    }

    pub const fn available_frames(&self) -> u64 {
        self.capacity_frames
            .saturating_sub(self.metrics.buffered_frames)
    }

    pub const fn metrics(&self) -> RingMetrics {
        self.metrics
    }
}

fn invalid_block() -> PlayerError {
    PlayerError::new(
        ErrorCode::BundleInconsistent,
        "Synchronized source block is not contiguous and complete",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(generation: GenerationId, start: u64, frames: u32) -> SourceBlock {
        SourceBlock::new(
            generation,
            start,
            frames,
            2,
            vec![start as f32; frames as usize * 2],
        )
        .unwrap()
    }

    #[test]
    fn ring_wraps_without_gaps_or_overwrite() {
        let mut ring = SynchronizedRing::new(4, 2, 2, GenerationId::INITIAL, 0).unwrap();
        assert!(ring.publish(block(GenerationId::INITIAL, 0, 2)).unwrap());
        assert!(ring.publish(block(GenerationId::INITIAL, 2, 2)).unwrap());
        assert!(!ring.publish(block(GenerationId::INITIAL, 4, 1)).unwrap());
        let mut frame = [0.0; 2];
        assert!(ring.pop_frame(&mut frame));
        assert!(ring.pop_frame(&mut frame));
        assert!(ring.publish(block(GenerationId::INITIAL, 4, 2)).unwrap());
        while ring.pop_frame(&mut frame) {}
        assert_eq!(ring.read_frame(), 6);
        assert_eq!(ring.metrics().overflows, 1);
    }

    #[test]
    #[ignore = "Phase 5 30-minute no-drift qualification"]
    fn thirty_minute_cursor_has_no_drift_at_qualified_rates() {
        for rate in [44_100_u64, 48_000] {
            let total = rate * 30 * 60;
            let mut ring =
                SynchronizedRing::new(8_192, 4_096, 8, GenerationId::INITIAL, 0).unwrap();
            let mut frame = [0.0_f32; 8];
            while ring.write_frame() < total {
                let frames = (total - ring.write_frame()).min(4_096) as u32;
                let block = SourceBlock::new(
                    GenerationId::INITIAL,
                    ring.write_frame(),
                    frames,
                    8,
                    vec![0.0; frames as usize * 8],
                )
                .unwrap();
                assert!(ring.publish(block).unwrap());
                for _ in 0..frames {
                    assert!(ring.pop_frame(&mut frame));
                }
            }
            assert_eq!(ring.read_frame(), total);
            assert_eq!(ring.write_frame(), total);
            assert_eq!(ring.buffered_frames(), 0);
        }
    }

    #[test]
    fn reset_rejects_stale_generation() {
        let mut ring = SynchronizedRing::new(4, 2, 2, GenerationId::INITIAL, 0).unwrap();
        let stale = block(GenerationId::INITIAL, 0, 2);
        let next = GenerationId::INITIAL.next().unwrap();
        ring.reset(next, 10);
        assert!(!ring.publish(stale).unwrap());
        assert_eq!(ring.read_frame(), 10);
        assert_eq!(ring.metrics().stale_blocks, 1);
    }

    #[test]
    fn empty_source_blocks_are_rejected() {
        assert!(SourceBlock::new(GenerationId::INITIAL, 0, 0, 2, vec![]).is_err());
    }
}
