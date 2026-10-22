use crate::StereoFrame;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};
use stemplayer_domain::{ErrorCode, PlayerError, ResamplerSnapshot};

const MINIMUM_SAMPLE_RATE: u32 = 8_000;
const MAXIMUM_SAMPLE_RATE: u32 = 384_000;
const MAXIMUM_BUFFER_FRAMES: usize = 65_536;

enum ResamplerKind {
    Passthrough,
    Fft(Box<Fft<f32>>),
}

pub struct PostMixResampler {
    input_rate: u32,
    output_rate: u32,
    kind: ResamplerKind,
    input: Vec<f32>,
    output: Vec<f32>,
    frames: Vec<StereoFrame>,
    output_delay: usize,
    delay_remaining: usize,
    target_output_frames: u64,
    input_frames: u64,
    output_frames: u64,
    input_ended: bool,
    reset_count: u64,
    discontinuity_count: u64,
}

impl PostMixResampler {
    pub fn new(
        input_rate: u32,
        output_rate: u32,
        maximum_input_frames: usize,
        total_input_frames: u64,
    ) -> Result<Self, PlayerError> {
        if input_rate == 0 || output_rate == 0 || maximum_input_frames == 0 {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Resampler rates and chunk size must be positive",
            ));
        }
        if maximum_input_frames > MAXIMUM_BUFFER_FRAMES {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Resampler input chunk exceeds the bounded buffer size",
            ));
        }
        if !(MINIMUM_SAMPLE_RATE..=MAXIMUM_SAMPLE_RATE).contains(&input_rate)
            || !(MINIMUM_SAMPLE_RATE..=MAXIMUM_SAMPLE_RATE).contains(&output_rate)
        {
            return Err(PlayerError::new(
                ErrorCode::FeatureUnsupported,
                "Source or device sample rate is outside the qualified resampler range",
            ));
        }
        let (kind, input_capacity, output_capacity, output_delay) = if input_rate == output_rate {
            (
                ResamplerKind::Passthrough,
                maximum_input_frames,
                maximum_input_frames,
                0,
            )
        } else {
            let resampler = Fft::<f32>::new(
                input_rate as usize,
                output_rate as usize,
                maximum_input_frames,
                2,
                FixedSync::Input,
            )
            .map_err(resampler_error)?;
            let input_capacity = resampler.input_frames_max();
            let output_capacity = resampler.output_frames_max();
            let output_delay = resampler.output_delay();
            if input_capacity > MAXIMUM_BUFFER_FRAMES || output_capacity > MAXIMUM_BUFFER_FRAMES {
                return Err(PlayerError::new(
                    ErrorCode::FeatureUnsupported,
                    "The device sample-rate ratio requires oversized resampler buffers",
                ));
            }
            (
                ResamplerKind::Fft(Box::new(resampler)),
                input_capacity,
                output_capacity,
                output_delay,
            )
        };
        Ok(Self {
            input_rate,
            output_rate,
            kind,
            input: vec![0.0; input_capacity * 2],
            output: vec![0.0; output_capacity * 2],
            frames: vec![StereoFrame::default(); output_capacity],
            output_delay,
            delay_remaining: output_delay,
            target_output_frames: scaled_frames(total_input_frames, input_rate, output_rate),
            input_frames: 0,
            output_frames: 0,
            input_ended: total_input_frames == 0,
            reset_count: 0,
            discontinuity_count: 0,
        })
    }

    pub fn input_frames_next(&self) -> usize {
        match &self.kind {
            ResamplerKind::Passthrough => self.input.len() / 2,
            ResamplerKind::Fft(resampler) => resampler.input_frames_next(),
        }
    }

    pub fn maximum_output_frames(&self) -> usize {
        self.frames.len()
    }

    pub fn process(
        &mut self,
        frames: &[StereoFrame],
        end_of_input: bool,
    ) -> Result<&[StereoFrame], PlayerError> {
        if self.input_ended && !frames.is_empty() {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Resampler input has already ended",
            ));
        }
        if self.finished() {
            return Ok(&[]);
        }
        if frames.len() > self.input_frames_next() {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Resampler input exceeds its fixed chunk size",
            ));
        }
        if frames.len() < self.input_frames_next() && !end_of_input {
            return Err(PlayerError::new(
                ErrorCode::PlaybackNotReady,
                "Resampler requires a complete input chunk before the final block",
            ));
        }
        self.input_ended |= end_of_input;
        self.input_frames = self.input_frames.saturating_add(frames.len() as u64);
        match &mut self.kind {
            ResamplerKind::Passthrough => {
                let available = frames
                    .len()
                    .min(self.target_output_frames.saturating_sub(self.output_frames) as usize);
                self.frames[..available].copy_from_slice(&frames[..available]);
                self.output_frames = self.output_frames.saturating_add(available as u64);
                Ok(&self.frames[..available])
            }
            ResamplerKind::Fft(resampler) => {
                for (target, source) in self.input.chunks_exact_mut(2).zip(frames) {
                    target[0] = source.left;
                    target[1] = source.right;
                }
                let required = resampler.input_frames_next();
                let input =
                    InterleavedSlice::new(&self.input, 2, required).map_err(resampler_error)?;
                let output_capacity = self.output.len() / 2;
                let mut output = InterleavedSlice::new_mut(&mut self.output, 2, output_capacity)
                    .map_err(resampler_error)?;
                let indexing =
                    (frames.len() < required).then(|| Indexing::new().partial_len(frames.len()));
                let (_, written) = resampler
                    .process_into_buffer(&input, &mut output, indexing.as_ref())
                    .map_err(resampler_error)?;
                let skip = self.delay_remaining.min(written);
                self.delay_remaining -= skip;
                let available = written
                    .saturating_sub(skip)
                    .min(self.target_output_frames.saturating_sub(self.output_frames) as usize);
                for (target, source) in self.frames[..available]
                    .iter_mut()
                    .zip(self.output[skip * 2..(skip + available) * 2].chunks_exact(2))
                {
                    *target = StereoFrame {
                        left: source[0],
                        right: source[1],
                    };
                }
                self.output_frames = self.output_frames.saturating_add(available as u64);
                Ok(&self.frames[..available])
            }
        }
    }

    pub fn reset(&mut self, total_input_frames: u64) {
        if let ResamplerKind::Fft(resampler) = &mut self.kind {
            resampler.reset();
        }
        self.input.fill(0.0);
        self.output.fill(0.0);
        self.frames.fill(StereoFrame::default());
        self.delay_remaining = self.output_delay;
        self.target_output_frames =
            scaled_frames(total_input_frames, self.input_rate, self.output_rate);
        self.input_frames = 0;
        self.output_frames = 0;
        self.input_ended = total_input_frames == 0;
        self.reset_count = self.reset_count.saturating_add(1);
    }

    pub fn record_discontinuity(&mut self) {
        self.discontinuity_count = self.discontinuity_count.saturating_add(1);
    }

    pub fn finished(&self) -> bool {
        self.input_ended && self.output_frames == self.target_output_frames
    }

    pub fn snapshot(&self) -> ResamplerSnapshot {
        let phase_numerator = ((u128::from(self.input_frames) * u128::from(self.output_rate))
            % u128::from(self.input_rate)) as u32;
        ResamplerSnapshot {
            input_rate: self.input_rate,
            output_rate: self.output_rate,
            delay_frames: self.output_delay.min(u32::MAX as usize) as u32,
            input_frames: self.input_frames,
            output_frames: self.output_frames,
            phase_numerator,
            phase_denominator: self.input_rate,
            buffer_occupancy_frames: self.delay_remaining.min(u32::MAX as usize) as u32,
            reset_count: self.reset_count,
            discontinuity_count: self.discontinuity_count,
        }
    }
}

