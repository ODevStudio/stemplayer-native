#![deny(unsafe_code)]

use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;
use stemplayer_domain::GenerationId;
use stemplayer_engine::{SourceBlock, SynchronizedRing};
use stemplayer_format::sha256_bytes;

const BUILD_INFO: &str = include_str!(concat!(env!("OUT_DIR"), "/build-info.json"));
const FRAMES: u32 = 32_768;
const BLOCK_FRAMES: u32 = 256;
const CHANNELS: u8 = 8;
const MAX_ITERATIONS: u32 = 64;
const MAX_REPORT_BYTES: usize = 64 * 1024;
const HELP: &str = "stemplayer-support --build-info|--report|--benchmark [--iterations 1..64] [--output FILE]\nReports are local-only. Output files must not exist. The synthetic benchmark does not qualify hardware audio.";

fn sample(frame: u64, channel: u8) -> f32 {
    ((frame * 17 + u64::from(channel) * 31) % 1024) as f32 / 1024.0 - 0.5
}

fn benchmark(iterations: u32) -> Result<Value, &'static str> {
    let mut elapsed = Vec::with_capacity(iterations as usize);
    let mut output = Vec::with_capacity(FRAMES as usize * usize::from(CHANNELS) * 4);
    for _ in 0..iterations {
        output.clear();
        let started = Instant::now();
        let mut ring = SynchronizedRing::new(
            u64::from(BLOCK_FRAMES) * 2,
            BLOCK_FRAMES,
            CHANNELS,
            GenerationId::INITIAL,
            0,
        )
        .map_err(|_| "source-ring initialization failed")?;
        let mut frame = [0.0_f32; CHANNELS as usize];
        for start in (0..FRAMES).step_by(BLOCK_FRAMES as usize) {
            let mut samples = Vec::with_capacity(BLOCK_FRAMES as usize * usize::from(CHANNELS));
            for position in start..start + BLOCK_FRAMES {
                for channel in 0..CHANNELS {
                    samples.push(sample(u64::from(position), channel));
                }
            }
            let block = SourceBlock::new(
                GenerationId::INITIAL,
                u64::from(start),
                BLOCK_FRAMES,
                CHANNELS,
                samples,
            )
            .map_err(|_| "synthetic block creation failed")?;
            if !ring
                .publish(block)
                .map_err(|_| "source-ring publish failed")?
            {
                return Err("source-ring rejected a synthetic block");
            }
            for position in start..start + BLOCK_FRAMES {
                if !ring.pop_frame(&mut frame) {
                    return Err("source-ring synthetic underrun");
                }
                for channel in 0..CHANNELS {
                    let actual = frame[usize::from(channel)];
                    if actual != sample(u64::from(position), channel) {
                        return Err("source-ring sample mismatch");
                    }
                    output.extend_from_slice(&actual.to_le_bytes());
                }
            }
        }
        let metrics = ring.metrics();
        if ring.read_frame() != u64::from(FRAMES)
            || ring.write_frame() != u64::from(FRAMES)
            || metrics.buffered_frames != 0
            || metrics.consumed_frames != u64::from(FRAMES)
            || metrics.overflows != 0
            || metrics.stale_blocks != 0
        {
            return Err("source-ring counter mismatch");
        }
        elapsed.push(u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX));
    }
    let mut sorted = elapsed.clone();
    sorted.sort_unstable();
    Ok(json!({
        "name": "synchronized-source-ring-v1",
        "generator_version": 1,
        "iterations": iterations,
        "frames_per_iteration": FRAMES,
        "channels": CHANNELS,
        "block_frames": BLOCK_FRAMES,
        "sample_format": "interleaved-f32-little-endian",
        "output_sha256": sha256_bytes(&output),
        "elapsed_ns": elapsed,
        "median_ns": sorted[sorted.len() / 2],
        "p95_ns": sorted[(sorted.len() * 95).div_ceil(100) - 1],
        "sample_mismatches": 0,
        "ring_overflows": 0,
        "qualification": "not-performed",
        "measurement_scope": "generation, allocation, ring publish/consume, comparison, and encoding; not device playback"
    }))
}

fn write_report(value: &Value, destination: Option<PathBuf>) -> Result<(), &'static str> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| "report serialization failed")?;
    bytes.push(b'\n');
    if bytes.len() > MAX_REPORT_BYTES {
        return Err("report size limit exceeded");
    }
    if let Some(path) = destination {
        // create_new rejects existing files and final symlinks. Never overwrite a
        // settings file, media file, or an earlier report selected by mistake.
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "could not create a new report file")?;
        if file
            .write_all(&bytes)
            .and_then(|()| file.sync_all())
            .is_err()
        {
            drop(file);
            let _ = fs::remove_file(path);
            return Err("report write failed");
        }
    } else {
        std::io::stdout()
            .lock()
            .write_all(&bytes)
            .map_err(|_| "report output failed")?;
    }
    Ok(())
}

fn run() -> Result<(), &'static str> {
    let args = std::env::args_os().skip(1).take(8).collect::<Vec<_>>();
    if args.len() == 1 && args[0] == "--help" {
        println!("{HELP}");
        return Ok(());
    }
    if args.is_empty() || args.len() > 5 {
        return Err("invalid arguments; use --help");
    }
    let mode = args[0].to_str().ok_or("invalid command")?;
    if !matches!(mode, "--build-info" | "--report" | "--benchmark") {
        return Err("invalid command; use --help");
    }
    let mut destination = None;
    let mut iterations = None;
    let mut remaining = args[1..].iter();
    while let Some(flag) = remaining.next() {
        let value: &OsString = remaining.next().ok_or("option value is missing")?;
        match flag.to_str() {
            Some("--output") if destination.is_none() && !value.is_empty() => {
                destination = Some(PathBuf::from(value));
            }
            Some("--iterations") if mode == "--benchmark" && iterations.is_none() => {
                let count = value
                    .to_str()
                    .and_then(|text| text.parse::<u32>().ok())
                    .filter(|count| (1..=MAX_ITERATIONS).contains(count))
                    .ok_or("iterations must be between 1 and 64")?;
                iterations = Some(count);
            }
            _ => return Err("unknown or repeated option"),
        }
    }
    let identity: Value =
        serde_json::from_str(BUILD_INFO).map_err(|_| "embedded build identity is invalid")?;
    let report = match mode {
        "--build-info" => identity,
        "--report" => json!({
            "schema_version": 1,
            "kind": "local-support-report",
            "build": identity,
            "collection": {
                "media": false,
                "paths": false,
                "urls": false,
                "settings": false,
                "environment": false,
                "device_inventory": false,
                "live_player_state": false,
                "network": false
            },
            "qualification": "not-performed"
        }),
        "--benchmark" => json!({
            "schema_version": 1,
            "kind": "synthetic-support-benchmark",
            "build": identity,
            "benchmark": benchmark(iterations.unwrap_or(4))?
        }),
        _ => unreachable!("mode was validated"),
    };
    write_report(&report, destination)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
