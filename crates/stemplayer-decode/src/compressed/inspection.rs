use super::*;
use crate::media::{Mp4SeekGuard, ProbedFile, check_cancelled, probe_file};
use crate::presentation::track_timeline;

pub fn inspect_audio_file(path: &Path) -> Result<CompressedTrackInfo, PlayerError> {
    inspect_audio_file_cancelled(path, &AtomicBool::new(false))
}

pub fn inspect_audio_file_cancelled(
    path: &Path,
    cancelled: &AtomicBool,
) -> Result<CompressedTrackInfo, PlayerError> {
    let probed = probe_file(path, None, false, cancelled)?;
    let tracks = probed
        .format
        .tracks()
        .iter()
        .filter(|track| track.track_type() == Some(TrackType::Audio))
        .collect::<Vec<_>>();
    if tracks.len() != 1 {
        return Err(PlayerError::new(
            ErrorCode::TrackCountUnsupported,
            "Audio files must contain exactly one audio track",
        ));
    }
    let info = inspected_track(&probed, tracks[0], true)?;
    bounded_audio_frames(info.declared_frames)?;
    probed.access.ensure_unchanged()?;
    check_cancelled(cancelled)?;
    Ok(info)
}

pub(super) fn bounded_audio_frames(frames: Option<u64>) -> Result<u64, PlayerError> {
    frames
        .filter(|frames| *frames > 0 && i64::try_from(*frames).is_ok())
        .ok_or_else(|| {
            PlayerError::new(
                ErrorCode::TimelineInvalid,
                "Audio file duration is unavailable or exceeds the supported timeline",
            )
        })
}

pub fn inspect_compressed_tracks(path: &Path) -> Result<Vec<CompressedTrackInfo>, PlayerError> {
    let cancelled = AtomicBool::new(false);
    let probed = probe_file(path, None, false, &cancelled)?;
    inspect_tracks(&probed)
}

fn inspect_tracks(probed: &ProbedFile<'_>) -> Result<Vec<CompressedTrackInfo>, PlayerError> {
    let tracks = probed
        .format
        .tracks()
        .iter()
        .filter(|track| track.track_type() == Some(TrackType::Audio))
        .map(|track| inspected_track(probed, track, false))
        .collect::<Result<Vec<_>, _>>()?;
    if tracks.is_empty() {
        return Err(PlayerError::new(
            ErrorCode::NoAudioTracks,
            "Stem container contains no audio tracks",
        ));
    }
    if tracks.len() > 16 {
        return Err(PlayerError::new(
            ErrorCode::TrackCountUnsupported,
            "Stem container contains more than 16 tracks",
        ));
    }
    probed.access.ensure_unchanged()?;
    Ok(tracks)
}

pub(super) fn inspected_track(
    probed: &ProbedFile<'_>,
    track: &symphonia::core::formats::Track,
    ordinary: bool,
) -> Result<CompressedTrackInfo, PlayerError> {
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|value| value.audio())
        .ok_or_else(|| codec_error("Audio track has no codec configuration"))?;
    let info = CompressedTrackInfo {
        identity: Some(probed.access.identity.clone()),
        ..validate_parameters(track.id, track.num_frames, parameters, ordinary)?
    };
    if let Some(structure) = &probed.structure {
        let descriptor = structure
            .tracks
            .iter()
            .find(|value| value.track_id == track.id)
            .ok_or_else(|| configuration_error("MP4 audio track is missing"))?;
        let decoded_frames = info
            .media_frames
            .ok_or_else(|| configuration_error("Compressed track frame count is unavailable"))?;
        let timeline = track_timeline(
            structure,
            descriptor,
            info.sample_rate,
            decoded_frames,
            ordinary,
        )?;
        let last_media_frame = timeline
            .segments
            .iter()
            .filter_map(|segment| {
                segment.media_start_frame.map(|start| {
                    start.get() + (segment.end_frame_exclusive.get() - segment.start_frame.get())
                        - 1
                })
            })
            .max()
            .unwrap_or(0);
        let seek_target = if info.codec == CompressedCodec::AacLc {
            last_media_frame
                .saturating_sub(AAC_SEEK_PREROLL_FRAMES)
                .max(0)
        } else {
            last_media_frame
        };
        Mp4SeekGuard::new(probed.format.as_ref(), structure, track.id)?.check(seek_target)?;
        if !ordinary {
            return Ok(info);
        }
        let declared_frames = Some(
            u64::try_from(timeline.end_frame_exclusive.get())
                .map_err(|_| configuration_error("MP4 presentation duration is invalid"))?,
        );
        return Ok(CompressedTrackInfo {
            declared_frames,
            timeline: Some(timeline),
            ..info
        });
    }
    Ok(info)
}

pub fn inspect_compressed_container(
    path: &Path,
    cancelled: &AtomicBool,
) -> Result<CompressedContainer, PlayerError> {
    let probed = probe_file(path, None, true, cancelled)?;
    let structure = probed
        .structure
        .as_ref()
        .ok_or_else(|| PlayerError::new(ErrorCode::ContainerNotMp4, "Stem container is not MP4"))?;
    let roles = resolve_mp4_structure_roles(structure)?;
    let codec_tracks = inspect_tracks(&probed)?;
    let tracks = roles
        .into_iter()
        .map(|resolved| {
            let descriptor = structure
                .tracks
                .iter()
                .find(|track| track.track_id == resolved.track_id)
                .ok_or_else(|| configuration_error("Resolved MP4 track is missing"))?;
            let info = codec_tracks
                .iter()
                .find(|track| track.track_id == resolved.track_id)
                .cloned()
                .ok_or_else(|| configuration_error("Resolved MP4 track has no qualified codec"))?;
            let decoded_frames = info.media_frames.ok_or_else(|| {
                configuration_error("Compressed track frame count is unavailable")
            })?;
            let timeline = track_timeline(
                structure,
                descriptor,
                info.sample_rate,
                decoded_frames,
                false,
            )?;
            Ok(CompressedContainerTrack {
                role: resolved.role,
                role_source: resolved.source,
                info,
                timeline,
                edit_count: descriptor.edits.len(),
                encoded_bytes: descriptor.encoded_bytes.unwrap_or(0),
            })
        })
        .collect::<Result<Vec<_>, PlayerError>>()?;
    if codec_tracks.iter().any(|codec| {
        !tracks
            .iter()
            .any(|track| track.info.track_id == codec.track_id)
    }) {
        return Err(configuration_error(
            "Stem container contains an unresolved audio track",
        ));
    }
    let sample_rate = tracks
        .first()
        .map(|track| track.info.sample_rate)
        .ok_or_else(|| configuration_error("Stem container has no routed tracks"))?;
    if tracks
        .iter()
        .any(|track| track.info.sample_rate != sample_rate)
    {
        return Err(configuration_error(
            "Stem container tracks use different sample rates",
        ));
    }
    let timelines = tracks
        .iter()
        .map(|track| track.timeline.clone())
        .collect::<Vec<_>>();
    let (start_frame, end_frame_exclusive) = common_mp4_range(&timelines)?;
    probed.access.ensure_unchanged()?;
    check_cancelled(cancelled)?;
    Ok(CompressedContainer {
        structure: probed.structure.unwrap(),
        tracks,
        sample_rate,
        start_frame,
        end_frame_exclusive,
    })
}
