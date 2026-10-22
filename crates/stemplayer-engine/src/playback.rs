use crate::scheduler::DecodeProducer;
use crate::{AnalysisDecoder, PlaybackSession, RingMetrics, SynchronizedRing};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use stemplayer_domain::{
    BoundedText, ErrorCode, GainDb, GenerationId, Microseconds, OutputPreference, PlaybackPosition,
    PlayerError, PlayerSnapshot, SampleRate, SnapshotDetails, SourceDiagnostics, SourceFrame,
    StemRole, TransportMachine, TransportState, WaveformState,
};
use stemplayer_mixer::MixerRenderer;
use stemplayer_output::StereoFrame;
use stemplayer_timeline::frames_to_microseconds;

pub struct PlaybackEngine {
    session: PlaybackSession,
    ring: SynchronizedRing,
    producer: DecodeProducer,
    mixer: MixerRenderer,
    transport: TransportMachine,
    position: u64,
    underruns: u64,
    source_exhausted: bool,
    resume_after_seek: bool,
    play_after_priming: bool,
    last_seek_packets: u32,
    last_seek_discarded_frames: u64,
    last_seek_latency_us: u64,
    pending_seek: Option<(Instant, crate::DecodeMetrics)>,
}

impl PlaybackEngine {
    pub fn new(session: PlaybackSession) -> Result<Self, PlayerError> {
        Self::with_cancellation(session, Arc::new(AtomicBool::new(false)))
    }

    pub fn with_cancellation(
        session: PlaybackSession,
        cancellation: Arc<AtomicBool>,
    ) -> Result<Self, PlayerError> {
        if session.metadata().end_frame_exclusive <= session.metadata().start_frame {
            return Err(PlayerError::new(
                ErrorCode::TimelineInvalid,
                "Playback source timeline must contain at least one frame",
            ));
        }
        let config = session.buffer_config();
        let start_frame = session.metadata().start_frame.get().max(0) as u64;
        let ring = SynchronizedRing::new(
            config.maximum_frames,
            config.block_frames,
            session.channel_count_for_mixing(),
            session.generation(),
            start_frame,
        )?;
        let mut transport = TransportMachine::new();
        transport.transition(TransportState::Inspecting)?;
        transport.transition(TransportState::Ready)?;
        let mixer = MixerRenderer::new(
            session.metadata().sample_rate.get(),
            session.metadata().has_master,
        );
        Ok(Self {
            session,
            ring,
            producer: DecodeProducer::new(cancellation)?,
            mixer,
            transport,
            position: start_frame,
            underruns: 0,
            source_exhausted: false,
            resume_after_seek: false,
            play_after_priming: false,
            last_seek_packets: 0,
            last_seek_discarded_frames: 0,
            last_seek_latency_us: 0,
            pending_seek: None,
        })
    }

    pub fn play(&mut self) -> Result<(), PlayerError> {
        if self.transport.state() == TransportState::Ended {
            self.reset_generation(self.session.metadata().start_frame.get().max(0) as u64)?;
        }
        self.play_after_priming = true;
        if self.transport.state() != TransportState::Priming {
            self.transport.transition(TransportState::Priming)?;
        }
        self.refill()
    }

    pub fn pause(&mut self) -> Result<(), PlayerError> {
        self.play_after_priming = false;
        self.transport.transition(TransportState::Paused)
    }

    pub fn stop(&mut self) -> Result<(), PlayerError> {
        self.producer.cancel();
        self.pending_seek = None;
        self.play_after_priming = false;
        self.reset_generation(self.session.metadata().start_frame.get().max(0) as u64)?;
        if self.transport.state() != TransportState::Ready {
            self.transport.transition(TransportState::Ready)?;
        }
        Ok(())
    }

    pub fn seek(&mut self, frame: SourceFrame) -> Result<SourceFrame, PlayerError> {
        let started = Instant::now();
        let before = self.producer.metrics();
        let resume = self.playback_requested();
        self.resume_after_seek = resume;
        if self.transport.state() != TransportState::Seeking {
            self.transport.transition(TransportState::Seeking)?;
        }
        let end = self.session.metadata().end_frame_exclusive.get() as u64;
        let target = frame.get().max(0) as u64;
        let target = target.min(end);
        self.producer.cancel();
        self.reset_generation(target)?;
        if target == end {
            self.transport.transition(TransportState::Ended)?;
            self.pending_seek = None;
            self.play_after_priming = false;
            self.last_seek_packets = 0;
            self.last_seek_discarded_frames = 0;
            self.last_seek_latency_us =
                started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        } else {
            self.play_after_priming = resume;
            self.transport.transition(TransportState::Priming)?;
            self.pending_seek = Some((started, before));
            self.refill()?;
        }
        self.resume_after_seek = false;
        Ok(SourceFrame::new(target as i64))
    }

