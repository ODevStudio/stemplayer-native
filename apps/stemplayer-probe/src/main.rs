#![deny(unsafe_code)]

use serde_json::json;
use std::env;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use stemplayer_decode::{MAX_DECODE_FRAMES, decode_window, inspect_compressed_container};
use stemplayer_domain::{ErrorCode, GenerationId, PlayerError, TransportState};
use stemplayer_engine::{PlaybackEngine, PlaybackSession};
use stemplayer_format::{BoundedReader, StudioBundle, StudioRole, inspect_mp4_structure};
use stemplayer_output::{
    OutputTestOptions, PostMixResampler, StereoFrame, callback_audit, enumerate_devices,
    hardware_reopen_test, operator_recovery_test, run_output_test,
};
use stemplayer_platform::select_local_source;

const EXIT_USAGE: u8 = 2;
const EXIT_VALIDATION: u8 = 3;
const EXIT_UNSUPPORTED: u8 = 4;
const EXIT_INTERNAL: u8 = 5;

enum Failure {
    Usage(PlayerError),
    Validation(PlayerError),
    Unsupported(PlayerError),
    Internal(PlayerError),
}

impl Failure {
    fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => EXIT_USAGE,
            Self::Validation(_) => EXIT_VALIDATION,
            Self::Unsupported(_) => EXIT_UNSUPPORTED,
            Self::Internal(_) => EXIT_INTERNAL,
        }
    }

    fn error(&self) -> &PlayerError {
        match self {
            Self::Usage(value)
            | Self::Validation(value)
            | Self::Unsupported(value)
            | Self::Internal(value) => value,
        }
    }
}

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(line) => {
                println!("{line}");
                ExitCode::SUCCESS
            }
            Err(_) => ExitCode::from(
                Failure::Internal(PlayerError::new(
                    ErrorCode::Internal,
                    "JSON serialization failed",
                ))
                .exit_code(),
            ),
        },
        Err(failure) => {
            if let Ok(line) = serde_json::to_string(failure.error()) {
                eprintln!("{line}");
            }
            ExitCode::from(failure.exit_code())
        }
    }
}

fn run(mut args: impl Iterator<Item = String>) -> Result<serde_json::Value, Failure> {
    let command = args.next().ok_or_else(usage_error)?;
    match command.as_str() {
        "version" if args.next().is_none() => Ok(json!({
            "schema": "stemplayer-probe-v1",
            "version": env!("CARGO_PKG_VERSION")
        })),
        "validate-config" if args.next().is_none() => Ok(json!({
            "schema": "stemplayer-probe-config-v1",
            "status": "valid",
            "features": enabled_features()
        })),
        "devices" if args.next().is_none() => enumerate_devices()
            .map(|devices| json!({"schema": "stemplayer-devices-v1", "devices": devices}))
            .map_err(Failure::Internal),
        "callback-audit" if args.next().is_none() => callback_audit(10_000)
            .and_then(|report| {
                serde_json::to_value(report).map_err(|_| {
                    PlayerError::new(ErrorCode::Internal, "Audit serialization failed")
                })
            })
            .map_err(Failure::Internal),
        "device-reopen-test" if args.next().is_none() => {
            hardware_reopen_test(OutputTestOptions::default(), 10)
                .and_then(|report| {
                    serde_json::to_value(report).map_err(|_| {
                        PlayerError::new(ErrorCode::Internal, "Audit serialization failed")
                    })
                })
                .map_err(Failure::Internal)
        }
        "output-test" => output_test(args),
        "device-recovery-test" => device_recovery_test(args),
        "inspect" => studio_report(args, false),
        "container-inspect" => container_report(args),
        "container-structure" => container_structure_report(args),
        "validate" => studio_report(args, true),
        "decode-window" => studio_decode_window(args),
        "playback-sim" => playback_sim(args),
        "container-seek-test" => container_seek_test(args),
        "hls-validate" => hls_validate(args),
        "hls-playback-sim" => hls_playback_sim(args),
        "benchmark" | "cache-inspect" => {
            require_arguments(&mut args, 1, command.as_str())?;
            Err(feature_error(command.as_str()))
        }
        "render" => {
            require_arguments(&mut args, 2, command.as_str())?;
            Err(feature_error(command.as_str()))
        }
        _ => Err(usage_error()),
    }
}

