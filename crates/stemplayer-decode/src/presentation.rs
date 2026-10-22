use stemplayer_domain::{ErrorCode, PlayerError};
use stemplayer_format::{Mp4Structure, Mp4TrackStructure};
use stemplayer_timeline::{Mp4EditEntry, Mp4TrackTimeline, map_mp4_timeline};

pub(crate) fn track_timeline(
    structure: &Mp4Structure,
    descriptor: &Mp4TrackStructure,
    sample_rate: u32,
    decoded_frames: u64,
    ordinary: bool,
) -> Result<Mp4TrackTimeline, PlayerError> {
    let media_timescale = descriptor.media_timescale.ok_or_else(|| {
        PlayerError::new(
            ErrorCode::TimelineInvalid,
            "Compressed track timescale is unavailable",
        )
    })?;
    let edits = descriptor
        .edits
        .iter()
        .map(|edit| Mp4EditEntry {
            segment_duration: edit.segment_duration,
            media_time: edit.media_time,
            media_rate_integer: edit.media_rate_integer,
            media_rate_fraction: edit.media_rate_fraction,
        })
        .collect::<Vec<_>>();
    let (movie_timescale, edits) = if ordinary {
        (
            sample_rate,
            ordinary_edits(structure.movie_timescale, sample_rate, edits)?,
        )
    } else {
        (structure.movie_timescale, edits)
    };
    map_mp4_timeline(
        movie_timescale,
        media_timescale,
        sample_rate,
        decoded_frames,
        &edits,
    )
}

fn ordinary_edits(
    movie_timescale: u32,
    sample_rate: u32,
    edits: Vec<Mp4EditEntry>,
) -> Result<Vec<Mp4EditEntry>, PlayerError> {
    if movie_timescale == 0 {
        return Err(PlayerError::new(
            ErrorCode::TimelineInvalid,
            "MP4 movie timescale must be positive",
        ));
    }
    let mut movie_end = 0_u128;
    let mut previous_frame_end = 0_u64;
    edits
        .into_iter()
        .map(|edit| {
            movie_end = movie_end
                .checked_add(u128::from(edit.segment_duration))
                .ok_or_else(|| {
                    PlayerError::new(
                        ErrorCode::PresentationRangeInvalid,
                        "MP4 edit duration overflowed",
                    )
                })?;
            let frame_end = movie_end
                .checked_mul(u128::from(sample_rate))
                .map(|value| value / u128::from(movie_timescale))
                .and_then(|value| u64::try_from(value).ok())
                .filter(|value| i64::try_from(*value).is_ok())
                .ok_or_else(|| {
                    PlayerError::new(
                        ErrorCode::PresentationRangeInvalid,
                        "MP4 edit duration exceeds the supported timeline",
                    )
                })?;
            let segment_duration = frame_end.checked_sub(previous_frame_end).ok_or_else(|| {
                PlayerError::new(
                    ErrorCode::PresentationRangeInvalid,
                    "MP4 presentation edits run backwards",
                )
            })?;
            previous_frame_end = frame_end;
            Ok(Mp4EditEntry {
                segment_duration,
                ..edit
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(duration: u64, media_time: i64) -> Mp4EditEntry {
        Mp4EditEntry {
            segment_duration: duration,
            media_time,
            media_rate_integer: 1,
            media_rate_fraction: 0,
        }
    }

    #[test]
    fn ordinary_edit_endpoints_floor_cumulatively_without_changing_stem_alignment() {
        let edits = vec![edit(501, 0), edit(499, 22_094)];
        assert_eq!(
            map_mp4_timeline(1000, 44_100, 44_100, 44_100, &edits)
                .unwrap_err()
                .code,
            ErrorCode::PresentationRangeInvalid,
        );
        let normalized = ordinary_edits(1000, 44_100, edits).unwrap();
        assert_eq!(normalized[0].segment_duration, 22_094);
        assert_eq!(normalized[1].segment_duration, 22_006);
        let timeline = map_mp4_timeline(44_100, 44_100, 44_100, 44_100, &normalized).unwrap();
        assert_eq!(timeline.segments[0].end_frame_exclusive.get(), 22_094);
        assert_eq!(timeline.end_frame_exclusive.get(), 44_100);
    }
}
