use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use stemplayer_decode::{DecodedSegment, decode_aac_segment};
use stemplayer_domain::{ErrorCode, GenerationId, PlayerError};
use stemplayer_engine::{ExternalSource, SourceBlock};

use crate::client::HttpTransport;
use crate::manifest::{HlsManifest, HlsWaveform, invalid};
use crate::playlist::MediaPlaylist;

const CACHE_CAPACITY_BYTES: usize = 64 * 1024 * 1024;
const MAX_SEGMENT_BYTES: usize = 8 * 1024 * 1024;
const RETRIES: usize = 2;

#[derive(Clone, Debug, Default, Serialize)]
pub struct HlsDiagnostics {
    pub source: String,
    pub manifest_latency_us: u64,
    pub group_ready_us: u64,
    pub fetch_count: u64,
    pub fetch_failures: u64,
    pub buffering_count: u64,
    pub recovery_us: u64,
    pub current_group: Option<usize>,
    pub cache_bytes: u64,
    pub peak_cache_bytes: u64,
    pub packets_decoded: u64,
    pub discarded_preroll_frames: u64,
    pub inter_stem_offset_frames: i64,
}

struct CachedGroup {
    index: usize,
    encoded: Vec<Vec<u8>>,
    decoded: Vec<DecodedSegment>,
    bytes: usize,
}

struct State {
    groups: VecDeque<CachedGroup>,
    diagnostics: HlsDiagnostics,
}

pub struct HlsPlaybackSource {
    transport: Arc<dyn HttpTransport>,
    manifest: HlsManifest,
    playlists: Vec<MediaPlaylist>,
    waveforms: [HlsWaveform; 4],
    frames: u64,
    state: Mutex<State>,
}

impl HlsPlaybackSource {
    pub fn new(
        transport: Arc<dyn HttpTransport>,
        manifest: HlsManifest,
        playlists: Vec<MediaPlaylist>,
        waveforms: [HlsWaveform; 4],
        manifest_latency: Duration,
    ) -> Result<Self, PlayerError> {
        let frames = playlists
            .first()
            .and_then(|playlist| playlist.segments.last())
            .map(|segment| segment.end_frame_exclusive)
            .ok_or_else(|| invalid(ErrorCode::HlsTimelineMismatch, "HLS source has no timeline"))?;
        let source = manifest.redacted_url.clone();
        let initial_fetches = 1_u64 + manifest.routes.len() as u64 + 4;
        Ok(Self {
            transport,
            manifest,
            playlists,
            waveforms,
            frames,
            state: Mutex::new(State {
                groups: VecDeque::new(),
                diagnostics: HlsDiagnostics {
                    source,
                    manifest_latency_us: elapsed_us(manifest_latency),
                    fetch_count: initial_fetches,
                    ..Default::default()
                },
            }),
        })
    }

    pub fn title(&self) -> &str {
        &self.manifest.title
    }

    pub const fn sample_rate(&self) -> u32 {
        self.manifest.sample_rate
    }

    pub const fn frames(&self) -> u64 {
        self.frames
    }

    pub fn has_master(&self) -> bool {
        self.manifest.routes.len() == 5
    }

    pub const fn waveforms(&self) -> &[HlsWaveform; 4] {
        &self.waveforms
    }

    pub fn diagnostics(&self) -> HlsDiagnostics {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .diagnostics
            .clone()
    }

    pub fn clear_cache(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.groups.clear();
        state.diagnostics.cache_bytes = 0;
        state.diagnostics.current_group = None;
    }

    fn group_index(&self, frame: u64) -> Result<usize, PlayerError> {
        self.playlists[0]
            .segments
            .partition_point(|segment| segment.end_frame_exclusive <= frame)
            .min(self.playlists[0].segments.len().saturating_sub(1))
            .checked_add(0)
            .ok_or_else(|| invalid(ErrorCode::HlsTimelineMismatch, "HLS frame has no segment"))
    }