#[cfg(feature = "hls")]
fn hls_validate(mut args: impl Iterator<Item = String>) -> Result<serde_json::Value, Failure> {
    let url = args.next().ok_or_else(usage_error)?;
    if args.next().is_some() {
        return Err(usage_error());
    }
    let client = stemplayer_network::HlsClient::new(
        std::sync::Arc::new(stemplayer_network::UreqTransport::default()),
        cfg!(debug_assertions),
    );
    let source = client
        .open(&url, &std::sync::atomic::AtomicBool::new(false))
        .map_err(Failure::Validation)?;
    Ok(serde_json::json!({
        "title": source.title(),
        "sample_rate": source.sample_rate(),
        "frames": source.frames(),
        "has_master": source.has_master(),
        "waveform_peaks": source.waveforms().iter().map(|waveform| waveform.minimum.len()).collect::<Vec<_>>(),
        "diagnostics": source.diagnostics()
    }))
}

#[cfg(feature = "hls")]
fn hls_playback_sim(mut args: impl Iterator<Item = String>) -> Result<serde_json::Value, Failure> {
    let url = args.next().ok_or_else(usage_error)?;
    if args.next().is_some() {
        return Err(usage_error());
    }
    let cancellation = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let client = stemplayer_network::HlsClient::new(
        std::sync::Arc::new(stemplayer_network::UreqTransport::default()),
        cfg!(debug_assertions),
    );
    let source = client
        .open(&url, &cancellation)
        .map_err(Failure::Validation)?;
    let session = stemplayer_engine::PlaybackSession::new_external(
        source.title(),
        source.sample_rate(),
        source.frames(),
        source.has_master(),
        stemplayer_domain::GenerationId::INITIAL,
        std::sync::Arc::clone(&source) as std::sync::Arc<dyn stemplayer_engine::ExternalSource>,
    )
    .map_err(Failure::Internal)?;
    let mut engine = stemplayer_engine::PlaybackEngine::with_cancellation(session, cancellation)
        .map_err(Failure::Internal)?;
    engine.play().map_err(Failure::Internal)?;
    wait_for_playback(&mut engine)?;
    let mut output = [stemplayer_output::StereoFrame::default(); 4096];
    let mut rendered = 0_u64;
    while !engine.source_exhausted() {
        engine.refill().map_err(Failure::Internal)?;
        let frames = engine.render(&mut output).map_err(Failure::Internal)?;
        if frames == 0 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }
        rendered = rendered.saturating_add(frames as u64);
    }
    engine
        .finish_if_output_drained(0)
        .map_err(Failure::Internal)?;
    let end_frame = engine.position().get();
    let seek_targets = [0, source.frames() / 2, source.frames().saturating_sub(4096)];
    for target in seek_targets {
        engine
            .seek(stemplayer_domain::SourceFrame::new(target as i64))
            .map_err(Failure::Internal)?;
        engine.play().map_err(Failure::Internal)?;
        wait_for_playback(&mut engine)?;
        if engine.render(&mut output).map_err(Failure::Internal)? == 0 {
            return Err(Failure::Internal(PlayerError::new(
                ErrorCode::HlsGroupBufferingTimeout,
                "HLS seek did not produce audio",
            )));
        }
        engine.pause().map_err(Failure::Internal)?;
    }
    Ok(serde_json::json!({
        "frames": source.frames(),
        "rendered_frames": rendered,
        "end_frame": end_frame,
        "state": "ended",
        "seek_targets": seek_targets,
        "diagnostics": source.diagnostics()
    }))
}

#[cfg(not(feature = "hls"))]
fn hls_validate(mut args: impl Iterator<Item = String>) -> Result<serde_json::Value, Failure> {
    require_arguments(&mut args, 1, "hls-validate")?;
    Err(feature_error("hls-validate"))
}

#[cfg(not(feature = "hls"))]
fn hls_playback_sim(mut args: impl Iterator<Item = String>) -> Result<serde_json::Value, Failure> {
    require_arguments(&mut args, 1, "hls-playback-sim")?;
    Err(feature_error("hls-playback-sim"))
}

