use crate::{DecodeScheduler, PlaybackSession, RingMetrics, SynchronizedRing};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use stemplayer_domain::{
    BoundedText, GainDb, GenerationId, Microseconds, OutputPreference, PlaybackPosition,
    PlayerError, PlayerSnapshot, SampleRate, SnapshotDetails, SourceDiagnostics, SourceFrame,
    StemRole, TransportMachine, TransportState, WaveformState,
};
use stemplayer_mixer::MixerRenderer;
use stemplayer_output::StereoFrame;
use stemplayer_timeline::frames_to_microseconds;

pub struct PlaybackEngine {
    session: PlaybackSession,
    ring: SynchronizedRing,
    scheduler: DecodeScheduler,
    mixer: MixerRenderer,
    transport: TransportMachine,
    position: u64,
    underruns: u64,
    source_exhausted: bool,
    last_seek_packets: u32,
    last_seek_discarded_frames: u64,
    last_seek_latency_us: u64,
}

impl PlaybackEngine {
    pub fn new(session: PlaybackSession) -> Result<Self, PlayerError> {
        Self::with_cancellation(session, Arc::new(AtomicBool::new(false)))
    }

    pub fn with_cancellation(
        session: PlaybackSession,
        cancellation: Arc<AtomicBool>,
    ) -> Result<Self, PlayerError> {
        let config = session.buffer_config();
        let start_frame = session.metadata().start_frame.get().max(0) as u64;
        let ring = SynchronizedRing::new(
            config.maximum_frames,
            config.block_frames,
            session.metadata().channel_count.get(),
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
            scheduler: DecodeScheduler::with_cancellation(cancellation),
            mixer,
            transport,
            position: start_frame,
            underruns: 0,
            source_exhausted: false,
            last_seek_packets: 0,
            last_seek_discarded_frames: 0,
            last_seek_latency_us: 0,
        })
    }

    pub fn play(&mut self) -> Result<(), PlayerError> {
        if self.transport.state() == TransportState::Ended {
            self.reset_generation(self.session.metadata().start_frame.get().max(0) as u64)?;
        }
        self.transport.transition(TransportState::Priming)?;
        self.scheduler.fill_to(
            &self.session,
            &mut self.ring,
            self.session.buffer_config().start_frames,
        )?;
        self.transport.transition(TransportState::Playing)
    }

    pub fn pause(&mut self) -> Result<(), PlayerError> {
        self.transport.transition(TransportState::Paused)
    }

    pub fn stop(&mut self) -> Result<(), PlayerError> {
        self.scheduler.cancel();
        self.reset_generation(self.session.metadata().start_frame.get().max(0) as u64)?;
        if self.transport.state() != TransportState::Ready {
            self.transport.transition(TransportState::Ready)?;
        }
        Ok(())
    }

    pub fn seek(&mut self, frame: SourceFrame) -> Result<SourceFrame, PlayerError> {
        let started = Instant::now();
        let before = self.scheduler.metrics();
        let resume = matches!(
            self.transport.state(),
            TransportState::Playing | TransportState::Priming
        );
        self.transport.transition(TransportState::Seeking)?;
        let end = self.session.metadata().end_frame_exclusive.get() as u64;
        let target = frame.get().max(0) as u64;
        let target = target.min(end);
        self.scheduler.cancel();
        self.reset_generation(target)?;
        if target == end {
            self.transport.transition(TransportState::Ended)?;
        } else {
            self.scheduler.fill_to(
                &self.session,
                &mut self.ring,
                self.session.buffer_config().start_frames,
            )?;
            self.transport.transition(if resume {
                TransportState::Playing
            } else {
                TransportState::Paused
            })?;
        }
        let after = self.scheduler.metrics();
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
        self.last_seek_latency_us = started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
        Ok(SourceFrame::new(target as i64))
    }

    pub fn render(&mut self, output: &mut [StereoFrame]) -> Result<usize, PlayerError> {
        output.fill(StereoFrame::default());
        if self.transport.state() != TransportState::Playing {
            return Ok(0);
        }
        if self.ring.buffered_frames() == 0 && self.position < end_frame(&self.session) {
            self.scheduler.fill_to(
                &self.session,
                &mut self.ring,
                self.session.buffer_config().start_frames,
            )?;
        }
        let channels = usize::from(self.session.metadata().channel_count.get());
        let mut source = [0.0_f32; 10];
        let end = self.session.metadata().end_frame_exclusive.get() as u64;
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
        if self.transport.state() != TransportState::Playing {
            return Ok(());
        }
        let config = self.session.buffer_config();
        let buffered = self.ring.buffered_frames();
        if buffered < config.target_frames && self.position < end_frame(&self.session) {
            self.scheduler.fill_to(
                &self.session,
                &mut self.ring,
                buffered.saturating_add(u64::from(config.block_frames)),
            )?;
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
                waveform: WaveformState::Unavailable,
                error: None,
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
        self.scheduler.metrics()
    }

    pub const fn ring_metrics(&self) -> RingMetrics {
        self.ring.metrics()
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
        let metrics = self.scheduler.metrics();
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
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

    #[test]
    fn playback_reaches_the_exact_last_frame_once() {
        let (mut engine, _fixture) = engine(true);
        engine.play().unwrap();
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
        engine.play().unwrap();
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
    fn pause_preserves_the_frame_and_resume_continues() {
        let (mut engine, _fixture) = engine(false);
        engine.play().unwrap();
        let mut output = [StereoFrame::default(); 3];
        engine.render(&mut output).unwrap();
        engine.pause().unwrap();
        let paused = engine.position();
        engine.render(&mut output).unwrap();
        assert_eq!(engine.position(), paused);
        engine.play().unwrap();
        engine.render(&mut output).unwrap();
        assert_eq!(engine.position(), SourceFrame::new(6));
    }

    #[test]
    fn mixer_automation_never_changes_transport_or_source_generation() {
        let (mut engine, _fixture) = engine(true);
        engine.play().unwrap();
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
        engine.play().unwrap();
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
        let mixer = engine.snapshot().unwrap().details().mixer;
        assert_eq!(mixer.current_linear_gains[0], 0.0);
        assert_eq!(mixer.remaining_gain_frames[0], 0);
        engine.set_gain(StemRole::Drums, GainDb::new(GainDb::MAX).unwrap());
        let rendered = engine.render(&mut output[..1]).unwrap();
        assert_eq!(rendered, 1);
        assert!(output[0].left.is_finite());
        assert!(engine.source_exhausted());
    }
}
