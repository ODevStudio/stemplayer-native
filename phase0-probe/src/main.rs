use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{Fft, FixedSync, Resampler};
use serde::Serialize;
use sha2::{Digest, Sha256};
use slint::ComponentHandle;
use std::error::Error;
use std::fs::{File, remove_file, write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

mod codec;

slint::include_modules!();

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Serialize)]
struct Check {
    name: &'static str,
    status: &'static str,
    details: serde_json::Value,
}

#[derive(Serialize)]
struct ProbeReport {
    schema: &'static str,
    timestamp_unix_ms: u128,
    os: &'static str,
    architecture: &'static str,
    checks: Vec<Check>,
}

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("--self-check") => run_self_check(args.get(1).map(PathBuf::from)),
        Some("--ui-smoke") => run_ui(
            args.get(1)
                .and_then(|value| value.parse().ok())
                .unwrap_or(10),
        ),
        Some("--codec-check") => run_codec_check(&args),
        Some(value) => Err(format!("unknown argument: {value}").into()),
        None => run_ui(0),
    }
}

fn run_codec_check(args: &[String]) -> Result<()> {
    let path = args.get(1).ok_or("codec path is required")?;
    let expected_frames = args
        .get(2)
        .ok_or("expected frame count is required")?
        .parse()?;
    let seek_frame = args.get(3).ok_or("seek frame is required")?.parse()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&codec::qualify(
            Path::new(path),
            expected_frames,
            seek_frame
        )?)?
    );
    Ok(())
}

fn run_ui(seconds: u64) -> Result<()> {
    let app = AppWindow::new()?;
    if seconds > 0 {
        slint::Timer::single_shot(Duration::from_secs(seconds), || {
            let _ = slint::quit_event_loop();
        });
    }
    app.run()?;
    Ok(())
}