fn playback_sim(mut args: impl Iterator<Item = String>) -> Result<serde_json::Value, Failure> {
    let source = args.next().ok_or_else(usage_error)?;
    if source.is_empty() {
        return Err(validation_error("Source must not be empty"));
    }
    let output_rate = match args.next() {
        None => None,
        Some(flag) if flag == "--output-rate" => Some(parse_positive(
            &args.next().ok_or_else(usage_error)?,
            "output sample rate",
        )?),
        Some(_) => return Err(usage_error()),
    };
    if args.next().is_some() {
        return Err(usage_error());
    }
    let cancelled = AtomicBool::new(false);
    let path = std::path::Path::new(&source);
    let is_manifest = path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("manifest.json"));
    let session = if path.is_file() && !is_manifest {
        PlaybackSession::open_compressed(path, GenerationId::INITIAL, &cancelled)
            .map_err(Failure::Validation)?
    } else {
        let group = select_local_source(path).map_err(Failure::Validation)?;
        let bundle =
            StudioBundle::inspect(group, false, &cancelled).map_err(Failure::Validation)?;
        PlaybackSession::new(bundle, GenerationId::INITIAL).map_err(Failure::Validation)?
    };
    let mut engine = PlaybackEngine::new(session).map_err(Failure::Internal)?;
    engine.play().map_err(Failure::Internal)?;
    wait_for_playback(&mut engine)?;

    let mut output = [StereoFrame::default(); 2_048];
    let source_rate = engine.sample_rate().get();
    let output_rate = output_rate.unwrap_or(source_rate);
    let input_chunk =
        ((2_048_u128 * u128::from(source_rate)) / u128::from(output_rate)).clamp(1, 2_048) as usize;
    let mut resampler = PostMixResampler::new(
        source_rate,
        output_rate,
        input_chunk,
        engine.remaining_source_frames(),
    )
    .map_err(Failure::Internal)?;
    let mut rendered_frames = 0_u64;
    let mut first = None;
    let mut last = None;
    while !resampler.finished() {
        engine.refill().map_err(Failure::Internal)?;
        let needed = resampler.input_frames_next();
        if needed > output.len() {
            return Err(Failure::Unsupported(PlayerError::new(
                ErrorCode::FeatureUnsupported,
                "The output sample-rate ratio exceeds the bounded render chunk",
            )));
        }
        let produced = if engine.source_exhausted() {
            0
        } else {
            engine
                .render(&mut output[..needed])
                .map_err(Failure::Internal)?
        };
        if produced == 0 && !engine.source_exhausted() {
            std::thread::sleep(std::time::Duration::from_millis(1));
            continue;
        }
        let converted = resampler
            .process(&output[..produced], engine.source_exhausted())
            .map_err(Failure::Internal)?;
        if let Some(frame) = converted.first() {
            first.get_or_insert(*frame);
        }
        if let Some(frame) = converted.last() {
            last = Some(*frame);
        }
        rendered_frames = rendered_frames.saturating_add(converted.len() as u64);
    }
    engine
        .finish_if_output_drained(0)
        .map_err(Failure::Internal)?;
    let ring = engine.ring_metrics();
    let decode = engine.decode_metrics();
    let resampler = resampler.snapshot();
    Ok(json!({
        "schema": "stemplayer-playback-sim-v1",
        "state": format!("{:?}", engine.state()).to_ascii_uppercase(),
        "rendered_frames": rendered_frames,
        "end_frame_exclusive": engine.position().get(),
        "duration_microseconds": engine.duration().get(),
        "source_rate": source_rate,
        "output_rate": output_rate,
        "first_frame": first.map(|frame| [frame.left, frame.right]),
        "last_frame": last.map(|frame| [frame.left, frame.right]),
        "ring_capacity_frames": ring.capacity_frames,
        "minimum_buffered_frames": decode.minimum_buffered_frames,
        "maximum_buffered_frames": decode.maximum_buffered_frames,
        "decoded_frames": decode.decoded_frames,
        "bytes_read": decode.bytes_read,
        "maximum_pending_blocks": decode.maximum_pending_blocks,
        "worker_count": decode.worker_count,
        "ring_overflows": ring.overflows,
        "stale_blocks": ring.stale_blocks,
        "resampler_delay_frames": resampler.delay_frames,
        "resampler_input_frames": resampler.input_frames,
        "resampler_output_frames": resampler.output_frames
    }))
}

