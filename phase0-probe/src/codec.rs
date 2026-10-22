use serde_json::{Value, json};
use std::error::Error;
use std::fs::File;
use std::path::Path;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, Track, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct DecodeSummary {
    codec: String,
    packets: u64,
    decoded_frames: u64,
    playable_frames: u64,
    presentation_frames: u64,
    covered_presentation_frames: u64,
    start_trim_frames: u64,
    end_trim_frames: u64,
    declared_frames: Option<u64>,
    contiguous: bool,
    decoded_covers_timeline: bool,
}

pub fn qualify(path: &Path, expected_frames: u64, seek_frame: u64) -> Result<Value> {
    if expected_frames == 0 || seek_frame >= expected_frames {
        return Err("Expected duration and seek target must describe a nonempty timeline".into());
    }
    let first = decode_all(path)?;
    let second = decode_all(path)?;
    let seek = seek(path, seek_frame)?;
    let exact = first.presentation_frames == expected_frames
        && second.presentation_frames == expected_frames
        && first.covered_presentation_frames == first.presentation_frames
        && second.covered_presentation_frames == second.presentation_frames
        && first.decoded_frames == second.decoded_frames
        && first.contiguous
        && second.contiguous
        && first.decoded_covers_timeline
        && second.decoded_covers_timeline;
    let status = if exact && seek["contains_target"] == true {
        "pass"
    } else {
        "fail"
    };
    Ok(json!({
        "schema": "stemplayer-codec-qualification-v1",
        "status": status,
        "codec": first.codec,
        "expected_frames": expected_frames,
        "sequential": {
            "packets": first.packets,
            "decoded_frames": first.decoded_frames,
            "encoded_playable_frames": first.playable_frames,
            "presentation_frames": first.presentation_frames,
            "covered_presentation_frames": first.covered_presentation_frames,
            "start_trim_frames": first.start_trim_frames,
            "end_trim_frames": first.end_trim_frames,
            "declared_frames": first.declared_frames,
            "contiguous": first.contiguous,
            "decoded_covers_timeline": first.decoded_covers_timeline,
            "repeat_identical": first.decoded_frames == second.decoded_frames
                && first.presentation_frames == second.presentation_frames
        },
        "seek": seek
    }))
}

fn decode_all(path: &Path) -> Result<DecodeSummary> {
    let (mut format, track, parameters) = open(path)?;
    let media = format.media_info();
    let duration = media
        .duration
        .ok_or("Presentation duration is unavailable")?;
    let time_base = media
        .time_base
        .ok_or("Presentation timebase is unavailable")?;
    let rate = parameters.sample_rate.ok_or("Sample rate is unavailable")?;
    if track.time_base.is_none_or(|base| {
        u128::from(base.numer.get()) * u128::from(rate) != u128::from(base.denom.get())
    }) {
        return Err("Codec qualification requires sample-based packet timestamps".into());
    }
    let scaled_duration =
        u128::from(duration.get()) * u128::from(time_base.numer.get()) * u128::from(rate);
    let denominator = u128::from(time_base.denom.get());
    if !scaled_duration.is_multiple_of(denominator) || media.start_ts.get() != 0 {
        return Err("Presentation timeline is not zero-based and sample-aligned".into());
    }
    let presentation_end = i64::try_from(scaled_duration / denominator)?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&parameters, &AudioDecoderOptions::default())?;
    let mut packets = 0;
    let mut decoded_frames = 0;
    let mut playable_frames = 0;
    let presentation_frames = u64::try_from(presentation_end)?;
    let mut covered_presentation_frames = 0;
    let mut start_trim_frames = 0;
    let mut end_trim_frames = 0;
    let mut expected_pts = None;
    let mut contiguous = true;
    let mut decoded_covers_timeline = true;
    while let Some(packet) = format.next_packet()? {
        if packet.track_id != track.id {
            continue;
        }
        let decoded = decoder.decode(&packet)?;
        if let Some(expected_pts) = expected_pts {
            contiguous &= packet.pts == expected_pts;
        }
        decoded_covers_timeline &= decoded.frames() as u64 >= packet.dur.get();
        expected_pts = Some(
            packet
                .pts
                .checked_add(packet.dur)
                .ok_or("timestamp overflow")?,
        );
        let packet_start = packet.pts.get();
        let packet_end = packet_start
            .checked_add_unsigned(packet.dur.get())
            .ok_or("timestamp overflow")?;
        let mapped_start = packet_start.max(0);
        let mapped_end = packet_end.min(presentation_end);
        if mapped_end > mapped_start {
            covered_presentation_frames += u64::try_from(mapped_end - mapped_start)?;
        }
        packets += 1;
        decoded_frames += decoded.frames() as u64;
        playable_frames += packet.dur.get();
        start_trim_frames += packet.trim_start.get();
        end_trim_frames += packet.trim_end.get();
    }
    Ok(DecodeSummary {
        codec: format!("{:?}", parameters.codec),
        packets,
        decoded_frames,
        playable_frames,
        presentation_frames,
        covered_presentation_frames,
        start_trim_frames,
        end_trim_frames,
        declared_frames: track.num_frames,
        contiguous,
        decoded_covers_timeline,
    })
}