fn scaled_frames(input_frames: u64, input_rate: u32, output_rate: u32) -> u64 {
    let scaled = u128::from(input_frames) * u128::from(output_rate);
    scaled
        .div_ceil(u128::from(input_rate))
        .min(u128::from(u64::MAX)) as u64
}

fn resampler_error(error: impl std::error::Error + Send + Sync + 'static) -> PlayerError {
    PlayerError::new(ErrorCode::Internal, "Stereo resampling failed").with_source(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(input_rate: u32, output_rate: u32, input: &[StereoFrame]) -> Vec<StereoFrame> {
        let mut resampler =
            PostMixResampler::new(input_rate, output_rate, 2_048, input.len() as u64).unwrap();
        let mut output = Vec::new();
        let mut offset = 0;
        while !resampler.finished() {
            let frames = resampler.input_frames_next().min(input.len() - offset);
            let end = offset + frames == input.len();
            output.extend_from_slice(
                resampler
                    .process(&input[offset..offset + frames], end)
                    .unwrap(),
            );
            offset += frames;
        }
        output
    }

    #[test]
    fn qualified_rates_reach_the_exact_scaled_end() {
        for (input_rate, output_rate) in [
            (44_100, 48_000),
            (48_000, 44_100),
            (48_000, 96_000),
            (192_000, 48_000),
        ] {
            let input = vec![StereoFrame::default(); input_rate as usize];
            let output = convert(input_rate, output_rate, &input);
            assert_eq!(output.len(), output_rate as usize);
        }
    }

    #[test]
    fn left_and_right_remain_independent() {
        let mut input = vec![StereoFrame::default(); 4_410];
        input[2_000].left = 1.0;
        let output = convert(44_100, 48_000, &input);
        assert!(output.iter().any(|frame| frame.left.abs() > 0.01));
        assert!(output.iter().all(|frame| frame.right == 0.0));
    }

    #[test]
    fn reset_is_fixed_memory_and_deterministic() {
        let mut resampler = PostMixResampler::new(44_100, 48_000, 2_048, 1_000).unwrap();
        let capacities = (
            resampler.input.capacity(),
            resampler.output.capacity(),
            resampler.frames.capacity(),
        );
        let input = vec![StereoFrame::default(); 1_000];
        let first = resampler.process(&input, true).unwrap().to_vec();
        resampler.reset(1_000);
        let second = resampler.process(&input, true).unwrap().to_vec();
        assert_eq!(first, second);
        assert_eq!(
            capacities,
            (
                resampler.input.capacity(),
                resampler.output.capacity(),
                resampler.frames.capacity()
            )
        );
        assert_eq!(resampler.snapshot().reset_count, 1);
    }

    #[test]
    fn frame_count_math_has_no_long_duration_drift() {
        let hour = 48_000_u64 * 60 * 60;
        assert_eq!(scaled_frames(hour, 48_000, 44_100), 44_100_u64 * 60 * 60);
        assert_eq!(scaled_frames(hour, 48_000, 96_000), 96_000_u64 * 60 * 60);
        assert_eq!(scaled_frames(441, 44_100, 48_000), 480);
        assert_eq!(scaled_frames(882, 44_100, 48_000), 960);
    }

    #[test]
    fn unqualified_rates_fail_before_buffer_allocation() {
        assert!(PostMixResampler::new(1, 48_000, 2_048, 1).is_err());
        assert!(PostMixResampler::new(384_001, 48_000, 2_048, 1).is_err());
        assert!(PostMixResampler::new(48_000, 384_001, 2_048, 1).is_err());
        assert!(PostMixResampler::new(48_000, 48_000, 65_537, 1).is_err());
    }

    #[test]
    fn non_final_partial_input_is_rejected_without_advancing() {
        for output_rate in [44_100, 48_000] {
            let mut resampler = PostMixResampler::new(44_100, output_rate, 2_048, 4_096).unwrap();
            let before = resampler.snapshot();
            let partial = [StereoFrame::default(); 1_000];
            let error = resampler.process(&partial, false).unwrap_err();
            assert_eq!(error.code, ErrorCode::PlaybackNotReady);
            assert_eq!(resampler.snapshot(), before);
            assert!(!resampler.finished());
            assert!(resampler.process(&[], false).is_err());
        }
    }

    #[test]
    fn rejected_partial_input_does_not_change_later_pcm() {
        let input = vec![
            StereoFrame {
                left: 0.25,
                right: -0.25,
            };
            4_096
        ];
        let expected = convert(44_100, 48_000, &input);
        let mut resampler = PostMixResampler::new(44_100, 48_000, 2_048, 4_096).unwrap();
        assert!(resampler.process(&input[..1_000], false).is_err());
        let first = resampler.process(&input[..2_048], false).unwrap().to_vec();
        let second = resampler.process(&input[2_048..], true).unwrap().to_vec();
        let mut output = [first, second].concat();
        while !resampler.finished() {
            output.extend_from_slice(resampler.process(&[], true).unwrap());
        }
        assert_eq!(output, expected);
    }

    #[test]
    fn final_input_cannot_be_extended_while_draining_or_after_completion() {
        let mut resampler = PostMixResampler::new(44_100, 48_000, 2_048, 1_000).unwrap();
        resampler
            .process(&[StereoFrame::default(); 1_000], true)
            .unwrap();
        assert!(resampler.process(&[StereoFrame::default()], true).is_err());
        while !resampler.finished() {
            resampler.process(&[], true).unwrap();
        }
        assert!(resampler.process(&[StereoFrame::default()], true).is_err());
    }
}
