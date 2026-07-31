use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use stemplayer_domain::{
    ErrorCode, GenerationId, PlayerError, SourceFrame, SourceKind, SourceMetadata,
};
use stemplayer_engine::{AnalysisDecoder, PlaybackEngine};
use stemplayer_waveform::{
    AnalysisDecision, AnalysisPriority, CacheStatus, PeakAnalyzer, SourceFingerprint,
    WaveformSnapshot,
};

pub(super) struct AnalysisWork {
    decoder: AnalysisDecoder,
    cache_status: CacheStatus,
    analyzer: Option<PeakAnalyzer>,
    priority: AnalysisPriority,
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
            decoder: engine.analysis_decoder(Arc::new(AtomicBool::new(false))),
            cache_status,
            analyzer: Some(analyzer),
            priority: AnalysisPriority::new(generation.get(), low_watermark_frames),
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
        let (active, buffered, _) = engine.playback_pressure();
        if self.priority.decision(self.generation, active, buffered) != AnalysisDecision::Run {
            return Ok(None);
        }
        let frames = (self.end_frame - self.next_frame)
            .min(u64::from(stemplayer_decode::MAX_DECODE_FRAMES)) as u32;
        if frames == 0 {
            return Ok(None);
        }
        let block = self.decoder.decode(self.next_frame, frames)?;
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
        let complete = self.next_frame == self.end_frame;
        if complete {
            return Ok(Some((
                self.analyzer.take().unwrap().finish(self.cache_status)?,
                true,
            )));
        }
        if self.next_frame < self.next_snapshot_frame {
            return Ok(None);
        }
        self.next_snapshot_frame = self
            .next_snapshot_frame
            .saturating_add(self.snapshot_interval);
        let snapshot = analyzer.snapshot(self.cache_status);
        Ok(Some((snapshot, false)))
    }
}
