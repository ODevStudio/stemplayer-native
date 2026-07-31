use std::{array, sync::Arc};
use stemplayer_domain::{ErrorCode, GenerationId, PlayerError, SampleRate, SourceFrame, StemRole};

use crate::SourceFingerprint;

pub(crate) const WAVEFORM_COLUMNS: u64 = 1_024;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Peak {
    pub minimum: f32,
    pub maximum: f32,
}

#[derive(Clone, Debug)]
pub struct PeakLevel {
    pub frames_per_peak: u32,
    pub roles: [Arc<[Peak]>; 4],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CacheStatus {
    #[default]
    Disabled,
    Miss,
    Hit,
    Written,
    Error,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ConsistencySummary {
    pub peak: f32,
    pub rms: f32,
    pub dbfs: f32,
    pub frames_examined: u64,
    pub non_finite_count: u64,
}

#[derive(Clone, Debug)]
pub struct WaveformSnapshot {
    pub generation: GenerationId,
    pub fingerprint: SourceFingerprint,
    pub start_frame: SourceFrame,
    pub end_frame_exclusive: SourceFrame,
    pub analyzed_end_frame: SourceFrame,
    pub sample_rate: SampleRate,
    pub levels: Arc<[PeakLevel]>,
    pub progress: f32,
    pub cache_status: CacheStatus,
    pub consistency: Option<ConsistencySummary>,
}

struct LevelBuilder {
    resolution: u32,
    frames: u32,
    minimum: [f32; 4],
    maximum: [f32; 4],
    sum_squares: [f64; 4],
    rms: bool,
    roles: [Vec<Peak>; 4],
}

impl LevelBuilder {
    fn new(resolution: u32, total: u64) -> Result<Self, PlayerError> {
        let capacity = usize::try_from(total.div_ceil(u64::from(resolution)))
            .map_err(|_| error("Waveform bucket count exceeds memory limits"))?;
        let mut roles: [Vec<Peak>; 4] = array::from_fn(|_| Vec::new());
        for role in &mut roles {
            role.try_reserve_exact(capacity)
                .map_err(|_| error("Waveform peak allocation failed"))?;
        }
        Ok(Self {
            resolution,
            frames: 0,
            minimum: [f32::INFINITY; 4],
            maximum: [f32::NEG_INFINITY; 4],
            sum_squares: [0.0; 4],
            rms: false,
            roles,
        })
    }

    fn push_peaks(&mut self, values: [(f32, f32); 4]) {
        for (index, (minimum, maximum)) in values.into_iter().enumerate() {
            self.minimum[index] = self.minimum[index].min(minimum);
            self.maximum[index] = self.maximum[index].max(maximum);
        }
        self.advance();
    }

    fn push_rms(&mut self, values: [f32; 4]) {
        for (index, value) in values.into_iter().enumerate() {
            self.sum_squares[index] += f64::from(value) * f64::from(value);
        }
        self.advance();
    }

    fn advance(&mut self) {
        self.frames += 1;
        if self.frames == self.resolution {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.frames == 0 {
            return;
        }
        for index in 0..4 {
            self.roles[index].push(self.pending_peak(index));
        }
        self.frames = 0;
        self.minimum = [f32::INFINITY; 4];
        self.maximum = [f32::NEG_INFINITY; 4];
        self.sum_squares = [0.0; 4];
    }

    fn pending_peak(&self, index: usize) -> Peak {
        if self.rms {
            let rms = (self.sum_squares[index] / f64::from(self.frames)).sqrt() as f32;
            Peak {
                minimum: if index == 0 { -rms } else { 0.0 },
                maximum: rms,
            }
        } else {
            Peak {
                minimum: self.minimum[index],
                maximum: self.maximum[index],
            }
        }
    }

    fn snapshot(&self) -> PeakLevel {
        let roles = array::from_fn(|index| {
            let mut peaks = self.roles[index].clone();
            if self.frames > 0 {
                peaks.push(self.pending_peak(index));
            }
            Arc::from(peaks)
        });
        PeakLevel {
            frames_per_peak: self.resolution,
            roles,
        }
    }
}

pub struct PeakAnalyzer {
    generation: GenerationId,
    fingerprint: SourceFingerprint,
    start: i64,
    end: i64,
    analyzed: i64,
    sample_rate: SampleRate,
    levels: Vec<LevelBuilder>,
    sum_squares: f64,
    residual_peak: f32,
    residual_samples: u64,
    non_finite: u64,
    frequency: Option<FrequencyAnalyzer>,
}

impl PeakAnalyzer {
    pub fn new(
        generation: GenerationId,
        fingerprint: SourceFingerprint,
        start: SourceFrame,
        end: SourceFrame,
        sample_rate: SampleRate,
    ) -> Result<Self, PlayerError> {
        let total = end
            .get()
            .checked_sub(start.get())
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| error("Waveform frame range is invalid"))?;
        Ok(Self {
            generation,
            fingerprint,
            start: start.get(),
            end: end.get(),
            analyzed: start.get(),
            sample_rate,
            levels: vec![LevelBuilder::new(
                u32::try_from(total.div_ceil(WAVEFORM_COLUMNS).max(1))
                    .map_err(|_| error("Waveform bucket resolution exceeds its limit"))?,
                total,
            )?],
            sum_squares: 0.0,
            residual_peak: 0.0,
            residual_samples: 0,
            non_finite: 0,
            frequency: None,
        })
    }

    pub fn enable_frequency_bands(&mut self) -> Result<(), PlayerError> {
        self.levels[0].rms = true;
        self.frequency = Some(FrequencyAnalyzer::new(self.sample_rate.get()));
        Ok(())
    }

    pub fn ingest(
        &mut self,
        start: SourceFrame,
        frames: u32,
        channels: u8,
        samples: &[f32],
    ) -> Result<(), PlayerError> {
        let expected = frames as usize * usize::from(channels);
        let next = self
            .analyzed
            .checked_add(i64::from(frames))
            .ok_or_else(|| error("Waveform frame range overflowed"))?;
        if start.get() != self.analyzed
            || !matches!(channels, 8 | 10)
            || samples.len() != expected
            || next > self.end
        {
            return Err(error("Waveform block is invalid"));
        }
        for frame in samples.chunks_exact(usize::from(channels)) {
            if let Some(frequency) = self.frequency.as_mut() {
                let values = frequency.analyze([finite(frame[0]), finite(frame[1])]);
                for level in &mut self.levels {
                    level.push_rms(values);
                }
            } else {
                let values = StemRole::ALL.map(|role| {
                    let index = role.index() * 2;
                    let left = finite(frame[index]);
                    let right = finite(frame[index + 1]);
                    (left.min(right), left.max(right))
                });
                for level in &mut self.levels {
                    level.push_peaks(values);
                }
            }
            if channels == 10 {
                self.measure(frame);
            }
        }
        self.analyzed = next;
        Ok(())
    }

    pub fn snapshot(&self, cache_status: CacheStatus) -> WaveformSnapshot {
        let total = (self.end - self.start).max(0) as f64;
        WaveformSnapshot {
            generation: self.generation,
            fingerprint: self.fingerprint,
            start_frame: SourceFrame::new(self.start),
            end_frame_exclusive: SourceFrame::new(self.end),
            analyzed_end_frame: SourceFrame::new(self.analyzed),
            sample_rate: self.sample_rate,
            levels: Arc::from(
                self.levels
                    .iter()
                    .map(LevelBuilder::snapshot)
                    .collect::<Vec<_>>(),
            ),
            progress: if total == 0.0 {
                1.0
            } else {
                ((self.analyzed - self.start) as f64 / total) as f32
            },
            cache_status,
            consistency: self.consistency(),
        }
    }

    pub fn finish(mut self, cache_status: CacheStatus) -> Result<WaveformSnapshot, PlayerError> {
        if self.analyzed != self.end {
            return Err(error("Waveform analysis ended before the source"));
        }
        for level in &mut self.levels {
            level.flush();
        }
        Ok(self.snapshot(cache_status))
    }

    fn measure(&mut self, frame: &[f32]) {
        for channel in 0..2 {
            let residual = frame[8 + channel]
                - StemRole::ALL
                    .iter()
                    .map(|role| frame[role.index() * 2 + channel])
                    .sum::<f32>();
            if residual.is_finite() {
                self.residual_peak = self.residual_peak.max(residual.abs());
                self.sum_squares += f64::from(residual) * f64::from(residual);
                self.residual_samples = self.residual_samples.saturating_add(1);
            } else {
                self.non_finite = self.non_finite.saturating_add(1);
            }
        }
    }

    fn consistency(&self) -> Option<ConsistencySummary> {
        if self.residual_samples == 0 && self.non_finite == 0 {
            return None;
        }
        let rms = if self.residual_samples == 0 {
            0.0
        } else {
            (self.sum_squares / self.residual_samples as f64).sqrt() as f32
        };
        Some(ConsistencySummary {
            peak: self.residual_peak,
            rms,
            dbfs: if rms == 0.0 {
                f32::NEG_INFINITY
            } else {
                20.0 * rms.log10()
            },
            frames_examined: self.residual_samples / 2,
            non_finite_count: self.non_finite,
        })
    }
}

struct FrequencyAnalyzer {
    low_alpha: f32,
    mid_alpha: f32,
    low: [f32; 2],
    mid: [f32; 2],
}

impl FrequencyAnalyzer {
    fn new(sample_rate: u32) -> Self {
        let alpha =
            |cutoff: f32| 1.0 - (-2.0 * std::f32::consts::PI * cutoff / sample_rate as f32).exp();
        Self {
            low_alpha: alpha(250.0),
            mid_alpha: alpha(4_000.0),
            low: [0.0; 2],
            mid: [0.0; 2],
        }
    }

    fn analyze(&mut self, input: [f32; 2]) -> [f32; 4] {
        let mut low = [0.0; 2];
        let mut mid = [0.0; 2];
        let mut high = [0.0; 2];
        for channel in 0..2 {
            self.low[channel] += self.low_alpha * (input[channel] - self.low[channel]);
            self.mid[channel] += self.mid_alpha * (input[channel] - self.mid[channel]);
            low[channel] = self.low[channel];
            mid[channel] = self.mid[channel] - self.low[channel];
            high[channel] = input[channel] - self.mid[channel];
        }
        [energy(input), energy(low), energy(mid), energy(high)]
    }
}

fn energy(stereo: [f32; 2]) -> f32 {
    ((stereo[0] * stereo[0] + stereo[1] * stereo[1]) * 0.5).sqrt()
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn error(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::AudioDataInvalid, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_level_preserves_the_final_bucket() {
        let mut analyzer = PeakAnalyzer::new(
            GenerationId::INITIAL,
            SourceFingerprint::from_bytes([7; 32]),
            SourceFrame::new(0),
            SourceFrame::new(257),
            SampleRate::new(48_000).unwrap(),
        )
        .unwrap();
        let mut samples = vec![0.0; 257 * 8];
        samples[256 * 8] = -0.75;
        samples[256 * 8 + 1] = 0.5;
        analyzer
            .ingest(SourceFrame::new(0), 257, 8, &samples)
            .unwrap();
        let snapshot = analyzer.finish(CacheStatus::Miss).unwrap();
        assert_eq!(snapshot.levels.len(), 1);
        assert_eq!(snapshot.levels[0].roles[0].len(), 257);
        assert_eq!(
            snapshot.levels[0].roles[0][256],
            Peak {
                minimum: -0.75,
                maximum: 0.5
            }
        );
    }

    #[test]
    fn exact_master_sum_has_silent_residual() {
        let mut analyzer = PeakAnalyzer::new(
            GenerationId::INITIAL,
            SourceFingerprint::from_bytes([3; 32]),
            SourceFrame::new(0),
            SourceFrame::new(1),
            SampleRate::new(44_100).unwrap(),
        )
        .unwrap();
        let samples = [0.1, 0.2, 0.2, 0.1, 0.3, 0.2, 0.4, 0.5, 1.0, 1.0];
        analyzer
            .ingest(SourceFrame::new(0), 1, 10, &samples)
            .unwrap();
        let summary = analyzer
            .finish(CacheStatus::Disabled)
            .unwrap()
            .consistency
            .unwrap();
        assert!(summary.peak < 0.000_001);
        assert_eq!(summary.frames_examined, 1);
    }

    #[test]
    fn known_master_residual_reports_peak_and_rms_without_correction() {
        let mut analyzer = PeakAnalyzer::new(
            GenerationId::INITIAL,
            SourceFingerprint::from_bytes([8; 32]),
            SourceFrame::new(0),
            SourceFrame::new(1),
            SampleRate::new(48_000).unwrap(),
        )
        .unwrap();
        let samples = [0.1, 0.2, 0.2, 0.1, 0.3, 0.2, 0.4, 0.5, 1.25, 0.5];
        analyzer
            .ingest(SourceFrame::new(0), 1, 10, &samples)
            .unwrap();
        let summary = analyzer
            .finish(CacheStatus::Disabled)
            .unwrap()
            .consistency
            .unwrap();
        assert!((summary.peak - 0.5).abs() < 0.000_001);
        assert!((summary.rms - 0.395_284_7).abs() < 0.000_001);
    }

    #[test]
    fn ordinary_audio_uses_rms_height_and_frequency_bands() {
        let frames = 4_096;
        let analyze = |frequency: f32| {
            let mut analyzer = PeakAnalyzer::new(
                GenerationId::INITIAL,
                SourceFingerprint::from_bytes([4; 32]),
                SourceFrame::new(0),
                SourceFrame::new(frames),
                SampleRate::new(48_000).unwrap(),
            )
            .unwrap();
            analyzer.enable_frequency_bands().unwrap();
            let samples = (0..frames)
                .flat_map(|frame| {
                    let value = (2.0 * std::f32::consts::PI * frequency * frame as f32 / 48_000.0)
                        .sin()
                        * 0.5;
                    [value, value, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
                })
                .collect::<Vec<_>>();
            analyzer
                .ingest(SourceFrame::new(0), frames as u32, 8, &samples)
                .unwrap();
            analyzer.finish(CacheStatus::Disabled).unwrap()
        };
        let bass = analyze(80.0);
        let treble = analyze(8_000.0);
        assert_eq!(bass.levels.len(), 1);
        assert_eq!(bass.levels[0].roles[0].len(), 1_024);
        assert!(bass.levels[0].roles[1][512].maximum > bass.levels[0].roles[3][512].maximum);
        assert!(treble.levels[0].roles[3][512].maximum > treble.levels[0].roles[1][512].maximum);
        assert_eq!(
            bass.levels[0].roles[0][0].minimum,
            -bass.levels[0].roles[0][0].maximum
        );
    }
}