    pub fn render(&mut self, output: &mut [StereoFrame]) -> Result<usize, PlayerError> {
        output.fill(StereoFrame::default());
        self.refill()?;
        if self.transport.state() != TransportState::Playing {
            return Ok(0);
        }
        let channels = usize::from(self.session.channel_count_for_mixing());
        let mut source = [0.0_f32; 10];
        let end = self.session.metadata().end_frame_exclusive.get() as u64;
        let required = (output.len() as u64).min(end.saturating_sub(self.position));
        if self.ring.buffered_frames() < required {
            self.underruns = self.underruns.saturating_add(1);
            return Ok(0);
        }
        let mut rendered = 0;
        self.mixer.begin_block();
        for target in output {
            if !self.ring.pop_frame(&mut source[..channels]) {
                if self.position < end {
                    self.underruns = self.underruns.saturating_add(1);
                }
                break;
            }
            let mixed = self.mixer.render(&source[..channels]);
            *target = StereoFrame {
                left: mixed[0],
                right: mixed[1],
            };
            rendered += 1;
            self.position = self.ring.read_frame();
            if self.position == end {
                self.source_exhausted = true;
                break;
            }
        }
        Ok(rendered)
    }

    pub fn refill(&mut self) -> Result<(), PlayerError> {
        if !matches!(
            self.transport.state(),
            TransportState::Priming | TransportState::Playing
        ) {
            return Ok(());
        }
        let config = self.session.buffer_config();
        let buffered = self.ring.buffered_frames();
        let target = if self.transport.state() == TransportState::Priming {
            config
                .start_frames
                .min(end_frame(&self.session).saturating_sub(self.position))
        } else if buffered < config.target_frames {
            buffered.saturating_add(u64::from(config.block_frames))
        } else {
            buffered
        };
        self.producer
            .fill_to(&self.session, &mut self.ring, target)?;
        if self.transport.state() == TransportState::Priming
            && (self.ring.buffered_frames() >= target
                || self.ring.write_frame() == end_frame(&self.session))
        {
            self.transport.transition(if self.play_after_priming {
                TransportState::Playing
            } else {
                TransportState::Paused
            })?;
            if let Some((started, before)) = self.pending_seek.take() {
                let after = self.producer.metrics();
                self.last_seek_packets = u32::try_from(
                    after
                        .packets_decoded
                        .saturating_sub(before.packets_decoded)
                        .min(u64::from(u32::MAX)),
                )
                .unwrap();
                self.last_seek_discarded_frames = after
                    .discarded_frames
                    .saturating_sub(before.discarded_frames);
                self.last_seek_latency_us =
                    started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
            }
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Result<PlayerSnapshot, PlayerError> {
        let frame = SourceFrame::new(self.position as i64);
        let position = PlaybackPosition::new(
            frame,
            frames_to_microseconds(frame, self.session.metadata().sample_rate)?,
        );
        PlayerSnapshot::new(
            self.transport.state(),
            self.session.generation(),
            Some(position),
            Some(self.session.metadata().clone()),
            vec![],
        )
        .map(|snapshot| {
            snapshot.with_details(SnapshotDetails {
                mixer: self.mixer.snapshot(),
                device_name: None,
                buffering: self.transport.state() == TransportState::Priming,
                buffered_source_frames: self.ring.buffered_frames(),
                device_queue_frames: 0,
                audio_stream_active: self.transport.state() == TransportState::Playing,
                underrun_count: self.underruns,
                overflow_count: self.ring.metrics().overflows,
                resampler: None,
                source_diagnostics: self.source_diagnostics(),
                network_diagnostics: None,
                waveform: WaveformState::Unavailable,
                error: None,
                waveform_progress_percent: 0,
                waveform_cache: Default::default(),
                waveform_throttled: false,
            })
        })
    }

    pub fn finish_if_output_drained(&mut self, queued_frames: usize) -> Result<bool, PlayerError> {
        if self.source_exhausted
            && queued_frames == 0
            && self.transport.state() == TransportState::Playing
        {
            self.transport.transition(TransportState::Ended)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub const fn source_exhausted(&self) -> bool {
        self.source_exhausted
    }

    pub const fn state(&self) -> TransportState {
        self.transport.state()
    }

    pub const fn playback_requested(&self) -> bool {
        match self.transport.state() {
            TransportState::Playing => true,
            TransportState::Priming => self.play_after_priming,
            TransportState::Seeking => self.resume_after_seek,
            _ => false,
        }
    }

    pub const fn position(&self) -> SourceFrame {
        SourceFrame::new(self.position as i64)
    }

    pub const fn generation(&self) -> GenerationId {
        self.session.generation()
    }

    pub const fn duration(&self) -> Microseconds {
        self.session.duration()
    }

    pub const fn sample_rate(&self) -> SampleRate {
        self.session.metadata().sample_rate
    }

    pub fn remaining_source_frames(&self) -> u64 {
        end_frame(&self.session).saturating_sub(self.position)
    }

    pub fn set_gain(&mut self, role: StemRole, gain_db: GainDb) {
        self.mixer.set_gain(role, gain_db);
    }

    pub fn set_mute(&mut self, role: StemRole, muted: bool) {
        self.mixer.set_mute(role, muted);
    }

    pub fn set_solo(&mut self, role: StemRole, solo: bool) {
        self.mixer.set_solo(role, solo);
    }

    pub fn reset_mix(&mut self) {
        self.mixer.reset();
    }

    pub fn set_output_preference(
        &mut self,
        preference: OutputPreference,
    ) -> Result<(), PlayerError> {
        self.mixer.set_output_preference(preference)
    }

    pub fn decode_metrics(&self) -> crate::DecodeMetrics {
        self.producer.metrics()
    }

    pub const fn ring_metrics(&self) -> RingMetrics {
        self.ring.metrics()
    }

    pub fn analysis_decoder(&self, cancellation: Arc<AtomicBool>) -> AnalysisDecoder {
        AnalysisDecoder::new(&self.session, cancellation)
    }

    pub fn playback_pressure(&self) -> (bool, u64, u64) {
        (
            matches!(
                self.transport.state(),
                TransportState::Priming | TransportState::Playing
            ),
            self.ring.buffered_frames(),
            self.session.buffer_config().low_frames,
        )
    }

    fn source_diagnostics(&self) -> Option<SourceDiagnostics> {
        let crate::session::SessionSource::Compressed { container, .. } = self.session.source()
        else {
            return None;
        };
        let tracks = container
            .tracks
            .iter()
            .map(|track| {
                format!(
                    "{:?}:{:?}/{:?} {}Hz {}ch edits={} priming={} padding={} {}..{}",
                    track.role,
                    track.info.codec,
                    track.role_source,
                    track.info.sample_rate,
                    track.info.channels,
                    track.edit_count,
                    track.timeline.priming_resolved,
                    track.timeline.padding_resolved,
                    track.timeline.start_frame.get(),
                    track.timeline.end_frame_exclusive.get()
                )
                .to_ascii_lowercase()
            })
            .collect::<Vec<_>>()
            .join("; ");
        let metrics = self.producer.metrics();
        Some(SourceDiagnostics {
            container: BoundedText::new("mp4").unwrap(),
            tracks: BoundedText::truncated(tracks),
            metadata_read_bytes: container.structure.metadata_bytes_read,
            media_read_bytes: metrics.bytes_read,
            maximum_pending_blocks: metrics.maximum_pending_blocks,
            seek_packet_count: self.last_seek_packets,
            seek_discarded_frames: self.last_seek_discarded_frames,
            seek_latency_us: self.last_seek_latency_us,
        })
    }

    fn reset_generation(&mut self, frame: u64) -> Result<(), PlayerError> {
        let generation = self.session.advance_generation()?;
        self.ring.reset(generation, frame);
        self.position = frame;
        self.source_exhausted = false;
        self.mixer.prime();
        Ok(())
    }
}

fn end_frame(session: &PlaybackSession) -> u64 {
    session.metadata().end_frame_exclusive.get() as u64
}

#[cfg(test)]
mod regression_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::Duration;
    use stemplayer_format::{SelectedFile, SourceGroup, StudioBundle};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

    struct Fixture {
        path: PathBuf,
    }

    impl Fixture {
        fn bundle(has_master: bool) -> (StudioBundle, Self) {
            let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("stemplayer-phase5-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            let fixture = Self { path };
            let mut roles = vec!["drums", "bass", "other", "vocals"];
            if has_master {
                roles.push("master");
            }
            for (index, role) in roles.iter().enumerate() {
                fs::write(
                    fixture.path.join(format!("{role}.wav")),
                    wave(44_100, &[1000_i16 + index as i16; 8]),
                )
                .unwrap();
            }
            let outputs = roles
                .iter()
                .map(|role| {
                    format!(
                        r#"{{"logical_type":"{role}","filename":"{role}.wav","sample_rate":44100,"channels":2,"frames":8}}"#
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            fs::write(
                fixture.path.join("manifest.json"),
                format!(r#"{{"schema_version":1,"outputs":[{outputs}]}}"#),
            )
            .unwrap();
            let mut selected = vec![SelectedFile::new(
                "manifest.json".to_owned(),
                fixture.path.join("manifest.json"),
            )];
            selected.extend(roles.iter().map(|role| {
                SelectedFile::new(
                    format!("{role}.wav"),
                    fixture.path.join(format!("{role}.wav")),
                )
            }));
            let group = SourceGroup::new("fixture".to_owned(), selected).unwrap();
            let bundle = StudioBundle::inspect(group, false, &AtomicBool::new(false)).unwrap();
            (bundle, fixture)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn wave(sample_rate: u32, samples: &[i16]) -> Vec<u8> {
        let data_size = (samples.len() * 4) as u32;
        let mut bytes = Vec::with_capacity(44 + data_size as usize);
        bytes.extend(b"RIFF");
        bytes.extend((36 + data_size).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend(sample_rate.to_le_bytes());
        bytes.extend((sample_rate * 4).to_le_bytes());
        bytes.extend(4_u16.to_le_bytes());
        bytes.extend(16_u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(data_size.to_le_bytes());
        for sample in samples {
            bytes.extend(sample.to_le_bytes());
            bytes.extend(sample.to_le_bytes());
        }
        bytes
    }

    fn engine(has_master: bool) -> (PlaybackEngine, Fixture) {
        let (bundle, fixture) = Fixture::bundle(has_master);
        let session = PlaybackSession::new(bundle, GenerationId::INITIAL).unwrap();
        (PlaybackEngine::new(session).unwrap(), fixture)
    }

    fn play(engine: &mut PlaybackEngine) {
        engine.play().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while engine.state() == TransportState::Priming {
            engine.refill().unwrap();
            assert!(Instant::now() < deadline, "decode producer timed out");
            std::thread::yield_now();
        }
        assert_eq!(engine.state(), TransportState::Playing);
    }

    #[test]
    fn playback_reaches_the_exact_last_frame_once() {
        let (mut engine, _fixture) = engine(true);
        play(&mut engine);
        let mut output = [StereoFrame::default(); 16];
        engine.render(&mut output).unwrap();
        assert_eq!(engine.position(), SourceFrame::new(8));
        assert!(engine.source_exhausted());
        assert_eq!(engine.state(), TransportState::Playing);
        assert!(engine.finish_if_output_drained(0).unwrap());
        assert_eq!(engine.state(), TransportState::Ended);
        assert_eq!(output[0].left, 1004.0 / 32768.0);
        assert_eq!(output[7].left, 1004.0 / 32768.0);
        assert_eq!(output[8], StereoFrame::default());
        engine.render(&mut output).unwrap();
        assert_eq!(engine.position(), SourceFrame::new(8));
    }

    #[test]
    fn stop_and_seek_storms_replace_the_generation() {
        let (mut engine, _fixture) = engine(false);
        play(&mut engine);
        let initial = engine.generation();
        for frame in 0..500 {
            engine.seek(SourceFrame::new((frame % 9) as i64)).unwrap();
        }
        assert!(engine.generation() > initial);
        engine.stop().unwrap();
        assert_eq!(engine.position(), SourceFrame::new(0));
        assert_eq!(engine.state(), TransportState::Ready);
        assert!(engine.ring_metrics().capacity_frames <= 44_100 * 12);
    }

    #[test]
    fn interrupted_seek_retargets_without_losing_playback_state() {
        let (mut engine, _fixture) = engine(false);
        play(&mut engine);
        engine.resume_after_seek = true;
        engine
            .transport
            .transition(TransportState::Seeking)
            .unwrap();
        engine.seek(SourceFrame::new(5)).unwrap();
        while engine.state() == TransportState::Priming {
            engine.refill().unwrap();
            std::thread::yield_now();
        }
        assert_eq!(engine.state(), TransportState::Playing);
    }

    #[test]
    fn pause_preserves_the_frame_and_resume_continues() {
        let (mut engine, _fixture) = engine(false);
        play(&mut engine);
        let mut output = [StereoFrame::default(); 3];
        engine.render(&mut output).unwrap();
        engine.pause().unwrap();
        let paused = engine.position();
        engine.render(&mut output).unwrap();
        assert_eq!(engine.position(), paused);
        play(&mut engine);
        engine.render(&mut output).unwrap();
        assert_eq!(engine.position(), SourceFrame::new(6));
    }

    #[test]
    fn mixer_automation_never_changes_transport_or_source_generation() {
        let (mut engine, _fixture) = engine(true);
        play(&mut engine);
        let mut output = [StereoFrame::default(); 2];
        engine.render(&mut output).unwrap();
        let generation = engine.generation();
        let position = engine.position();
        engine.set_gain(StemRole::Vocals, GainDb::new(-3.0).unwrap());
        engine.set_mute(StemRole::Bass, true);
        engine.set_solo(StemRole::Drums, true);
        engine
            .set_output_preference(OutputPreference::StemMix)
            .unwrap();
        assert_eq!(engine.generation(), generation);
        assert_eq!(engine.position(), position);
        assert_eq!(engine.state(), TransportState::Playing);
        let mixer = engine.snapshot().unwrap().details().mixer;
        assert_eq!(mixer.automation_count, 4);
        assert_eq!(mixer.crossfade_count, 1);
    }

    #[test]
    fn seek_primes_active_automation_and_last_frame_commands_stay_bounded() {
        let (mut engine, _fixture) = engine(false);
        engine.set_gain(StemRole::Drums, GainDb::SILENCE);
        play(&mut engine);
        let mut output = [StereoFrame::default(); 2];
        engine.render(&mut output).unwrap();
        assert!(
            engine
                .snapshot()
                .unwrap()
                .details()
                .mixer
                .remaining_gain_frames[0]
                > 0
        );
        engine.seek(SourceFrame::new(7)).unwrap();
        while engine.state() == TransportState::Priming {
            engine.refill().unwrap();
            std::thread::yield_now();
        }
        let mixer = engine.snapshot().unwrap().details().mixer;
        assert_eq!(mixer.current_linear_gains[0], 0.0);
        assert_eq!(mixer.remaining_gain_frames[0], 0);
        engine.set_gain(StemRole::Drums, GainDb::new(GainDb::MAX).unwrap());
        let rendered = engine.render(&mut output[..1]).unwrap();
        assert_eq!(rendered, 1);
        assert!(output[0].left.is_finite());
        assert!(engine.source_exhausted());
    }

    struct BlockingSource {
        started: Arc<AtomicBool>,
        release: Arc<AtomicBool>,
    }

    impl crate::ExternalSource for BlockingSource {
        fn decode_block(
            &self,
            _generation: GenerationId,
            _start_frame: u64,
            _frames: u32,
            cancelled: &AtomicBool,
        ) -> Result<crate::SourceBlock, PlayerError> {
            self.started.store(true, Ordering::Release);
            while !cancelled.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            while !self.release.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(PlayerError::new(
                stemplayer_domain::ErrorCode::BundleAborted,
                "cancelled",
            ))
        }
    }

    #[test]
    fn empty_playback_timelines_are_rejected() {
        let session = PlaybackSession::new_external(
            "empty",
            48_000,
            0,
            false,
            GenerationId::INITIAL,
            Arc::new(BlockingSource {
                started: Arc::new(AtomicBool::new(false)),
                release: Arc::new(AtomicBool::new(false)),
            }),
        )
        .unwrap();
        assert_eq!(
            PlaybackEngine::new(session).err().unwrap().code,
            stemplayer_domain::ErrorCode::TimelineInvalid
        );
    }

    #[test]
    fn blocking_decode_does_not_block_stop() {
        let started = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        let session = PlaybackSession::new_external(
            "blocking",
            48_000,
            48_000,
            false,
            GenerationId::INITIAL,
            Arc::new(BlockingSource {
                started: Arc::clone(&started),
                release: Arc::clone(&release),
            }),
        )
        .unwrap();
        let mut engine =
            PlaybackEngine::with_cancellation(session, Arc::clone(&cancelled)).unwrap();
        engine.play().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !started.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline, "decode producer did not start");
            std::thread::yield_now();
        }
        engine.stop().unwrap();
        assert_eq!(engine.state(), TransportState::Ready);
        engine.play().unwrap();
        assert!(!cancelled.load(Ordering::Acquire));
        release.store(true, Ordering::Release);
    }
}
