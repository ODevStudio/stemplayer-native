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
            ordinary_edits(
                structure.movie_timescale,
                media_timescale,
                sample_rate,
                decoded_frames,
                edits,
            )?,
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
    media_timescale: u32,
    sample_rate: u32,
    decoded_frames: u64,
    edits: Vec<Mp4EditEntry>,
) -> Result<Vec<Mp4EditEntry>, PlayerError> {
    if movie_timescale == 0 || media_timescale == 0 {
        return Err(PlayerError::new(
            ErrorCode::TimelineInvalid,
            "MP4 timescales must be positive",
        ));
    }
    let mut movie_end = 0_u128;
    let mut previous_frame_end = 0_u64;
    let edit_count = edits.len();
    edits
        .into_iter()
        .enumerate()
        .map(|(index, edit)| {
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
            let segment_duration = if index + 1 == edit_count && edit.media_time >= 0 {
                let scaled_start = edit.media_time as u128 * u128::from(sample_rate);
                let media_end =
                    scaled_start / u128::from(media_timescale) + u128::from(segment_duration);
                let excess = media_end.saturating_sub(u128::from(decoded_frames));
                if scaled_start.is_multiple_of(u128::from(media_timescale))
                    && excess < u128::from(segment_duration)
                    && excess * u128::from(movie_timescale) < u128::from(sample_rate)
                {
                    segment_duration - u64::try_from(excess).unwrap()
                } else {
                    segment_duration
                }
            } else {
                segment_duration
            };
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
        let normalized = ordinary_edits(1000, 44_100, 44_100, 44_100, edits).unwrap();
        assert_eq!(normalized[0].segment_duration, 22_094);
        assert_eq!(normalized[1].segment_duration, 22_006);
        let timeline = map_mp4_timeline(44_100, 44_100, 44_100, 44_100, &normalized).unwrap();
        assert_eq!(timeline.segments[0].end_frame_exclusive.get(), 22_094);
        assert_eq!(timeline.end_frame_exclusive.get(), 44_100);
    }

    #[test]
    fn ordinary_edit_rounding_preserves_invalid_range_and_alignment_rejections() {
        for (movie_scale, media_scale, rate, frames, edits) in [
            (1000, 48_000, 48_000, 48_000, vec![edit(1001, 0)]),
            (1000, 44_100, 44_100, 44_099, vec![edit(1001, 0)]),
            (1000, 44_100, 44_100, 44_100, vec![edit(1, 44_100)]),
            (1000, 48_000, 44_100, 44_100, vec![edit(1001, 1)]),
            (
                1000,
                44_100,
                44_100,
                44_100,
                vec![edit(1001, 0), edit(1, -1)],
            ),
        ] {
            let normalized = ordinary_edits(movie_scale, media_scale, rate, frames, edits).unwrap();
            assert_eq!(
                map_mp4_timeline(rate, media_scale, rate, frames, &normalized)
                    .unwrap_err()
                    .code,
                ErrorCode::PresentationRangeInvalid,
            );
        }
        for (movie_scale, media_scale) in [(0, 44_100), (1000, 0)] {
            assert_eq!(
                ordinary_edits(
                    movie_scale,
                    media_scale,
                    44_100,
                    44_100,
                    vec![edit(1001, 0)]
                )
                .unwrap_err()
                .code,
                ErrorCode::TimelineInvalid,
            );
        }
    }
}
