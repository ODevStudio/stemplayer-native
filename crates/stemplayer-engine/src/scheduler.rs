use crate::session::SessionSource;
use crate::{PlaybackSession, SourceBlock, SynchronizedRing};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::thread;
use stemplayer_decode::{
    CompressedContainerTrack, CompressedDecodeDiagnostics, CompressedTrackDecoder,
    StudioTrackDecoder,
};
use stemplayer_domain::{ErrorCode, PlayerError, StemRole, TrackRole};
use stemplayer_format::StudioRole;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DecodeMetrics {
    pub minimum_buffered_frames: u64,
    pub maximum_buffered_frames: u64,
    pub decoded_frames: u64,
    pub bytes_read: u64,
    pub pending_blocks: u8,
    pub maximum_pending_blocks: u8,
    pub worker_count: u8,
    pub packets_decoded: u64,
    pub discarded_frames: u64,
}

pub struct DecodeScheduler {
    cancelled: Arc<AtomicBool>,
    metrics: DecodeMetrics,
    compressed_routes: Option<Vec<CompressedRoute>>,
    audio_decoder: Option<CompressedTrackDecoder>,
    studio_decoders: Option<Vec<StudioTrackDecoder>>,
}

struct CompressedRoute {
    track: CompressedContainerTrack,
    decoder: CompressedTrackDecoder,
}