fn container_seek_test(
    mut args: impl Iterator<Item = String>,
) -> Result<serde_json::Value, Failure> {
    let source = args.next().ok_or_else(usage_error)?;
    let iterations = match args.next() {
        None => 16,
        Some(flag) if flag == "--iterations" => {
            parse_positive(&args.next().ok_or_else(usage_error)?, "iterations")?
        }
        Some(_) => return Err(usage_error()),
    };
    if iterations > 64 || args.next().is_some() {
        return Err(validation_error("Seek iterations must be between 1 and 64"));
    }
    let cancelled = AtomicBool::new(false);
    let session = PlaybackSession::open_compressed(
        std::path::Path::new(&source),
        GenerationId::INITIAL,
        &cancelled,
    )
    .map_err(Failure::Validation)?;
    let mut engine = PlaybackEngine::new(session).map_err(Failure::Internal)?;
    let end = engine
        .snapshot()
        .map_err(Failure::Internal)?
        .source()
        .unwrap()
        .end_frame_exclusive
        .get() as u64;
    let mut seed = 0x9e37_79b9_u64;
    let mut latencies = Vec::with_capacity(iterations as usize + 3);
    let targets = [0, end / 2, end.saturating_sub(256)]
        .into_iter()
        .chain((0..iterations).map(|_| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            seed % end.saturating_sub(256).max(1)
        }));
    let mut output = [StereoFrame::default(); 256];
    let mut maximum_packets = 0_u32;
    let mut maximum_discarded = 0_u64;
    for target in targets {
        engine
            .seek(stemplayer_domain::SourceFrame::new(target as i64))
            .map_err(Failure::Validation)?;
        engine.play().map_err(Failure::Internal)?;
        wait_for_playback(&mut engine)?;
        let diagnostics = engine
            .snapshot()
            .map_err(Failure::Internal)?
            .details()
            .source_diagnostics
            .clone()
            .ok_or_else(|| {
                Failure::Internal(PlayerError::new(
                    ErrorCode::Internal,
                    "Compressed seek diagnostics are unavailable",
                ))
            })?;
        latencies.push(diagnostics.seek_latency_us);
        maximum_packets = maximum_packets.max(diagnostics.seek_packet_count);
        maximum_discarded = maximum_discarded.max(diagnostics.seek_discarded_frames);
        let rendered = engine.render(&mut output).map_err(Failure::Internal)?;
        if rendered != output.len()
            || output
                .iter()
                .any(|frame| !frame.left.is_finite() || !frame.right.is_finite())
        {
            return Err(Failure::Validation(PlayerError::new(
                ErrorCode::AudioDataInvalid,
                "Seek did not produce a complete finite synchronized window",
            )));
        }
        engine.pause().map_err(Failure::Internal)?;
    }
    latencies.sort_unstable();
    let p95_index = (latencies.len() * 95).div_ceil(100).saturating_sub(1);
    Ok(json!({
        "schema": "stemplayer-container-seek-v1",
        "seek_count": latencies.len(),
        "p95_latency_us": latencies[p95_index],
        "maximum_latency_us": latencies.last().copied().unwrap_or(0),
        "maximum_packets": maximum_packets,
        "maximum_discarded_frames": maximum_discarded,
        "window_frames": output.len()
    }))
}

