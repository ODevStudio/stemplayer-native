use stemplayer_domain::{ErrorCode, PlayerError};
use url::Url;

use crate::manifest::{MAX_SAFE_INTEGER, MAX_SEGMENTS, invalid};
use crate::url_policy::child_url;

pub const MAX_PLAYLIST_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmentDescriptor {
    pub url: Url,
    pub duration_us: u64,
    pub start_frame: u64,
    pub end_frame_exclusive: u64,
    pub trim_start_frames: usize,
}

#[derive(Clone, Debug)]
pub struct MediaPlaylist {
    pub media_sequence: u64,
    pub segments: Vec<SegmentDescriptor>,
    pub duration_us: u64,
}

impl MediaPlaylist {
    pub fn parse(
        bytes: &[u8],
        playlist_url: &Url,
        sample_rate: u32,
        preroll_frames: usize,
    ) -> Result<Self, PlayerError> {
        if bytes.len() > MAX_PLAYLIST_BYTES {
            return Err(invalid(
                ErrorCode::HlsResponseTooLarge,
                "HLS playlist exceeds 1 MiB",
            ));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| invalid(ErrorCode::HlsStemsetInvalid, "HLS playlist is not UTF-8"))?;
        let lines = text
            .lines()
            .map(|line| line.trim_end_matches('\r').trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        if lines.first() != Some(&"#EXTM3U")
            || lines.last() != Some(&"#EXT-X-ENDLIST")
            || !lines.contains(&"#EXT-X-PLAYLIST-TYPE:VOD")
        {
            return Err(invalid(
                ErrorCode::HlsStemsetInvalid,
                "HLS playlist must be static VOD",
            ));
        }
        let mut media_sequence = 0_u64;
        let mut duration_us = 0_u64;
        let mut raw = Vec::new();
        let mut index = 1;
        while index < lines.len() {
            let line = lines[index];
            if let Some(value) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
                media_sequence = value.parse().map_err(|_| {
                    invalid(
                        ErrorCode::HlsStemsetInvalid,
                        "HLS media sequence is invalid",
                    )
                })?;
                if media_sequence > MAX_SAFE_INTEGER {
                    return Err(invalid(
                        ErrorCode::HlsStemsetInvalid,
                        "HLS media sequence is invalid",
                    ));
                }
            } else if let Some(value) = line.strip_prefix("#EXTINF:") {
                let duration = parse_duration(value.split(',').next().unwrap_or(""))?;
                let reference = lines
                    .get(index + 1)
                    .filter(|next| !next.starts_with('#'))
                    .ok_or_else(|| {
                        invalid(
                            ErrorCode::HlsStemsetInvalid,
                            "HLS EXTINF has no segment reference",
                        )
                    })?;
                duration_us = duration_us.checked_add(duration).ok_or_else(|| {
                    invalid(ErrorCode::HlsTimelineMismatch, "HLS duration overflowed")
                })?;
                raw.push((child_url(playlist_url, reference)?, duration, duration_us));
                index += 1;
            } else if line.starts_with('#')
                && !line.starts_with("#EXT-X-VERSION:")
                && !line.starts_with("#EXT-X-TARGETDURATION:")
                && line != "#EXT-X-PLAYLIST-TYPE:VOD"
                && line != "#EXT-X-INDEPENDENT-SEGMENTS"
                && line != "#EXT-X-ENDLIST"
            {
                return Err(invalid(
                    ErrorCode::HlsStemsetInvalid,
                    "HLS playlist contains an unsupported tag",
                ));
            } else if !line.starts_with('#') {
                return Err(invalid(
                    ErrorCode::HlsStemsetInvalid,
                    "HLS playlist contains an unpaired segment reference",
                ));
            }
            index += 1;
        }
        if raw.is_empty() || raw.len() > MAX_SEGMENTS {
            return Err(invalid(
                ErrorCode::HlsStemsetInvalid,
                "HLS playlist segment count is invalid",
            ));
        }
        let segments = raw
            .into_iter()
            .enumerate()
            .scan(
                0_u64,
                |start_frame, (index, (url, duration_us, cumulative_us))| {
                    let end_frame = scale(cumulative_us, sample_rate);
                    let result = SegmentDescriptor {
                        url,
                        duration_us,
                        start_frame: *start_frame,
                        end_frame_exclusive: end_frame,
                        trim_start_frames: if index == 0 { 0 } else { preroll_frames },
                    };
                    *start_frame = end_frame;
                    Some(result)
                },
            )
            .collect();
        Ok(Self {
            media_sequence,
            segments,
            duration_us,
        })
    }
}

