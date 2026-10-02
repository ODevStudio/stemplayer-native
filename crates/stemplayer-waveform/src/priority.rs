#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnalysisDecision {
    Run,
    PauseForPlayback,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalysisPriority {
    generation: u64,
    low_watermark_frames: u64,
    worker_limit: u8,
    pending_block_limit: u8,
}

impl AnalysisPriority {
    pub fn new(generation: u64, low_watermark_frames: u64) -> Self {
        Self {
            generation,
            low_watermark_frames,
            worker_limit: 1,
            pending_block_limit: 32,
        }
    }

    pub const fn decision(
        &self,
        generation: u64,
        playback_active: bool,
        buffered_frames: u64,
    ) -> AnalysisDecision {
        if generation != self.generation {
            AnalysisDecision::Cancel
        } else if playback_active && buffered_frames < self.low_watermark_frames {
            AnalysisDecision::PauseForPlayback
        } else {
            AnalysisDecision::Run
        }
    }

    pub const fn worker_limit(&self) -> u8 {
        self.worker_limit
    }

    pub const fn pending_block_limit(&self) -> u8 {
        self.pending_block_limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_pressure_pauses_analysis_and_generation_change_cancels_it() {
        let priority = AnalysisPriority::new(7, 4_096);
        assert_eq!(priority.decision(7, false, 0), AnalysisDecision::Run);
        assert_eq!(
            priority.decision(7, true, 4_095),
            AnalysisDecision::PauseForPlayback
        );
        assert_eq!(priority.decision(7, true, 4_096), AnalysisDecision::Run);
        assert_eq!(
            priority.decision(8, false, u64::MAX),
            AnalysisDecision::Cancel
        );
        assert_eq!(priority.worker_limit(), 1);
        assert_eq!(priority.pending_block_limit(), 32);
    }
}