fn wait_for_playback(engine: &mut PlaybackEngine) -> Result<(), Failure> {
    while engine.state() == TransportState::Priming {
        engine.refill().map_err(Failure::Internal)?;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    if engine.state() == TransportState::Playing {
        Ok(())
    } else {
        Err(Failure::Internal(PlayerError::new(
            ErrorCode::PlaybackNotReady,
            "Playback did not finish priming",
        )))
    }
}

fn studio_report(
    mut args: impl Iterator<Item = String>,
    verify_hashes: bool,
) -> Result<serde_json::Value, Failure> {
    let source = one_argument(&mut args)?;
    let cancelled = AtomicBool::new(false);
    let group = select_local_source(source.as_ref()).map_err(Failure::Validation)?;
    let bundle =
        StudioBundle::inspect(group, verify_hashes, &cancelled).map_err(Failure::Validation)?;
    serde_json::to_value(bundle.report()).map_err(|_| {
        Failure::Internal(PlayerError::new(
            ErrorCode::Internal,
            "Report serialization failed",
        ))
    })
}

fn container_report(mut args: impl Iterator<Item = String>) -> Result<serde_json::Value, Failure> {
    let source = one_argument(&mut args)?;
    let path = std::path::Path::new(&source);
    let cancelled = AtomicBool::new(false);
    let container = inspect_compressed_container(path, &cancelled).map_err(Failure::Validation)?;
    let tracks = container
        .tracks
        .iter()
        .map(|track| {
            let sample_count = container
                .structure
                .tracks
                .iter()
                .find(|value| value.track_id == track.info.track_id)
                .and_then(|value| value.sample_count);
            json!({
                "track_id": track.info.track_id,
                "role": track.role,
                "metadata_source": track.role_source,
                "codec": track.info.codec,
                "sample_rate": track.info.sample_rate,
                "channels": track.info.channels,
                "bits_per_sample": track.info.bits_per_sample,
                "sample_count": sample_count,
                "encoded_bytes": track.encoded_bytes,
                "edit_count": track.edit_count,
                "priming_resolved": track.timeline.priming_resolved,
                "padding_resolved": track.timeline.padding_resolved,
                "start_frame": track.timeline.start_frame.get(),
                "end_frame_exclusive": track.timeline.end_frame_exclusive.get()
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "schema": "stemplayer-container-v1",
        "source_name": path.file_name().and_then(|value| value.to_str()).unwrap_or("Stem container"),
        "container": "mp4",
        "major_brand": container.structure.major_brand,
        "compatible_brands": container.structure.compatible_brands,
        "file_bytes": container.structure.file_bytes,
        "metadata_bytes_read": container.structure.metadata_bytes_read,
        "movie_timescale": container.structure.movie_timescale,
        "sample_rate": container.sample_rate,
        "start_frame": container.start_frame.get(),
        "end_frame_exclusive": container.end_frame_exclusive.get(),
        "tracks": tracks
    }))
}

fn container_structure_report(
    mut args: impl Iterator<Item = String>,
) -> Result<serde_json::Value, Failure> {
    let source = one_argument(&mut args)?;
    let cancelled = AtomicBool::new(false);
    let structure = inspect_mp4_structure(
        BoundedReader::open(std::path::Path::new(&source)).map_err(Failure::Validation)?,
        &cancelled,
    )
    .map_err(Failure::Validation)?;
    serde_json::to_value(structure).map_err(|_| {
        Failure::Internal(PlayerError::new(
            ErrorCode::Internal,
            "Container structure serialization failed",
        ))
    })
}

fn studio_decode_window(
    mut args: impl Iterator<Item = String>,
) -> Result<serde_json::Value, Failure> {
    let source = args.next().ok_or_else(usage_error)?;
    if source.is_empty() {
        return Err(validation_error("Source must not be empty"));
    }
    let mut start_frame = None;
    let mut frames = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(usage_error)?;
        match flag.as_str() {
            "--start-frame" => {
                start_frame = Some(parse_nonnegative(&value, "start frame")?);
            }
            "--frames" => {
                let value = parse_positive(&value, "frame count")?;
                if value > MAX_DECODE_FRAMES {
                    return Err(validation_error("Frame count must not exceed 4096"));
                }
                frames = Some(value);
            }
            _ => return Err(usage_error()),
        }
    }
    let start_frame = start_frame.ok_or_else(usage_error)?;
    let frames = frames.ok_or_else(usage_error)?;
    let cancelled = AtomicBool::new(false);
    let group = select_local_source(source.as_ref()).map_err(Failure::Validation)?;
    let bundle = StudioBundle::inspect(group, true, &cancelled).map_err(Failure::Validation)?;
    let roles = StudioRole::STEMS
        .into_iter()
        .chain(bundle.report().has_master.then_some(StudioRole::Master));
    let windows = roles
        .map(|role| decode_window(&bundle, role, start_frame, frames, &cancelled))
        .collect::<Result<Vec<_>, _>>()
        .map_err(Failure::Validation)?;
    Ok(json!({
        "schema": "stemplayer-decode-window-v1",
        "start_frame": start_frame,
        "requested_frames": frames,
        "windows": windows
    }))
}

fn one_argument(args: &mut impl Iterator<Item = String>) -> Result<String, Failure> {
    let value = args.next().ok_or_else(usage_error)?;
    if value.is_empty() {
        return Err(validation_error("Source must not be empty"));
    }
    if args.next().is_some() {
        return Err(usage_error());
    }
    Ok(value)
}

fn require_arguments(
    args: &mut impl Iterator<Item = String>,
    count: usize,
    command: &str,
) -> Result<(), Failure> {
    let values = args.take(count + 1).collect::<Vec<_>>();
    if values.len() != count {
        return Err(Failure::Usage(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            format!("{command} expects {count} explicit argument(s)"),
        )));
    }
    if values.iter().any(String::is_empty) {
        return Err(validation_error("Arguments must not be empty"));
    }
    Ok(())
}

