use super::{Shared, WorkerState, publish};
use std::array;
use std::sync::Arc;
use stemplayer_domain::{
    BoundedText, NetworkDiagnostics, PlayerError, PlayerSnapshot, SampleRate, SourceFrame,
    TransportState,
};
use stemplayer_engine::{PlaybackEngine, PlaybackSession};
use stemplayer_network::{HlsClient, HlsPlaybackSource, UreqTransport};
use stemplayer_waveform::{CacheStatus, Peak, PeakLevel, SourceFingerprint, WaveformSnapshot};

impl WorkerState {
    pub(super) fn open_hls(&mut self, url: &str, shared: &Shared) -> Result<(), PlayerError> {
        self.close_output()?;
        self.engine = None;
        self.analysis = None;
        self.waveform_error = None;
        self.fingerprint = None;
        self.current_source = None;
        *shared
            .current_source
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.hls = None;
        *shared
            .waveform
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        self.generation = self.generation.next()?;
        publish(
            shared,
            PlayerSnapshot::new(
                TransportState::Inspecting,
                self.generation,
                None,
                None,
                vec![],
            )?,
        );
        let client = HlsClient::new(Arc::new(UreqTransport::default()), cfg!(debug_assertions));
        let source = client.open(url, &shared.cancel)?;
        let session = PlaybackSession::new_external(
            source.title(),
            source.sample_rate(),
            source.frames(),
            source.has_master(),
            self.generation,
            Arc::clone(&source) as Arc<dyn stemplayer_engine::ExternalSource>,
        )?;
        *shared
            .waveform
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(remote_waveform(&source, self.generation)?);
        self.engine = Some(PlaybackEngine::with_cancellation(
            session,
            Arc::clone(&shared.cancel),
        )?);
        self.hls = Some(source);
        self.publish(shared)
    }

    pub(super) fn network_diagnostics(&self) -> Option<NetworkDiagnostics> {
        self.hls.as_ref().map(|source| {
            let value = source.diagnostics();
            NetworkDiagnostics {
                redacted_source: BoundedText::truncated(value.source),
                manifest_latency_us: value.manifest_latency_us,
                group_ready_us: value.group_ready_us,
                fetch_count: value.fetch_count,
                fetch_failures: value.fetch_failures,
                buffering_count: value.buffering_count,
                recovery_us: value.recovery_us,
                current_group: value
                    .current_group
                    .and_then(|group| u32::try_from(group).ok()),
                cache_bytes: value.cache_bytes,
                peak_cache_bytes: value.peak_cache_bytes,
                packets_decoded: value.packets_decoded,
                discarded_preroll_frames: value.discarded_preroll_frames,
                inter_stem_offset_frames: value.inter_stem_offset_frames,
            }
        })
    }
}

fn remote_waveform(
    source: &HlsPlaybackSource,
    generation: stemplayer_domain::GenerationId,
) -> Result<WaveformSnapshot, PlayerError> {
    let roles = array::from_fn(|index| {
        Arc::from(
            source.waveforms()[index]
                .minimum
                .iter()
                .zip(&source.waveforms()[index].maximum)
                .map(|(minimum, maximum)| Peak {
                    minimum: *minimum,
                    maximum: *maximum,
                })
                .collect::<Vec<_>>(),
        )
    });
    let frames_per_peak = u32::try_from(source.waveforms()[0].samples_per_peak).map_err(|_| {
        PlayerError::new(
            stemplayer_domain::ErrorCode::HlsStemsetInvalid,
            "HLS waveform resolution exceeds native limits",
        )
    })?;
    Ok(WaveformSnapshot {
        generation,
        fingerprint: SourceFingerprint::from_bytes([0; 32]),
        start_frame: SourceFrame::new(0),
        end_frame_exclusive: SourceFrame::new(source.frames() as i64),
        analyzed_end_frame: SourceFrame::new(source.frames() as i64),
        sample_rate: SampleRate::new(source.sample_rate())?,
        levels: Arc::from([PeakLevel {
            frames_per_peak,
            roles,
        }]),
        progress: 1.0,
        cache_status: CacheStatus::Disabled,
        consistency: None,
    })
}
