use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use stemplayer_decode::{
    CompressedContainer, CompressedTrackInfo, inspect_audio_file, inspect_compressed_container,
};
use stemplayer_domain::{
    ChannelCount, DisplayName, ErrorCode, GenerationId, Microseconds, PlayerError, SampleRate,
    SourceFrame, SourceKind, SourceMetadata,
};
use stemplayer_format::StudioBundle;
use stemplayer_timeline::frames_to_microseconds;

pub trait ExternalSource: Send + Sync {
    fn decode_block(
        &self,
        generation: GenerationId,
        start_frame: u64,
        frames: u32,
        cancelled: &AtomicBool,
    ) -> Result<crate::SourceBlock, PlayerError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferConfig {
    pub start_frames: u64,
    pub low_frames: u64,
    pub target_frames: u64,
    pub maximum_frames: u64,
    pub block_frames: u32,
}

impl BufferConfig {
    pub fn for_sample_rate(sample_rate: SampleRate) -> Self {
        let rate = u64::from(sample_rate.get());
        Self {
            start_frames: rate.saturating_mul(3) / 2,
            low_frames: rate.saturating_mul(3),
            target_frames: rate.saturating_mul(6),
            maximum_frames: rate.saturating_mul(12),
            block_frames: stemplayer_decode::MAX_DECODE_FRAMES,
        }
    }
}

#[derive(Clone)]
pub struct PlaybackSession {
    source: SessionSource,
    metadata: SourceMetadata,
    duration: Microseconds,
    generation: GenerationId,
    buffer_config: BufferConfig,
}

#[derive(Clone)]
pub(crate) enum SessionSource {
    Studio(Arc<StudioBundle>),
    Compressed {
        path: PathBuf,
        container: Arc<CompressedContainer>,
    },
    Audio {
        path: PathBuf,
        info: CompressedTrackInfo,
    },
    External(Arc<dyn ExternalSource>),
}

impl PlaybackSession {
    pub fn new(bundle: StudioBundle, generation: GenerationId) -> Result<Self, PlayerError> {
        let report = bundle.report();
        let sample_rate = SampleRate::new(report.sample_rate)?;
        let end_frame = SourceFrame::new(i64::try_from(report.frames).map_err(|_| {
            PlayerError::new(
                ErrorCode::TimelineInvalid,
                "Studio frame count exceeds the native timeline",
            )
        })?);
        let metadata = SourceMetadata {
            display_name: DisplayName::truncated(&report.display_name),
            kind: SourceKind::StudioWavBundle,
            has_master: report.has_master,
            sample_rate,
            channel_count: ChannelCount::new(if report.has_master { 10 } else { 8 })?,
            start_frame: SourceFrame::new(0),
            end_frame_exclusive: end_frame,
        };
        let duration = frames_to_microseconds(end_frame, sample_rate)?;
        Ok(Self {
            source: SessionSource::Studio(Arc::new(bundle)),
            metadata,
            duration,
            generation,
            buffer_config: BufferConfig::for_sample_rate(sample_rate),
        })
    }

    pub fn open_compressed(
        path: &Path,
        generation: GenerationId,
        cancelled: &AtomicBool,
    ) -> Result<Self, PlayerError> {
        let container = inspect_compressed_container(path, cancelled)?;
        let sample_rate = SampleRate::new(container.sample_rate)?;
        let has_master = container
            .tracks
            .iter()
            .any(|track| track.role == stemplayer_domain::TrackRole::Master);
        let display_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Stem container");
        let metadata = SourceMetadata {
            display_name: DisplayName::truncated(display_name),
            kind: SourceKind::LocalStemContainer,
            has_master,
            sample_rate,
            channel_count: ChannelCount::new(if has_master { 10 } else { 8 })?,
            start_frame: container.start_frame,
            end_frame_exclusive: container.end_frame_exclusive,
        };
        let duration = frames_to_microseconds(
            SourceFrame::new(
                container
                    .end_frame_exclusive
                    .get()
                    .checked_sub(container.start_frame.get())
                    .ok_or_else(|| {
                        PlayerError::new(
                            ErrorCode::TimelineInvalid,
                            "Compressed container duration is invalid",
                        )
                    })?,
            ),
            sample_rate,
        )?;
        Ok(Self {
            source: SessionSource::Compressed {
                path: path.to_path_buf(),
                container: Arc::new(container),
            },
            metadata,
            duration,
            generation,
            buffer_config: BufferConfig::for_sample_rate(sample_rate),
        })
    }