fn run_self_check(private_container: Option<PathBuf>) -> Result<()> {
    let synthetic = std::env::temp_dir().join(format!(
        "stemplayer-native-phase0-{}.wav",
        std::process::id()
    ));
    write(&synthetic, synthetic_wav())?;
    let private_check = private_container
        .as_deref()
        .map(|path| check_codec("private-container", path));
    let checks = [
        check_sha256(),
        check_ring(),
        check_resampler(),
        check_codec("synthetic-wav", &synthetic),
        check_audio(),
    ]
    .into_iter()
    .chain(private_check)
    .collect();
    remove_file(synthetic)?;
    let report = ProbeReport {
        schema: "stemplayer-phase0-probe-v1",
        timestamp_unix_ms: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        os: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        checks,
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn check_sha256() -> Check {
    let digest = Sha256::digest(b"abc")
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Check {
        name: "sha256",
        status: if digest == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" {
            "pass"
        } else {
            "fail"
        },
        details: serde_json::json!({ "known_vector": digest }),
    }
}

fn check_ring() -> Check {
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(2);
    let first = producer.push(11).is_ok();
    let second = producer.push(22).is_ok();
    let bounded = producer.push(33).is_err();
    let ordered = consumer.pop().ok() == Some(11) && consumer.pop().ok() == Some(22);
    Check {
        name: "spsc-ring",
        status: if first && second && bounded && ordered {
            "pass"
        } else {
            "fail"
        },
        details: serde_json::json!({ "capacity": 2, "bounded": bounded, "ordered": ordered }),
    }
}

fn check_resampler() -> Check {
    match Fft::<f32>::new(44_100, 48_000, 1_024, 2, FixedSync::Both) {
        Ok(resampler) => Check {
            name: "post-mix-resampler",
            status: "pass",
            details: serde_json::json!({
                "input_frames": resampler.input_frames_next(),
                "output_frames": resampler.output_frames_next(),
                "channels": 2
            }),
        },
        Err(error) => Check {
            name: "post-mix-resampler",
            status: "fail",
            details: serde_json::json!({ "error": error.to_string() }),
        },
    }
}

fn check_codec(name: &'static str, path: &Path) -> Check {
    match inspect_and_decode(path) {
        Ok(details) => Check {
            name,
            status: "pass",
            details,
        },
        Err(error) => Check {
            name,
            status: "fail",
            details: serde_json::json!({ "error": error.to_string() }),
        },
    }
}

fn inspect_and_decode(path: &Path) -> Result<serde_json::Value> {
    let file = Box::new(File::open(path)?);
    let stream = MediaSourceStream::new(file, Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let probed = symphonia::default::get_probe().probe(
        &hint,
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    let mut format = probed;
    let track_count = format.tracks().len();
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("no default audio track")?;
    let track_id = track.id;
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|value| value.audio())
        .ok_or("audio codec parameters missing")?;
    let codec = format!("{:?}", parameters.codec);
    let sample_rate = parameters.sample_rate;
    let channels = parameters.channels.as_ref().map(|value| value.count());
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(parameters, &AudioDecoderOptions::default())?;
    let decoded_frames = loop {
        let packet = format.next_packet()?.ok_or("no decodable packet")?;
        if packet.track_id == track_id {
            break decoder.decode(&packet)?.frames();
        }
    };
    Ok(serde_json::json!({
        "track_count": track_count,
        "codec": codec,
        "sample_rate": sample_rate,
        "channels": channels,
        "decoded_frames": decoded_frames
    }))
}

fn check_audio() -> Check {
    match open_default_output() {
        Ok((device, format, rate, channels, callbacks, errors)) => Check {
            name: "native-audio-output",
            status: if callbacks > 0 && errors == 0 {
                "pass"
            } else {
                "fail"
            },
            details: serde_json::json!({
                "device": device,
                "sample_format": format,
                "sample_rate": rate,
                "channels": channels,
                "callbacks": callbacks,
                "errors": errors
            }),
        },
        Err(error) => Check {
            name: "native-audio-output",
            status: "fail",
            details: serde_json::json!({ "error": error.to_string() }),
        },
    }
}

fn open_default_output() -> Result<(String, String, u32, u16, usize, usize)> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("no default output device")?;
    let supported = device.default_output_config()?;
    let sample_format = supported.sample_format();
    let config = supported.config();
    let callbacks = Arc::new(AtomicUsize::new(0));
    let errors = Arc::new(AtomicUsize::new(0));
    let stream = match sample_format {
        cpal::SampleFormat::F32 => {
            build_silence_stream::<f32>(&device, config, &callbacks, &errors)?
        }
        cpal::SampleFormat::I16 => {
            build_silence_stream::<i16>(&device, config, &callbacks, &errors)?
        }
        cpal::SampleFormat::U16 => {
            build_silence_stream::<u16>(&device, config, &callbacks, &errors)?
        }
        value => return Err(format!("unsupported output sample format: {value}").into()),
    };
    stream.play()?;
    std::thread::sleep(Duration::from_millis(300));
    stream.pause()?;
    Ok((
        device.to_string(),
        sample_format.to_string(),
        config.sample_rate,
        config.channels,
        callbacks.load(Ordering::Relaxed),
        errors.load(Ordering::Relaxed),
    ))
}

fn build_silence_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    callbacks: &Arc<AtomicUsize>,
    errors: &Arc<AtomicUsize>,
) -> std::result::Result<cpal::Stream, cpal::Error>
where
    T: cpal::Sample + cpal::SizedSample,
{
    let callback_counter = Arc::clone(callbacks);
    let error_counter = Arc::clone(errors);
    device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            data.fill(T::EQUILIBRIUM);
            callback_counter.fetch_add(1, Ordering::Relaxed);
        },
        move |_| {
            error_counter.fetch_add(1, Ordering::Relaxed);
        },
        None,
    )
}

fn synthetic_wav() -> Vec<u8> {
    let samples = [0.0_f32, 0.0, 0.25, -0.25, 0.5, -0.5, 0.0, 0.0];
    let data_size = (samples.len() * size_of::<f32>()) as u32;
    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&3_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&48_000_u32.to_le_bytes());
    bytes.extend_from_slice(&384_000_u32.to_le_bytes());
    bytes.extend_from_slice(&8_u16.to_le_bytes());
    bytes.extend_from_slice(&32_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    bytes.extend(samples.into_iter().flat_map(f32::to_le_bytes));
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_check_primitives_pass() {
        assert_eq!(check_sha256().status, "pass");
        assert_eq!(check_ring().status, "pass");
        assert_eq!(check_resampler().status, "pass");
    }
}
