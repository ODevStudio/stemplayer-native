use stemplayer_domain::{ErrorCode, PlayerError, SourceFrame};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Mp4EditEntry {
    pub segment_duration: u64,
    pub media_time: i64,
    pub media_rate_integer: i16,
    pub media_rate_fraction: i16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresentationSegment {
    pub start_frame: SourceFrame,
    pub end_frame_exclusive: SourceFrame,
    pub media_start_frame: Option<SourceFrame>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mp4TrackTimeline {
    pub start_frame: SourceFrame,
    pub end_frame_exclusive: SourceFrame,
    pub segments: Vec<PresentationSegment>,
    pub priming_resolved: bool,
    pub padding_resolved: bool,
}

pub fn map_mp4_timeline(
    movie_timescale: u32,
    media_timescale: u32,
    sample_rate: u32,
    decoded_frames: u64,
    edits: &[Mp4EditEntry],
) -> Result<Mp4TrackTimeline, PlayerError> {
    if movie_timescale == 0 || media_timescale == 0 || sample_rate == 0 {
        return Err(timeline_error(
            ErrorCode::TimelineInvalid,
            "MP4 timescales and sample rate must be positive",
        ));
    }
    let decoded_frames = i64::try_from(decoded_frames).map_err(|_| {
        timeline_error(
            ErrorCode::PresentationRangeInvalid,
            "Decoded MP4 frame count exceeds the signed timeline",
        )
    })?;
    if edits.is_empty() {
        return Ok(Mp4TrackTimeline {
            start_frame: SourceFrame::new(0),
            end_frame_exclusive: SourceFrame::new(decoded_frames),
            segments: vec![PresentationSegment {
                start_frame: SourceFrame::new(0),
                end_frame_exclusive: SourceFrame::new(decoded_frames),
                media_start_frame: Some(SourceFrame::new(0)),
            }],
            priming_resolved: false,
            padding_resolved: false,
        });
    }

    let mut presentation_start = 0_i64;
    let mut previous_media_end = None;
    let mut segments = Vec::with_capacity(edits.len());
    let mut media_segments = 0_usize;
    for edit in edits {
        if edit.media_rate_integer != 1 || edit.media_rate_fraction != 0 {
            return Err(timeline_error(
                ErrorCode::EditListUnsupported,
                "MP4 edit rate must be exactly one",
            ));
        }
        if edit.segment_duration == 0 {
            return Err(timeline_error(
                ErrorCode::EditListUnsupported,
                "MP4 edit duration must be positive",
            ));
        }
        let duration_frames =
            scale_exact_unsigned(edit.segment_duration, sample_rate, movie_timescale)?;
        let presentation_end =
            presentation_start
                .checked_add(duration_frames)
                .ok_or_else(|| {
                    timeline_error(
                        ErrorCode::PresentationRangeInvalid,
                        "MP4 presentation range overflowed",
                    )
                })?;

        let media_start_frame = match edit.media_time {
            -1 => None,
            value if value < -1 => {
                return Err(timeline_error(
                    ErrorCode::EditListUnsupported,
                    "MP4 edit contains a negative media time",
                ));
            }
            value => {
                let media_start = scale_exact_signed(value, sample_rate, media_timescale)?;
                let media_end = media_start.checked_add(duration_frames).ok_or_else(|| {
                    timeline_error(
                        ErrorCode::PresentationRangeInvalid,
                        "MP4 media range overflowed",
                    )
                })?;
                if media_end > decoded_frames {
                    return Err(timeline_error(
                        ErrorCode::PresentationRangeInvalid,
                        "MP4 edit exceeds decoded media",
                    ));
                }
                if previous_media_end.is_some_and(|previous| media_start < previous) {
                    return Err(timeline_error(
                        ErrorCode::EditListConflict,
                        "MP4 media edits overlap or run backwards",
                    ));
                }
                previous_media_end = Some(media_end);
                media_segments += 1;
                Some(SourceFrame::new(media_start))
            }
        };
        segments.push(PresentationSegment {
            start_frame: SourceFrame::new(presentation_start),
            end_frame_exclusive: SourceFrame::new(presentation_end),
            media_start_frame,
        });
        presentation_start = presentation_end;
    }
    if media_segments == 0 {
        return Err(timeline_error(
            ErrorCode::EditListUnsupported,
            "MP4 edit list contains no media",
        ));
    }

    Ok(Mp4TrackTimeline {
        start_frame: SourceFrame::new(0),
        end_frame_exclusive: SourceFrame::new(presentation_start),
        segments,
        priming_resolved: true,
        padding_resolved: true,
    })
}

pub fn common_mp4_range(
    timelines: &[Mp4TrackTimeline],
) -> Result<(SourceFrame, SourceFrame), PlayerError> {
    let first = timelines.first().ok_or_else(|| {
        timeline_error(
            ErrorCode::EditListConflict,
            "MP4 container has no routed track timelines",
        )
    })?;
    if timelines.iter().skip(1).any(|timeline| {
        timeline.start_frame != first.start_frame
            || timeline.end_frame_exclusive != first.end_frame_exclusive
    }) {
        return Err(timeline_error(
            ErrorCode::EditListConflict,
            "MP4 routed tracks have incompatible presentation ranges",
        ));
    }
    Ok((first.start_frame, first.end_frame_exclusive))
}

fn scale_exact_unsigned(value: u64, numerator: u32, denominator: u32) -> Result<i64, PlayerError> {
    let scaled = u128::from(value)
        .checked_mul(u128::from(numerator))
        .ok_or_else(|| {
            timeline_error(
                ErrorCode::PresentationRangeInvalid,
                "MP4 timeline scaling overflowed",
            )
        })?;
    if scaled % u128::from(denominator) != 0 {
        return Err(timeline_error(
            ErrorCode::PresentationRangeInvalid,
            "MP4 timeline does not align to source frames",
        ));
    }
    i64::try_from(scaled / u128::from(denominator)).map_err(|_| {
        timeline_error(
            ErrorCode::PresentationRangeInvalid,
            "MP4 timeline exceeds the signed frame range",
        )
    })
}

fn scale_exact_signed(value: i64, numerator: u32, denominator: u32) -> Result<i64, PlayerError> {
    let value = u64::try_from(value).map_err(|_| {
        timeline_error(
            ErrorCode::PresentationRangeInvalid,
            "MP4 media time is negative",
        )
    })?;
    scale_exact_unsigned(value, numerator, denominator)
}

fn timeline_error(code: ErrorCode, summary: &str) -> PlayerError {
    PlayerError::new(code, summary)
        .with_user_action("Choose a stem container with compatible track timelines")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_edit_and_explicit_empty_edit_map_exact_frames() {
        let unresolved = map_mp4_timeline(1_000, 48_000, 48_000, 96_000, &[]).unwrap();
        assert_eq!(unresolved.end_frame_exclusive, SourceFrame::new(96_000));
        assert!(!unresolved.priming_resolved);

        let edits = [
            Mp4EditEntry {
                segment_duration: 500,
                media_time: -1,
                media_rate_integer: 1,
                media_rate_fraction: 0,
            },
            Mp4EditEntry {
                segment_duration: 1_000,
                media_time: 1_024,
                media_rate_integer: 1,
                media_rate_fraction: 0,
            },
        ];
        let resolved = map_mp4_timeline(1_000, 48_000, 48_000, 96_000, &edits).unwrap();
        assert_eq!(resolved.end_frame_exclusive, SourceFrame::new(72_000));
        assert_eq!(
            resolved.segments[1].media_start_frame,
            Some(SourceFrame::new(1_024))
        );
        assert!(resolved.priming_resolved && resolved.padding_resolved);
    }

    #[test]
    fn invalid_rates_ranges_alignment_and_track_conflicts_fail_closed() {
        let edit = |duration, media_time, rate| Mp4EditEntry {
            segment_duration: duration,
            media_time,
            media_rate_integer: rate,
            media_rate_fraction: 0,
        };
        assert_eq!(
            map_mp4_timeline(1_000, 48_000, 48_000, 96_000, &[edit(1_000, 0, 2)])
                .unwrap_err()
                .code,
            ErrorCode::EditListUnsupported
        );
        assert_eq!(
            map_mp4_timeline(1_000, 48_000, 48_000, 1_000, &[edit(1_000, 0, 1)])
                .unwrap_err()
                .code,
            ErrorCode::PresentationRangeInvalid
        );
        assert_eq!(
            map_mp4_timeline(1_001, 48_000, 48_000, 96_000, &[edit(1, 0, 1)])
                .unwrap_err()
                .code,
            ErrorCode::PresentationRangeInvalid
        );

        let first = map_mp4_timeline(1_000, 48_000, 48_000, 48_000, &[]).unwrap();
        let second = map_mp4_timeline(1_000, 48_000, 48_000, 96_000, &[]).unwrap();
        assert_eq!(
            common_mp4_range(&[first, second]).unwrap_err().code,
            ErrorCode::EditListConflict
        );
    }
}
