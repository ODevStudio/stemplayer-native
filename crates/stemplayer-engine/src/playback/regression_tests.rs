use super::*;
use crate::{ExternalSource, SourceBlock};
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(2);

struct ControlledSource {
    first: AtomicBool,
    started: SyncSender<()>,
    release: Mutex<Receiver<()>>,
    completed: SyncSender<bool>,
    channels: u8,
}

impl ExternalSource for ControlledSource {
    fn decode_block(
        &self,
        generation: GenerationId,
        start_frame: u64,
        frames: u32,
        cancelled: &AtomicBool,
    ) -> Result<SourceBlock, PlayerError> {
        if self.first.swap(false, Ordering::AcqRel) {
            self.started.send(()).unwrap();
            self.release.lock().unwrap().recv_timeout(TIMEOUT).unwrap();
            self.completed
                .send(cancelled.load(Ordering::Acquire))
                .unwrap();
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(PlayerError::new(ErrorCode::BundleAborted, "Cancelled"));
        }
        Ok(source_block(generation, start_frame, frames, self.channels))
    }
}

struct Control {
    source: Arc<ControlledSource>,
    started: Receiver<()>,
    release: SyncSender<()>,
    completed: Receiver<bool>,
}

impl Control {
    fn new(has_master: bool) -> Self {
        let (started, started_receiver) = sync_channel(1);
        let (release, release_receiver) = sync_channel(1);
        let (completed, completed_receiver) = sync_channel(1);
        Self {
            source: Arc::new(ControlledSource {
                first: AtomicBool::new(true),
                started,
                release: Mutex::new(release_receiver),
                completed,
                channels: if has_master { 10 } else { 8 },
            }),
            started: started_receiver,
            release,
            completed: completed_receiver,
        }
    }

    fn session(&self, rate: u32, frames: u64) -> PlaybackSession {
        PlaybackSession::new_external(
            "Synthetic source",
            rate,
            frames,
            self.source.channels == 10,
            GenerationId::INITIAL,
            self.source.clone(),
        )
        .unwrap()
    }
}

fn source_block(generation: GenerationId, start: u64, frames: u32, channels: u8) -> SourceBlock {
    let samples = (0..frames)
        .flat_map(|frame| {
            (0..channels).map(move |channel| {
                if matches!(channel, 0 | 1 | 8 | 9) {
                    (start + u64::from(frame) + 1) as f32 / 100.0
                } else {
                    0.0
                }
            })
        })
        .collect();
    SourceBlock::new(generation, start, frames, channels, samples).unwrap()
}

fn refill_until(engine: &mut PlaybackEngine, ready: impl Fn(&PlaybackEngine) -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !ready(engine) {
        engine.refill().unwrap();
        assert!(Instant::now() < deadline, "Decode producer timed out");
        std::thread::yield_now();
    }
}

#[test]
fn repeated_seeks_preserve_ready_and_paused_playback_intent() {
    for rate in [44_100, 48_000] {
        for paused in [false, true] {
            let control = Control::new(false);
            let mut engine = PlaybackEngine::new(control.session(rate, 5_000)).unwrap();
            if paused {
                engine.play().unwrap();
                control.started.recv_timeout(TIMEOUT).unwrap();
                engine.pause().unwrap();
                engine.seek(SourceFrame::new(8)).unwrap();
            } else {
                engine.seek(SourceFrame::new(8)).unwrap();
                control.started.recv_timeout(TIMEOUT).unwrap();
            }
            engine.seek(SourceFrame::new(12)).unwrap();
            assert!(!engine.playback_requested());
            assert_eq!(engine.state(), TransportState::Priming);
            control.release.send(()).unwrap();
            refill_until(&mut engine, |engine| {
                engine.state() == TransportState::Paused
            });
            assert!(control.completed.recv_timeout(TIMEOUT).unwrap());
            assert_eq!(engine.position(), SourceFrame::new(12));
            assert!(!engine.playback_requested());
            let mut output = [StereoFrame::default(); 4];
            assert_eq!(engine.render(&mut output).unwrap(), 0);
            assert_eq!(output, [StereoFrame::default(); 4]);
        }
    }
}

