use std::array;
use stemplayer_domain::{ErrorCode, PlayerError};

use crate::{Peak, WaveformSnapshot};

pub const MAX_VIEWPORT_COLUMNS: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeakColumn {
    pub x: f32,
    pub minimum: f32,
    pub maximum: f32,
}

#[derive(Clone, Debug)]
pub struct ViewportGeometry {
    pub frames_per_peak: u32,
    pub rows: [Vec<PeakColumn>; 4],
}

pub fn project_viewport(
    snapshot: &WaveformSnapshot,
    start_frame: i64,
    end_frame_exclusive: i64,
    width_pixels: u32,
) -> Result<ViewportGeometry, PlayerError> {
    if width_pixels == 0
        || start_frame < snapshot.start_frame.get()
        || end_frame_exclusive > snapshot.end_frame_exclusive.get()
        || start_frame >= end_frame_exclusive
    {
        return Err(PlayerError::new(
            ErrorCode::PresentationRangeInvalid,
            "Waveform viewport is invalid",
        ));
    }
    let limit = usize::try_from(width_pixels)
        .unwrap()
        .min(MAX_VIEWPORT_COLUMNS);
    let relative_start = start_frame - snapshot.start_frame.get();
    let relative_end = end_frame_exclusive - snapshot.start_frame.get();
    let level = snapshot
        .levels
        .iter()
        .find(|level| bucket_count(relative_start, relative_end, level.frames_per_peak) <= limit)
        .or_else(|| snapshot.levels.last())
        .ok_or_else(|| {
            PlayerError::new(
                ErrorCode::PresentationRangeInvalid,
                "Waveform has no peak levels",
            )
        })?;
    let resolution = i64::from(level.frames_per_peak);
    let first = usize::try_from(relative_start.div_euclid(resolution)).unwrap();
    let last = usize::try_from((relative_end - 1).div_euclid(resolution) + 1).unwrap();
    let count = last.saturating_sub(first);
    let stride = count.div_ceil(limit).max(1);
    let rows = array::from_fn(|role| {
        (first..last)
            .step_by(stride)
            .map(|index| {
                let end = (index + stride).min(last).min(level.roles[role].len());
                let peak = combine(level.roles[role].get(index..end).unwrap_or_default());
                PeakColumn {
                    x: (index.saturating_sub(first)) as f32 / count.max(1) as f32,
                    minimum: peak.minimum,
                    maximum: peak.maximum,
                }
            })
            .collect()
    });
    Ok(ViewportGeometry {
        frames_per_peak: level.frames_per_peak,
        rows,
    })
}

fn bucket_count(start: i64, end: i64, resolution: u32) -> usize {
    let resolution = i64::from(resolution);
    usize::try_from((end - 1).div_euclid(resolution) - start.div_euclid(resolution) + 1).unwrap()
}

fn combine(peaks: &[Peak]) -> Peak {
    peaks.iter().fold(
        Peak {
            minimum: 0.0,
            maximum: 0.0,
        },
        |combined, peak| Peak {
            minimum: combined.minimum.min(peak.minimum),
            maximum: combined.maximum.max(peak.maximum),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CacheStatus, PeakAnalyzer, SourceFingerprint};
    use stemplayer_domain::{GenerationId, SampleRate, SourceFrame};

    #[test]
    fn very_long_viewport_remains_bounded() {
        let frames = 2_000_000;
        let mut analyzer = PeakAnalyzer::new(
            GenerationId::INITIAL,
            SourceFingerprint::from_bytes([1; 32]),
            SourceFrame::new(0),
            SourceFrame::new(frames),
            SampleRate::new(48_000).unwrap(),
        )
        .unwrap();
        let samples = vec![0.25; frames as usize * 8];
        analyzer
            .ingest(SourceFrame::new(0), frames as u32, 8, &samples)
            .unwrap();
        let snapshot = analyzer.finish(CacheStatus::Disabled).unwrap();
        let geometry = project_viewport(&snapshot, 0, frames, 320).unwrap();
        assert!(geometry.rows.iter().all(|row| row.len() <= 320));
        assert!(geometry.rows.iter().all(|row| !row.is_empty()));
    }
}