struct StereoWindow {
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Default for DecodeScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl DecodeScheduler {
    pub fn new() -> Self {
        Self::with_cancellation(Arc::new(AtomicBool::new(false)))
    }

    pub fn with_cancellation(cancelled: Arc<AtomicBool>) -> Self {
        Self {
            cancelled,
            metrics: DecodeMetrics {
                minimum_buffered_frames: u64::MAX,
                maximum_buffered_frames: 0,
                decoded_frames: 0,
                bytes_read: 0,
                pending_blocks: 0,
                maximum_pending_blocks: 0,
                worker_count: 1,
                packets_decoded: 0,
                discarded_frames: 0,
            },
            compressed_routes: None,
            audio_decoder: None,
            studio_decoders: None,
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn cancellation_token(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    pub fn fill_to(
        &mut self,
        session: &PlaybackSession,
        ring: &mut SynchronizedRing,
        target_frames: u64,
    ) -> Result<(), PlayerError> {
        self.cancelled.store(false, Ordering::Release);
        let target = target_frames.min(session.buffer_config().maximum_frames);
        let end = session.metadata().end_frame_exclusive.get() as u64;
        while ring.buffered_frames() < target && ring.write_frame() < end {
            let frames = u64::from(session.buffer_config().block_frames)
                .min(target.saturating_sub(ring.buffered_frames()))
                .min(ring.available_frames())
                .min(end.saturating_sub(ring.write_frame()));
            if frames == 0 {
                break;
            }
            self.metrics.pending_blocks = 1;
            self.metrics.maximum_pending_blocks = self.metrics.maximum_pending_blocks.max(1);
            let block = self.decode_block(session, ring.write_frame(), frames as u32)?;
            self.metrics.pending_blocks = 0;
            if !ring.publish(block)? {
                break;
            }
            self.metrics.decoded_frames = self.metrics.decoded_frames.saturating_add(frames);
            self.record_buffered(ring.buffered_frames());
        }
        self.metrics.pending_blocks = 0;
        self.record_buffered(ring.buffered_frames());
        Ok(())
    }

    pub fn metrics(&self) -> DecodeMetrics {
        let mut metrics = self.metrics;
        if metrics.minimum_buffered_frames == u64::MAX {
            metrics.minimum_buffered_frames = 0;
        }
        metrics
    }

    fn decode_block(
        &mut self,
        session: &PlaybackSession,
        start_frame: u64,
        frames: u32,
    ) -> Result<SourceBlock, PlayerError> {
        match session.source() {
            SessionSource::Studio(bundle) => {
                self.decode_studio_block(session, bundle, start_frame, frames)
            }
            SessionSource::Compressed { path, container } => {
                if self.compressed_routes.is_none() {
                    self.compressed_routes = Some(open_compressed_routes(path, container)?);
                    self.metrics.worker_count = u8::try_from(container.tracks.len()).unwrap();
                }
                let cancellation = Arc::clone(&self.cancelled);
                let routes = self.compressed_routes.as_mut().unwrap();
                let mut windows = Vec::with_capacity(routes.len());
                for route in routes {
                    if cancellation.load(Ordering::Acquire) {
                        return Err(PlayerError::new(
                            ErrorCode::BundleAborted,
                            "Decode generation was cancelled",
                        ));
                    }
                    let (window, diagnostics) =
                        decode_compressed_window(route, start_frame, frames, &cancellation)?;
                    self.metrics.bytes_read = self
                        .metrics
                        .bytes_read
                        .saturating_add(diagnostics.encoded_bytes_read);
                    self.metrics.packets_decoded = self
                        .metrics
                        .packets_decoded
                        .saturating_add(u64::from(diagnostics.packets_decoded));
                    self.metrics.discarded_frames = self
                        .metrics
                        .discarded_frames
                        .saturating_add(diagnostics.discarded_frames);
                    windows.push(window);
                }
                interleave(session, start_frame, frames, &windows)
            }
            SessionSource::Audio { path, info } => {
                if self.audio_decoder.is_none() {
                    self.audio_decoder =
                        Some(CompressedTrackDecoder::open_audio(path, info.track_id)?);
                    self.metrics.worker_count = 1;
                }
                let window = self.audio_decoder.as_mut().unwrap().decode_window(
                    start_frame,
                    frames,
                    &self.cancelled,
                )?;
                self.metrics.bytes_read = self
                    .metrics
                    .bytes_read
                    .saturating_add(window.diagnostics.encoded_bytes_read);
                self.metrics.packets_decoded = self
                    .metrics
                    .packets_decoded
                    .saturating_add(u64::from(window.diagnostics.packets_decoded));
                self.metrics.discarded_frames = self
                    .metrics
                    .discarded_frames
                    .saturating_add(window.diagnostics.discarded_frames);
                let silence = || StereoWindow {
                    left: vec![0.0; frames as usize],
                    right: vec![0.0; frames as usize],
                };
                interleave(
                    session,
                    start_frame,
                    frames,
                    &[
                        StereoWindow {
                            left: window.left,
                            right: window.right,
                        },
                        silence(),
                        silence(),
                        silence(),
                    ],
                )
            }
            SessionSource::External(source) => {
                source.decode_block(session.generation(), start_frame, frames, &self.cancelled)
            }
        }
    }

    fn decode_studio_block(
        &mut self,
        session: &PlaybackSession,
        bundle: &stemplayer_format::StudioBundle,
        start_frame: u64,
        frames: u32,
    ) -> Result<SourceBlock, PlayerError> {
        if self.studio_decoders.is_none() {
            let mut roles = Vec::with_capacity(5);
            roles.extend(StudioRole::STEMS);
            if session.metadata().has_master {
                roles.push(StudioRole::Master);
            }
            self.studio_decoders = Some(
                roles
                    .into_iter()
                    .map(|role| StudioTrackDecoder::open(bundle, role))
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        let decoders = self.studio_decoders.as_mut().unwrap();
        self.metrics.worker_count = u8::try_from(decoders.len()).unwrap();
        for decoder in decoders.iter_mut() {
            if self.cancelled.load(Ordering::Acquire) {
                return Err(PlayerError::new(
                    ErrorCode::BundleAborted,
                    "Decode generation was cancelled",
                ));
            }
            let window = decoder.decode_window(start_frame, frames, &self.cancelled)?;
            if window.start_frame != start_frame || window.frames != frames {
                return Err(PlayerError::new(
                    ErrorCode::BundleInconsistent,
                    "Studio tracks did not decode the same frame range",
                ));
            }
            self.metrics.bytes_read = self.metrics.bytes_read.saturating_add(window.bytes_read);
        }
        let channels = session.metadata().channel_count.get();
        let mut samples = Vec::with_capacity(frames as usize * usize::from(channels));
        for frame in 0..frames as usize {
            for decoder in decoders.iter() {
                let (left, right) = decoder.samples();
                samples.push(left[frame]);
                samples.push(right[frame]);
            }
        }
        SourceBlock::new(session.generation(), start_frame, frames, channels, samples)
    }

    fn record_buffered(&mut self, frames: u64) {
        self.metrics.minimum_buffered_frames = self.metrics.minimum_buffered_frames.min(frames);
        self.metrics.maximum_buffered_frames = self.metrics.maximum_buffered_frames.max(frames);
    }
}

pub(crate) struct DecodeProducer {
    sender: SyncSender<DecodeRequest>,
    receiver: Receiver<DecodeResult>,
    cancelled: Arc<AtomicBool>,
    pending_generation: Option<stemplayer_domain::GenerationId>,
    metrics: DecodeMetrics,
}

struct DecodeRequest {
    session: PlaybackSession,
    start_frame: u64,
    frames: u32,
}

struct DecodeResult {
    generation: stemplayer_domain::GenerationId,
    block: Result<SourceBlock, PlayerError>,
    metrics: DecodeMetrics,
}

impl DecodeProducer {
    pub fn new(cancelled: Arc<AtomicBool>) -> Result<Self, PlayerError> {
        let (sender, requests) = sync_channel::<DecodeRequest>(1);
        let (results, receiver) = sync_channel(1);
        let worker_cancelled = Arc::clone(&cancelled);
        thread::Builder::new()
            .name("stemplayer-decode".to_owned())
            .spawn(move || {
                let mut scheduler = DecodeScheduler::with_cancellation(worker_cancelled);
                while let Ok(request) = requests.recv() {
                    let generation = request.session.generation();
                    scheduler.metrics.pending_blocks = 1;
                    scheduler.metrics.maximum_pending_blocks = 1;
                    let block = scheduler.decode_block(
                        &request.session,
                        request.start_frame,
                        request.frames,
                    );
                    scheduler.metrics.pending_blocks = 0;
                    if let Ok(block) = &block {
                        scheduler.metrics.decoded_frames = scheduler
                            .metrics
                            .decoded_frames
                            .saturating_add(u64::from(block.frames));
                    }
                    if results
                        .send(DecodeResult {
                            generation,
                            block,
                            metrics: scheduler.metrics(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|source| {
                PlayerError::new(ErrorCode::Internal, "Decode producer could not start")
                    .with_source(source)
            })?;
        Ok(Self {
            sender,
            receiver,
            cancelled,
            pending_generation: None,
            metrics: DecodeMetrics::default(),
        })
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn fill_to(
        &mut self,
        session: &PlaybackSession,
        ring: &mut SynchronizedRing,
        target_frames: u64,
    ) -> Result<(), PlayerError> {
        self.poll(ring)?;
        let target = target_frames.min(session.buffer_config().maximum_frames);
        let end = session.metadata().end_frame_exclusive.get() as u64;
        if let Some(generation) = self.pending_generation {
            if generation != session.generation() {
                self.cancelled.store(true, Ordering::Release);
            }
            return Ok(());
        }
        if ring.buffered_frames() >= target || ring.write_frame() >= end {
            return Ok(());
        }
        let frames = u64::from(session.buffer_config().block_frames)
            .min(target.saturating_sub(ring.buffered_frames()))
            .min(ring.available_frames())
            .min(end.saturating_sub(ring.write_frame()));
        if frames == 0 {
            return Ok(());
        }
        self.cancelled.store(false, Ordering::Release);
        self.sender
            .try_send(DecodeRequest {
                session: session.clone(),
                start_frame: ring.write_frame(),
                frames: frames as u32,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => {
                    PlayerError::new(ErrorCode::Internal, "Decode producer queue is full")
                }
                TrySendError::Disconnected(_) => {
                    PlayerError::new(ErrorCode::Internal, "Decode producer terminated")
                }
            })?;
        self.pending_generation = Some(session.generation());
        self.metrics.pending_blocks = 1;
        self.metrics.maximum_pending_blocks = 1;
        Ok(())
    }

    pub fn metrics(&self) -> DecodeMetrics {
        self.metrics
    }

    fn poll(&mut self, ring: &mut SynchronizedRing) -> Result<(), PlayerError> {
        let result = match self.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return Ok(()),
            Err(TryRecvError::Disconnected) => {
                return Err(PlayerError::new(
                    ErrorCode::Internal,
                    "Decode producer terminated",
                ));
            }
        };
        self.pending_generation = None;
        let minimum = self.metrics.minimum_buffered_frames;
        let maximum = self.metrics.maximum_buffered_frames;
        self.metrics = result.metrics;
        self.metrics.pending_blocks = 0;
        self.metrics.maximum_pending_blocks = 1;
        self.metrics.minimum_buffered_frames = minimum;
        self.metrics.maximum_buffered_frames = maximum;
        if result.generation != ring.generation() {
            return Ok(());
        }
        let block = result.block?;
        if ring.publish(block)? {
            self.metrics.minimum_buffered_frames = if self.metrics.minimum_buffered_frames == 0 {
                ring.buffered_frames()
            } else {
                self.metrics
                    .minimum_buffered_frames
                    .min(ring.buffered_frames())
            };
            self.metrics.maximum_buffered_frames = self
                .metrics
                .maximum_buffered_frames
                .max(ring.buffered_frames());
        }
        Ok(())
    }
}

pub struct AnalysisDecoder {
    session: PlaybackSession,
    scheduler: DecodeScheduler,
}

impl AnalysisDecoder {
    pub fn new(session: &PlaybackSession, cancelled: Arc<AtomicBool>) -> Self {
        Self {
            session: session.clone(),
            scheduler: DecodeScheduler::with_cancellation(cancelled),
        }
    }

    pub fn decode(&mut self, start_frame: u64, frames: u32) -> Result<SourceBlock, PlayerError> {
        self.scheduler
            .decode_block(&self.session, start_frame, frames)
    }

    pub fn cancel(&self) {
        self.scheduler.cancel();
    }
}

fn open_compressed_routes(
    path: &std::path::Path,
    container: &stemplayer_decode::CompressedContainer,
) -> Result<Vec<CompressedRoute>, PlayerError> {
    let mut roles = StemRole::ALL
        .map(TrackRole::Stem)
        .into_iter()
        .collect::<Vec<_>>();
    if container
        .tracks
        .iter()
        .any(|track| track.role == TrackRole::Master)
    {
        roles.push(TrackRole::Master);
    }
    roles
        .into_iter()
        .map(|role| {
            let track = container
                .tracks
                .iter()
                .find(|track| track.role == role)
                .cloned()
                .ok_or_else(|| {
                    PlayerError::new(
                        ErrorCode::TrackConfigurationMismatch,
                        "Compressed route is missing",
                    )
                })?;
            let decoder = CompressedTrackDecoder::open(path, track.info.track_id)?;
            Ok(CompressedRoute { track, decoder })
        })
        .collect()
}

fn decode_compressed_window(
    route: &mut CompressedRoute,
    start_frame: u64,
    frames: u32,
    cancelled: &AtomicBool,
) -> Result<(StereoWindow, CompressedDecodeDiagnostics), PlayerError> {
    let end_frame = start_frame.checked_add(u64::from(frames)).ok_or_else(|| {
        PlayerError::new(
            ErrorCode::PresentationRangeInvalid,
            "Decode range overflowed",
        )
    })?;
    let mut left = vec![0.0_f32; frames as usize];
    let mut right = vec![0.0_f32; frames as usize];
    let mut covered = 0_u64;
    let mut diagnostics = CompressedDecodeDiagnostics::default();
    for segment in &route.track.timeline.segments {
        let segment_start = u64::try_from(segment.start_frame.get()).map_err(|_| {
            PlayerError::new(
                ErrorCode::PresentationRangeInvalid,
                "Timeline starts before zero",
            )
        })?;
        let segment_end = u64::try_from(segment.end_frame_exclusive.get()).map_err(|_| {
            PlayerError::new(
                ErrorCode::PresentationRangeInvalid,
                "Timeline ends before zero",
            )
        })?;
        let copy_start = start_frame.max(segment_start);
        let copy_end = end_frame.min(segment_end);
        if copy_start >= copy_end {
            continue;
        }
        let copy_frames = u32::try_from(copy_end - copy_start).unwrap();
        if let Some(media_start) = segment.media_start_frame {
            let media_start = media_start
                .get()
                .checked_add(i64::try_from(copy_start - segment_start).unwrap())
                .and_then(|value| u64::try_from(value).ok())
                .ok_or_else(|| {
                    PlayerError::new(
                        ErrorCode::PresentationRangeInvalid,
                        "Compressed media range is invalid",
                    )
                })?;
            let window = route
                .decoder
                .decode_window(media_start, copy_frames, cancelled)?;
            let target = usize::try_from(copy_start - start_frame).unwrap();
            let end = target + copy_frames as usize;
            left[target..end].copy_from_slice(&window.left);
            right[target..end].copy_from_slice(&window.right);
            diagnostics.packets_decoded = diagnostics
                .packets_decoded
                .saturating_add(window.diagnostics.packets_decoded);
            diagnostics.encoded_bytes_read = diagnostics
                .encoded_bytes_read
                .saturating_add(window.diagnostics.encoded_bytes_read);
            diagnostics.discarded_frames = diagnostics
                .discarded_frames
                .saturating_add(window.diagnostics.discarded_frames);
            diagnostics.required_seek_frame = window.diagnostics.required_seek_frame;
            diagnostics.actual_seek_frame = window.diagnostics.actual_seek_frame;
        }
        covered = covered.saturating_add(u64::from(copy_frames));
    }
    if covered != u64::from(frames) {
        return Err(PlayerError::new(
            ErrorCode::AudioDataInvalid,
            "Compressed timeline does not cover the requested frame window",
        ));
    }
    Ok((StereoWindow { left, right }, diagnostics))
}

fn interleave(
    session: &PlaybackSession,
    start_frame: u64,
    frames: u32,
    windows: &[StereoWindow],
) -> Result<SourceBlock, PlayerError> {
    let channels = session.metadata().channel_count.get();
    let mut samples = Vec::with_capacity(frames as usize * usize::from(channels));
    for frame in 0..frames as usize {
        for window in windows {
            samples.push(window.left[frame]);
            samples.push(window.right[frame]);
        }
    }
    SourceBlock::new(session.generation(), start_frame, frames, channels, samples)
}