    fn ensure_group(&self, index: usize, cancelled: &AtomicBool) -> Result<(), PlayerError> {
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(position) = state.groups.iter().position(|group| group.index == index) {
                let group = state.groups.remove(position).unwrap();
                state.groups.push_front(group);
                state.diagnostics.current_group = Some(index);
                return Ok(());
            }
            state.diagnostics.buffering_count = state.diagnostics.buffering_count.saturating_add(1);
        }
        let started = Instant::now();
        let descriptor = &self.playlists[0].segments[index];
        let expected_frames =
            usize::try_from(descriptor.end_frame_exclusive - descriptor.start_frame).unwrap();
        let mut encoded = Vec::with_capacity(self.playlists.len());
        let mut decoded = Vec::with_capacity(self.playlists.len());
        for playlist in &self.playlists {
            if cancelled.load(Ordering::Acquire) {
                return Err(PlayerError::new(
                    ErrorCode::BundleAborted,
                    "HLS group request was cancelled",
                ));
            }
            let segment = &playlist.segments[index];
            let maximum = segment_limit(self.manifest.bitrate_bps, segment.duration_us)?;
            let bytes = self.fetch_with_retry(&segment.url, maximum, cancelled)?;
            let audio = decode_aac_segment(
                bytes.clone(),
                self.manifest.sample_rate,
                segment.trim_start_frames,
                expected_frames,
                cancelled,
            )?;
            encoded.push(bytes);
            decoded.push(audio);
        }
        let bytes = encoded.iter().map(Vec::len).sum::<usize>()
            + decoded
                .iter()
                .map(|audio| (audio.left.len() + audio.right.len()) * size_of::<f32>())
                .sum::<usize>();
        if bytes > CACHE_CAPACITY_BYTES {
            return Err(invalid(
                ErrorCode::HlsResponseTooLarge,
                "HLS group exceeds the cache capacity",
            ));
        }
        let packets = decoded
            .iter()
            .map(|audio| u64::from(audio.packets))
            .sum::<u64>();
        let preroll = u64::try_from(descriptor.trim_start_frames * decoded.len()).unwrap();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state
            .groups
            .iter()
            .map(|group| group.bytes)
            .sum::<usize>()
            .saturating_add(bytes)
            > CACHE_CAPACITY_BYTES
        {
            if state.groups.pop_back().is_none() {
                break;
            }
        }
        state.groups.push_front(CachedGroup {
            index,
            encoded,
            decoded,
            bytes,
        });
        let current_bytes = state.groups.iter().map(|group| group.bytes).sum::<usize>();
        state.diagnostics.group_ready_us = elapsed_us(started.elapsed());
        state.diagnostics.recovery_us = state.diagnostics.group_ready_us;
        state.diagnostics.current_group = Some(index);
        state.diagnostics.cache_bytes = current_bytes as u64;
        state.diagnostics.peak_cache_bytes =
            state.diagnostics.peak_cache_bytes.max(current_bytes as u64);
        state.diagnostics.packets_decoded =
            state.diagnostics.packets_decoded.saturating_add(packets);
        state.diagnostics.discarded_preroll_frames = state
            .diagnostics
            .discarded_preroll_frames
            .saturating_add(preroll);
        Ok(())
    }

    fn fetch_with_retry(
        &self,
        url: &url::Url,
        maximum_bytes: usize,
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>, PlayerError> {
        let mut last = None;
        for attempt in 0..=RETRIES {
            if cancelled.load(Ordering::Acquire) {
                return Err(PlayerError::new(
                    ErrorCode::BundleAborted,
                    "HLS request was cancelled",
                ));
            }
            {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.diagnostics.fetch_count = state.diagnostics.fetch_count.saturating_add(1);
            }
            match self.transport.get(url, maximum_bytes) {
                Ok(response) => return Ok(response.bytes),
                Err(error)
                    if attempt < RETRIES
                        && matches!(
                            error.code,
                            ErrorCode::HlsHttpFailed | ErrorCode::HlsGroupBufferingTimeout
                        ) =>
                {
                    let mut state = self
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    state.diagnostics.fetch_failures =
                        state.diagnostics.fetch_failures.saturating_add(1);
                    drop(state);
                    last = Some(error);
                    thread::sleep(Duration::from_millis(25 + 50 * attempt as u64));
                }
                Err(error) => return Err(error),
            }
        }
        Err(last
            .unwrap_or_else(|| PlayerError::new(ErrorCode::HlsHttpFailed, "HLS request failed")))
    }
}

impl ExternalSource for HlsPlaybackSource {
    fn decode_block(
        &self,
        generation: GenerationId,
        start_frame: u64,
        frames: u32,
        cancelled: &AtomicBool,
    ) -> Result<SourceBlock, PlayerError> {
        let end_frame = start_frame.checked_add(u64::from(frames)).ok_or_else(|| {
            invalid(
                ErrorCode::HlsTimelineMismatch,
                "HLS decode range overflowed",
            )
        })?;
        if end_frame > self.frames {
            return Err(invalid(
                ErrorCode::HlsTimelineMismatch,
                "HLS decode range exceeds the timeline",
            ));
        }
        let channels = if self.has_master() { 10 } else { 8 };
        let mut samples = Vec::with_capacity(frames as usize * channels);
        let mut current = start_frame;
        while current < end_frame {
            let index = self.group_index(current)?;
            self.ensure_group(index, cancelled)?;
            let segment = &self.playlists[0].segments[index];
            let copy_end = end_frame.min(segment.end_frame_exclusive);
            let offset = usize::try_from(current - segment.start_frame).unwrap();
            let copy_frames = usize::try_from(copy_end - current).unwrap();
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let group = state
                .groups
                .front()
                .filter(|group| group.index == index)
                .ok_or_else(|| {
                    PlayerError::new(
                        ErrorCode::Internal,
                        "HLS current group cache is unavailable",
                    )
                })?;
            for frame in offset..offset + copy_frames {
                for audio in &group.decoded {
                    samples.push(audio.left[frame]);
                    samples.push(audio.right[frame]);
                }
            }
            let _ = group.encoded.len();
            drop(state);
            current = copy_end;
        }
        SourceBlock::new(generation, start_frame, frames, channels as u8, samples)
    }
}

fn segment_limit(bitrate_bps: u64, duration_us: u64) -> Result<usize, PlayerError> {
    let bytes = u128::from(bitrate_bps)
        .checked_mul(u128::from(duration_us))
        .and_then(|value| value.checked_div(8_000_000))
        .and_then(|value| value.checked_mul(4))
        .and_then(|value| value.checked_add(64 * 1024))
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value <= MAX_SEGMENT_BYTES)
        .ok_or_else(|| {
            invalid(
                ErrorCode::HlsResponseTooLarge,
                "HLS declared segment size exceeds its cap",
            )
        })?;
    Ok(bytes)
}

fn elapsed_us(duration: Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}