fn seek(path: &Path, seek_frame: u64) -> Result<Value> {
    let (mut format, track, parameters) = open(path)?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&parameters, &AudioDecoderOptions::default())?;
    let result = format.seek(
        SeekMode::Accurate,
        SeekTo::Timestamp {
            ts: Timestamp::new(i64::try_from(seek_frame)?),
            track_id: track.id,
        },
    )?;
    decoder.reset();
    while let Some(packet) = format.next_packet()? {
        if packet.track_id != track.id {
            continue;
        }
        let decoded = decoder.decode(&packet)?;
        let packet_start = u64::try_from(packet.pts.get())?;
        let packet_end = packet_start
            .checked_add(packet.dur.get())
            .ok_or("duration overflow")?;
        if seek_frame < packet_end {
            let contains_target =
                packet_start <= seek_frame && decoded.frames() as u64 == packet.block_dur().get();
            return Ok(json!({
                "requested_frame": seek_frame,
                "required_frame": result.required_ts.get(),
                "actual_frame": result.actual_ts.get(),
                "packet_start_frame": packet_start,
                "packet_end_frame": packet_end,
                "decode_offset_frames": packet.trim_start.get() + seek_frame - packet_start,
                "contains_target": contains_target
            }));
        }
    }
    Err("seek target was not decoded".into())
}

fn open(path: &Path) -> Result<(Box<dyn FormatReader>, Track, AudioCodecParameters)> {
    let stream = MediaSourceStream::new(Box::new(File::open(path)?), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let format = symphonia::default::get_probe().probe(
        &hint,
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("no default audio track")?
        .clone();
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|value| value.audio())
        .ok_or("audio codec parameters missing")?
        .clone();
    Ok((format, track, parameters))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn qualification_rejects_shorter_and_longer_expected_durations() {
        let path = std::env::temp_dir().join(format!(
            "stemplayer-codec-duration-{}.wav",
            std::process::id()
        ));
        let mut file = File::create_new(&path).unwrap();
        file.write_all(&crate::synthetic_wav()).unwrap();
        drop(file);
        let exact = qualify(&path, 4, 1);
        let short = qualify(&path, 3, 1);
        let long = qualify(&path, 5, 1);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(exact.unwrap()["status"], "pass");
        let short = short.unwrap();
        assert_eq!(short["status"], "fail");
        assert_eq!(short["sequential"]["presentation_frames"], 4);
        assert_eq!(long.unwrap()["status"], "fail");
    }

    #[test]
    fn invalid_expectations_fail_before_opening_media() {
        let path = Path::new("nonexistent-qualification-input.wav");
        assert!(qualify(path, 0, 0).is_err());
        assert!(qualify(path, 4, 4).is_err());
    }

    #[test]
    fn aac_and_alac_use_container_duration_without_requiring_padding_as_audio() {
        for name in ["stereo-aac.m4a", "stereo-alac.m4a"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../crates/stemplayer-decode/tests/fixtures")
                .join(name);
            let decoded = decode_all(&path).unwrap();
            let frames = decoded.presentation_frames;
            assert!(frames > 1);
            assert_eq!(decoded.covered_presentation_frames, frames, "{name}");
            assert!(decoded.decoded_frames >= frames, "{name}");
            assert_eq!(
                qualify(&path, frames, frames / 2).unwrap()["status"],
                "pass",
                "{name}"
            );
            assert_eq!(
                qualify(&path, frames - 1, 0).unwrap()["status"],
                "fail",
                "{name}"
            );
        }
    }
}