fn device_recovery_test(
    mut args: impl Iterator<Item = String>,
) -> Result<serde_json::Value, Failure> {
    if args.next().as_deref() != Some("--seconds") {
        return Err(usage_error());
    }
    let value = args.next().ok_or_else(usage_error)?;
    if args.next().is_some() {
        return Err(usage_error());
    }
    let seconds = u64::from(parse_positive(&value, "recovery observation")?);
    operator_recovery_test(seconds)
        .and_then(|report| {
            serde_json::to_value(report)
                .map_err(|_| PlayerError::new(ErrorCode::Internal, "Report serialization failed"))
        })
        .map_err(Failure::Internal)
}

fn output_test(mut args: impl Iterator<Item = String>) -> Result<serde_json::Value, Failure> {
    let mut options = OutputTestOptions::default();
    while let Some(flag) = args.next() {
        let value = args.next().ok_or_else(usage_error)?;
        match flag.as_str() {
            "--device" if !value.is_empty() => options.device_id = Some(value),
            "--rate" => options.sample_rate = Some(parse_positive(&value, "sample rate")?),
            "--seconds" => options.seconds = u64::from(parse_positive(&value, "duration")?),
            "--pause-cycles" => options.pause_cycles = parse_positive(&value, "pause cycles")?,
            _ => return Err(usage_error()),
        }
    }
    run_output_test(options)
        .and_then(|report| {
            serde_json::to_value(report)
                .map_err(|_| PlayerError::new(ErrorCode::Internal, "Report serialization failed"))
        })
        .map_err(Failure::Internal)
}

fn parse_positive(value: &str, name: &str) -> Result<u32, Failure> {
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| validation_error(&format!("{name} must be a positive integer")))
}

fn parse_nonnegative(value: &str, name: &str) -> Result<u64, Failure> {
    value
        .parse::<u64>()
        .map_err(|_| validation_error(&format!("{name} must be a nonnegative integer")))
}

fn validation_error(summary: &str) -> Failure {
    Failure::Validation(PlayerError::new(ErrorCode::ValueOutOfRange, summary))
}

fn usage_error() -> Failure {
    Failure::Usage(PlayerError::new(
        ErrorCode::ValueOutOfRange,
        "Expected version, validate-config, devices, output-test, callback-audit, device-reopen-test, device-recovery-test, inspect, container-inspect, container-structure, container-seek-test, validate, decode-window, render, playback-sim, benchmark, cache-inspect, hls-validate, or hls-playback-sim",
    ))
}

fn feature_error(feature: &str) -> Failure {
    Failure::Unsupported(PlayerError::new(
        ErrorCode::FeatureUnsupported,
        format!("{feature} is not implemented in the active phase"),
    ))
}

fn enabled_features() -> Vec<&'static str> {
    [
        (cfg!(feature = "local-wav"), "local-wav"),
        (cfg!(feature = "local-mp4"), "local-mp4"),
        (cfg!(feature = "aac"), "aac"),
        (cfg!(feature = "alac"), "alac"),
        (cfg!(feature = "hls"), "hls"),
    ]
    .into_iter()
    .filter_map(|(enabled, name)| enabled.then_some(name))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_exit_class_is_stable() {
        assert_eq!(
            Failure::Internal(PlayerError::new(ErrorCode::Internal, "test")).exit_code(),
            5
        );
    }
}