fn scale(microseconds: u64, sample_rate: u32) -> u64 {
    (u128::from(microseconds) * u128::from(sample_rate) / 1_000_000) as u64
}

fn parse_duration(value: &str) -> Result<u64, PlayerError> {
    let (seconds, fraction) = value.split_once('.').unwrap_or((value, ""));
    if seconds.is_empty()
        || !seconds.bytes().all(|value| value.is_ascii_digit())
        || fraction.len() > 6
        || !fraction.bytes().all(|value| value.is_ascii_digit())
    {
        return Err(invalid(
            ErrorCode::HlsStemsetInvalid,
            "HLS EXTINF duration is invalid",
        ));
    }
    let seconds: u64 = seconds.parse().map_err(|_| {
        invalid(
            ErrorCode::HlsStemsetInvalid,
            "HLS EXTINF duration is invalid",
        )
    })?;
    let fraction = format!("{fraction:0<6}").parse::<u64>().map_err(|_| {
        invalid(
            ErrorCode::HlsStemsetInvalid,
            "HLS EXTINF duration is invalid",
        )
    })?;
    let duration = seconds
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_add(fraction))
        .ok_or_else(|| {
            invalid(
                ErrorCode::HlsTimelineMismatch,
                "HLS EXTINF duration overflowed",
            )
        })?;
    if duration == 0 {
        return Err(invalid(
            ErrorCode::HlsStemsetInvalid,
            "HLS segment duration must be positive",
        ));
    }
    Ok(duration)
}

pub fn validate_parity(
    playlists: &[MediaPlaylist],
    manifest_duration_us: u64,
    segment_count: usize,
) -> Result<(), PlayerError> {
    let reference = playlists
        .first()
        .ok_or_else(|| invalid(ErrorCode::HlsRoleMissing, "HLS source has no routes"))?;
    if reference.segments.len() != segment_count
        || reference.duration_us.abs_diff(manifest_duration_us) > 1_000
    {
        return Err(invalid(
            ErrorCode::HlsTimelineMismatch,
            "HLS playlist does not match the manifest timeline",
        ));
    }
    for playlist in &playlists[1..] {
        if playlist.media_sequence != reference.media_sequence
            || playlist.duration_us != reference.duration_us
            || playlist.segments.len() != reference.segments.len()
        {
            return Err(invalid(
                ErrorCode::HlsTimelineMismatch,
                "HLS route timelines differ",
            ));
        }
        if playlist
            .segments
            .iter()
            .zip(&reference.segments)
            .any(|(left, right)| {
                left.duration_us != right.duration_us
                    || left.start_frame != right.start_frame
                    || left.end_frame_exclusive != right.end_frame_exclusive
            })
        {
            return Err(invalid(
                ErrorCode::HlsSegmentBoundaryMismatch,
                "HLS route segment boundaries differ",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exact_vod_timeline_without_accumulating_rounding() {
        let url = Url::parse("https://example.test/a/track.m3u8").unwrap();
        let playlist = MediaPlaylist::parse(b"#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:0.000023,\na.m4a\n#EXTINF:0.999977,\nb.m4a\n#EXT-X-ENDLIST\n", &url, 44_100, 1024).unwrap();
        assert_eq!(playlist.media_sequence, 7);
        assert_eq!(playlist.segments[0].end_frame_exclusive, 1);
        assert_eq!(playlist.segments[1].start_frame, 1);
        assert_eq!(playlist.segments[1].end_frame_exclusive, 44_100);
        assert_eq!(playlist.segments[1].trim_start_frames, 1024);
    }

    #[test]
    fn rejects_oversized_media_sequence() {
        let url = Url::parse("https://example.test/a/track.m3u8").unwrap();
        let playlist = b"#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:9007199254740992\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:1.000000,\na.m4a\n#EXT-X-ENDLIST\n";

        assert_eq!(
            MediaPlaylist::parse(playlist, &url, 48_000, 0)
                .unwrap_err()
                .code,
            ErrorCode::HlsStemsetInvalid
        );
    }
}