    pub fn open_audio(path: &Path, generation: GenerationId) -> Result<Self, PlayerError> {
        let info = inspect_audio_file(path)?;
        let sample_rate = SampleRate::new(info.sample_rate)?;
        let frames = info.declared_frames.unwrap();
        let end_frame = SourceFrame::new(i64::try_from(frames).unwrap());
        let display_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Audio file");
        Ok(Self {
            source: SessionSource::Audio {
                path: path.to_path_buf(),
                info,
            },
            metadata: SourceMetadata {
                display_name: DisplayName::truncated(display_name),
                kind: SourceKind::LocalAudioFile,
                has_master: false,
                sample_rate,
                channel_count: ChannelCount::new(8)?,
                start_frame: SourceFrame::new(0),
                end_frame_exclusive: end_frame,
            },
            duration: frames_to_microseconds(end_frame, sample_rate)?,
            generation,
            buffer_config: BufferConfig::for_sample_rate(sample_rate),
        })
    }

    pub fn new_external(
        title: &str,
        sample_rate: u32,
        frames: u64,
        has_master: bool,
        generation: GenerationId,
        source: Arc<dyn ExternalSource>,
    ) -> Result<Self, PlayerError> {
        let sample_rate = SampleRate::new(sample_rate)?;
        let end_frame = SourceFrame::new(i64::try_from(frames).map_err(|_| {
            PlayerError::new(
                ErrorCode::TimelineInvalid,
                "Network source exceeds the native timeline",
            )
        })?);
        let metadata = SourceMetadata {
            display_name: DisplayName::truncated(title),
            kind: SourceKind::HlsStemset,
            has_master,
            sample_rate,
            channel_count: ChannelCount::new(if has_master { 10 } else { 8 })?,
            start_frame: SourceFrame::new(0),
            end_frame_exclusive: end_frame,
        };
        Ok(Self {
            source: SessionSource::External(source),
            metadata,
            duration: frames_to_microseconds(end_frame, sample_rate)?,
            generation,
            buffer_config: BufferConfig::for_sample_rate(sample_rate),
        })
    }

    pub(crate) const fn source(&self) -> &SessionSource {
        &self.source
    }

    pub const fn metadata(&self) -> &SourceMetadata {
        &self.metadata
    }

    pub const fn duration(&self) -> Microseconds {
        self.duration
    }

    pub const fn generation(&self) -> GenerationId {
        self.generation
    }

    pub const fn buffer_config(&self) -> BufferConfig {
        self.buffer_config
    }

    pub fn fingerprint_inputs(&self) -> Vec<(String, PathBuf)> {
        match &self.source {
            SessionSource::Studio(bundle) => bundle.fingerprint_inputs(),
            SessionSource::Compressed { path, container } => vec![(
                format!(
                    "stem-container-v1:{}:{}:{}:{}",
                    container.sample_rate,
                    container.start_frame.get(),
                    container.end_frame_exclusive.get(),
                    container.tracks.len()
                ),
                path.clone(),
            )],
            SessionSource::Audio { path, info } => vec![(
                format!(
                    "audio-file-v3:{}:{}:{}",
                    info.sample_rate,
                    info.channels,
                    info.declared_frames.unwrap_or_default()
                ),
                path.clone(),
            )],
            SessionSource::External(_) => Vec::new(),
        }
    }

    pub fn advance_generation(&mut self) -> Result<GenerationId, PlayerError> {
        self.generation = self.generation.next()?;
        Ok(self.generation)
    }
}