#[test]
fn non_final_windows_wait_without_consuming_and_the_final_window_drains_exactly() {
    for rate in [44_100, 48_000] {
        for has_master in [false, true] {
            let control = Control::new(has_master);
            let mut engine = PlaybackEngine::new(control.session(rate, 10)).unwrap();
            engine
                .transport
                .transition(TransportState::Priming)
                .unwrap();
            engine
                .transport
                .transition(TransportState::Playing)
                .unwrap();
            assert!(
                engine
                    .ring
                    .publish(source_block(
                        engine.generation(),
                        0,
                        3,
                        control.source.channels
                    ))
                    .unwrap()
            );
            let mut output = [StereoFrame::default(); 4];
            assert_eq!(engine.render(&mut output).unwrap(), 0);
            control.started.recv_timeout(TIMEOUT).unwrap();
            assert_eq!(engine.position(), SourceFrame::new(0));
            assert_eq!(engine.ring.buffered_frames(), 3);
            assert_eq!(output, [StereoFrame::default(); 4]);
            assert_eq!(engine.underruns, 1);
            control.release.send(()).unwrap();
            refill_until(&mut engine, |engine| engine.ring.write_frame() == 10);
            for (start, count) in [(0, 4), (4, 4), (8, 2)] {
                assert_eq!(engine.render(&mut output).unwrap(), count);
                for (offset, frame) in output[..count].iter().enumerate() {
                    let expected = (start + offset + 1) as f32 / 100.0;
                    assert_eq!(
                        *frame,
                        StereoFrame {
                            left: expected,
                            right: expected
                        }
                    );
                }
                assert!(
                    output[count..]
                        .iter()
                        .all(|frame| *frame == StereoFrame::default())
                );
            }
            assert_eq!(engine.position(), SourceFrame::new(10));
            assert!(engine.source_exhausted());
            assert!(!engine.finish_if_output_drained(1).unwrap());
            assert!(engine.finish_if_output_drained(0).unwrap());
            assert_eq!(engine.state(), TransportState::Ended);
        }
    }
}

#[test]
fn retired_workers_cannot_be_revived_by_a_reused_source_flag() {
    let control = Control::new(false);
    let source_cancelled = Arc::new(AtomicBool::new(false));
    let mut engine = PlaybackEngine::with_cancellation(
        control.session(48_000, 5_000),
        Arc::clone(&source_cancelled),
    )
    .unwrap();
    engine.play().unwrap();
    control.started.recv_timeout(TIMEOUT).unwrap();
    source_cancelled.store(true, Ordering::Release);
    drop(engine);
    source_cancelled.store(false, Ordering::Release);
    let mut replacement = PlaybackEngine::with_cancellation(
        control.session(48_000, 5_000),
        Arc::clone(&source_cancelled),
    )
    .unwrap();
    replacement.play().unwrap();
    control.release.send(()).unwrap();
    assert!(control.completed.recv_timeout(TIMEOUT).unwrap());
    assert!(!source_cancelled.load(Ordering::Acquire));
}

#[test]
fn source_cancellation_is_never_cleared_by_decode_dispatch() {
    let control = Control::new(false);
    let source_cancelled = Arc::new(AtomicBool::new(true));
    let mut engine = PlaybackEngine::with_cancellation(
        control.session(48_000, 5_000),
        Arc::clone(&source_cancelled),
    )
    .unwrap();
    assert_eq!(engine.play().unwrap_err().code, ErrorCode::BundleAborted);
    assert!(source_cancelled.load(Ordering::Acquire));
    let mut scheduler = crate::DecodeScheduler::with_cancellation(Arc::clone(&source_cancelled));
    let session = control.session(48_000, 5_000);
    let mut ring = SynchronizedRing::new(8_192, 4_096, 8, session.generation(), 0).unwrap();
    assert_eq!(
        scheduler
            .fill_to(&session, &mut ring, 4_096)
            .unwrap_err()
            .code,
        ErrorCode::BundleAborted
    );
    assert!(source_cancelled.load(Ordering::Acquire));
    assert_eq!(ring.buffered_frames(), 0);
    assert!(control.started.try_recv().is_err());
}
