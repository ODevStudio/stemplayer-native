use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use stemplayer_domain::{
    ErrorCode, GenerationId, PlayerError, SourceFrame, SourceKind, SourceMetadata,
};
use stemplayer_engine::{AnalysisDecoder, PlaybackEngine};
use stemplayer_waveform::{
    AnalysisDecision, AnalysisPriority, CacheStatus, PeakAnalyzer, SourceFingerprint,
    WaveformSnapshot,
};

const ANALYSIS_SLICE: Duration = Duration::from_millis(10);

pub(super) struct AnalysisWork {
    decoder: AnalysisDecoder,
    cache_status: CacheStatus,
    analyzer: Option<PeakAnalyzer>,
    priority: AnalysisPriority,
    cancelled: Arc<AtomicBool>,
    generation: u64,
    next_frame: u64,
    end_frame: u64,
    snapshot_interval: u64,
    next_snapshot_frame: u64,
}

impl AnalysisWork {
    pub(super) fn new(
        engine: &PlaybackEngine,
        cache_status: CacheStatus,
        generation: GenerationId,
        fingerprint: SourceFingerprint,
        metadata: &SourceMetadata,
        low_watermark_frames: u64,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, PlayerError> {
        let next_frame = u64::try_from(metadata.start_frame.get()).map_err(|_| {
            PlayerError::new(
                ErrorCode::PresentationRangeInvalid,
                "Waveform timeline starts before zero",
            )
        })?;
        let end_frame = u64::try_from(metadata.end_frame_exclusive.get()).map_err(|_| {
            PlayerError::new(
                ErrorCode::PresentationRangeInvalid,
                "Waveform timeline ends before zero",
            )
        })?;
        let mut analyzer = PeakAnalyzer::new(
            generation,
            fingerprint,
            metadata.start_frame,
            metadata.end_frame_exclusive,
            metadata.sample_rate,
        )?;
        if metadata.kind == SourceKind::LocalAudioFile {
            analyzer.enable_frequency_bands()?;
        }
        let snapshot_interval = (end_frame - next_frame).div_ceil(100).max(1);
        Ok(Self {
            decoder: engine.analysis_decoder(Arc::clone(&cancelled)),
            cache_status,
            analyzer: Some(analyzer),
            priority: AnalysisPriority::new(generation.get(), low_watermark_frames),
            cancelled,
            generation: generation.get(),
            next_frame,
            end_frame,
            snapshot_interval,
            next_snapshot_frame: next_frame.saturating_add(snapshot_interval),
        })
    }

    pub(super) fn diagnostics(&self, engine: &PlaybackEngine) -> (CacheStatus, bool) {
        let (active, buffered, _) = engine.playback_pressure();
        let throttled = self.priority.decision(self.generation, active, buffered)
            == AnalysisDecision::PauseForPlayback;
        (self.cache_status, throttled)
    }

    pub(super) fn step(
        &mut self,
        engine: &PlaybackEngine,
    ) -> Result<Option<(WaveformSnapshot, bool)>, PlayerError> {
        let mut snapshot_due = false;
        let started = Instant::now();
        for block in 0..self.priority.pending_block_limit() {
            self.check_cancellation()?;
            if block > 0 && started.elapsed() >= ANALYSIS_SLICE {
                break;
            }
            let (active, buffered, _) = engine.playback_pressure();
            if self.priority.decision(self.generation, active, buffered) != AnalysisDecision::Run {
                break;
            }
            let frames = (self.end_frame - self.next_frame)
                .min(u64::from(stemplayer_decode::MAX_DECODE_FRAMES))
                as u32;
            if frames == 0 {
                break;
            }
            let block = self.decoder.decode(self.next_frame, frames)?;
            self.check_cancellation()?;
            self.next_frame += u64::from(block.frames);
            let analyzer = self.analyzer.as_mut().ok_or_else(|| {
                PlayerError::new(ErrorCode::Internal, "Waveform analyzer is unavailable")
            })?;
            analyzer.ingest(
                SourceFrame::new(block.start_frame as i64),
                block.frames,
                block.channels,
                &block.samples,
            )?;
            self.check_cancellation()?;
            if self.next_frame == self.end_frame {
                return Ok(Some((
                    self.analyzer.take().unwrap().finish(self.cache_status)?,
                    true,
                )));
            }
            if self.next_frame >= self.next_snapshot_frame {
                self.next_snapshot_frame = self.next_frame.saturating_add(self.snapshot_interval);
                snapshot_due = true;
            }
        }
        self.check_cancellation()?;
        if !snapshot_due {
            return Ok(None);
        }
        let analyzer = self.analyzer.as_ref().ok_or_else(|| {
            PlayerError::new(ErrorCode::Internal, "Waveform analyzer is unavailable")
        })?;
        self.check_cancellation()?;
        Ok(Some((analyzer.snapshot(self.cache_status), false)))
    }

    fn check_cancellation(&self) -> Result<(), PlayerError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(PlayerError::new(
                ErrorCode::BundleAborted,
                "Waveform analysis was cancelled",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use stemplayer_engine::{ExternalSource, PlaybackSession, SourceBlock};

    struct CancellingSource {
        cancelled: Arc<AtomicBool>,
        calls: Arc<AtomicUsize>,
    }

    impl ExternalSource for CancellingSource {
        fn decode_block(
            &self,
            generation: GenerationId,
            start_frame: u64,
            frames: u32,
            cancelled: &AtomicBool,
        ) -> Result<SourceBlock, PlayerError> {
            assert!(std::ptr::eq(cancelled, self.cancelled.as_ref()));
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.cancelled.store(true, Ordering::Release);
            SourceBlock::new(
                generation,
                start_frame,
                frames,
                8,
                vec![0.0; frames as usize * 8],
            )
        }
    }

    #[test]
    fn cancellation_reaches_the_decoder_and_prevents_publication() {
        for initially_cancelled in [false, true] {
            let cancelled = Arc::new(AtomicBool::new(initially_cancelled));
            let calls = Arc::new(AtomicUsize::new(0));
            let session = PlaybackSession::new_external(
                "Synthetic waveform",
                48_000,
                8,
                false,
                GenerationId::INITIAL,
                Arc::new(CancellingSource {
                    cancelled: Arc::clone(&cancelled),
                    calls: Arc::clone(&calls),
                }),
            )
            .unwrap();
            let metadata = session.metadata().clone();
            let engine = PlaybackEngine::new(session).unwrap();
            let mut analysis = AnalysisWork::new(
                &engine,
                CacheStatus::Disabled,
                GenerationId::INITIAL,
                SourceFingerprint::from_bytes([3; 32]),
                &metadata,
                1,
                cancelled,
            )
            .unwrap();

            assert_eq!(
                analysis.step(&engine).unwrap_err().code,
                ErrorCode::BundleAborted
            );
            assert_eq!(
                calls.load(Ordering::Relaxed),
                usize::from(!initially_cancelled)
            );
        }
    }
}
