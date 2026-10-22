use crate::identity::{endpoint_id, selected_index};
use crate::ring::{
    OutputConsumer, OutputMetrics, OutputProducer, OutputSample, StereoFrame, output_ring,
    output_ring_at_generation,
};
use crate::{PresentationEstimate, output_refill_interval};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, Device, SampleFormat, SizedSample, Stream, StreamConfig, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange,
};
use serde::Serialize;
use std::f32::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use stemplayer_domain::{ErrorCode, PlayerError};

const DEFAULT_RING_CAPACITY_FRAMES: usize = 8_192;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConfigReport {
    pub channels: u16,
    pub minimum_sample_rate: u32,
    pub maximum_sample_rate: u32,
    pub sample_format: String,
    pub buffer_size: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DeviceReport {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub default_configuration: Option<ConfigReport>,
    pub supported_configurations: Vec<ConfigReport>,
    pub stereo_supported: bool,
}

#[derive(Clone, Debug)]
pub struct OutputTestOptions {
    pub device_id: Option<String>,
    pub sample_rate: Option<u32>,
    pub seconds: u64,
    pub pause_cycles: u32,
    pub amplitude: f32,
    pub ring_capacity_frames: usize,
}

impl Default for OutputTestOptions {
    fn default() -> Self {
        Self {
            device_id: None,
            sample_rate: None,
            seconds: 5,
            pause_cycles: 0,
            amplitude: 0.1,
            ring_capacity_frames: DEFAULT_RING_CAPACITY_FRAMES,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct NegotiatedConfigReport {
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_format: String,
    pub buffer_size: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct OutputTestReport {
    pub schema: &'static str,
    pub device_id: String,
    pub device_name: String,
    pub negotiated: NegotiatedConfigReport,
    pub duration_seconds: u64,
    pub pause_cycles: u32,
    pub metrics: OutputMetrics,
    pub stream_errors: u64,
    pub shutdown_time_ms: u128,
}

pub struct DeviceOutput {
    stream: Option<Stream>,
    device: Device,
    config: StreamConfig,
    format: SampleFormat,
    producer: OutputProducer,
    stream_errors: Arc<AtomicU64>,
    report: DeviceReport,
    negotiated: NegotiatedConfigReport,
    active: bool,
}

impl DeviceOutput {
    pub fn open(device_id: Option<&str>, ring_capacity_frames: usize) -> Result<Self, PlayerError> {
        if ring_capacity_frames == 0 {
            return Err(PlayerError::new(
                ErrorCode::ValueOutOfRange,
                "Output ring capacity must be positive",
            ));
        }
        let entry = select_device(device_entries()?, device_id)?;
        let config = negotiate(&entry, None)?;
        let format = config.sample_format();
        let stream_config = StreamConfig {
            channels: config.channels(),
            sample_rate: config.sample_rate(),
            buffer_size: BufferSize::Default,
        };
        let (producer, consumer) = output_ring(ring_capacity_frames)?;
        let stream_errors = Arc::new(AtomicU64::new(0));
        let stream = build_stream(
            &entry.device,
            &stream_config,
            format,
            consumer,
            Arc::clone(&stream_errors),
            None,
        )?;
        Ok(Self {
            stream: Some(stream),
            device: entry.device,
            config: stream_config,
            format,
            producer,
            stream_errors,
            report: entry.report,
            negotiated: NegotiatedConfigReport {
                channels: stream_config.channels,
                sample_rate: stream_config.sample_rate,
                sample_format: format.to_string(),
                buffer_size: "default".to_owned(),
            },
            active: false,
        })
    }

    pub fn play(&mut self) -> Result<(), PlayerError> {
        if !self.active {
            self.stream
                .as_ref()
                .ok_or_else(stream_closed)?
                .play()
                .map_err(|error| output_error("Audio stream could not start", error))?;
            self.active = true;
        }
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), PlayerError> {
        if self.active {
            self.stream
                .as_ref()
                .ok_or_else(stream_closed)?
                .pause()
                .map_err(|error| output_error("Audio stream could not pause", error))?;
            self.active = false;
        }
        Ok(())
    }

    pub fn clear(&mut self, timeout: Duration) -> Result<(), PlayerError> {
        let generation = self.producer.generation().checked_add(1).ok_or_else(|| {
            PlayerError::new(ErrorCode::ValueOutOfRange, "Output generation is exhausted")
        })?;
        self.active = false;
        drop(self.stream.take());
        let (producer, consumer) =
            output_ring_at_generation(self.producer.metrics().capacity_frames, generation)?;
        self.producer = producer;
        self.stream_errors.store(0, Ordering::Relaxed);
        match build_stream(
            &self.device,
            &self.config,
            self.format,
            consumer,
            Arc::clone(&self.stream_errors),
            Some(timeout),
        ) {
            Ok(stream) => {
                self.stream = Some(stream);
                Ok(())
            }
            Err(error) => {
                self.stream_errors.fetch_add(1, Ordering::Relaxed);
                Err(error)
            }
        }
    }

    pub fn begin_drain(&self) {
        self.producer.begin_drain();
    }

    pub fn push(&mut self, frames: &[StereoFrame]) -> usize {
        self.producer.push_slice(frames)
    }

    pub fn available_frames(&self) -> usize {
        self.producer.available()
    }

    pub fn queued_frames(&self) -> usize {
        self.producer.occupancy()
    }

    pub fn metrics(&self) -> OutputMetrics {
        self.producer.metrics()
    }

    pub fn presentation_estimate(&self) -> PresentationEstimate {
        let metrics = self.metrics();
        let submission = if self.active {
            self.producer.presentation_submission()
        } else {
            None
        };
        let now_ns = self
            .stream
            .as_ref()
            .map_or(0, |stream| stream.now().as_nanos());
        PresentationEstimate::new(metrics, submission, now_ns)
    }

    pub fn estimated_drain_complete(
        &self,
        elapsed: Duration,
        timeout: Duration,
    ) -> Result<bool, PlayerError> {
        if self.stream_errors() > 0 {
            return Err(PlayerError::new(
                ErrorCode::PlaybackNotReady,
                "Audio output failed while draining",
            ));
        }
        self.presentation_estimate()
            .drain_complete(elapsed, timeout)
    }

    pub fn refill_interval(&self, maximum_render_frames: usize) -> Result<Duration, PlayerError> {
        output_refill_interval(
            self.producer.metrics().capacity_frames,
            maximum_render_frames,
            self.config.sample_rate,
        )
    }

    pub fn stream_errors(&self) -> u64 {
        self.stream_errors.load(Ordering::Relaxed)
    }

    pub fn device_name(&self) -> &str {
        &self.report.name
    }

    pub const fn negotiated(&self) -> &NegotiatedConfigReport {
        &self.negotiated
    }

    pub const fn is_active(&self) -> bool {
        self.active
    }
}

struct DeviceEntry {
    report: DeviceReport,
    device: Device,
    ranges: Vec<SupportedStreamConfigRange>,
    default: Option<SupportedStreamConfig>,
}

pub fn enumerate_devices() -> Result<Vec<DeviceReport>, PlayerError> {
    Ok(device_entries()?
        .into_iter()
        .map(|entry| entry.report)
        .collect())
}

pub fn run_output_test(options: OutputTestOptions) -> Result<OutputTestReport, PlayerError> {
    if options.seconds == 0 || options.seconds > 86_400 {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Output duration must be between 1 and 86400 seconds",
        ));
    }
    if !options.amplitude.is_finite() || !(0.0..=1.0).contains(&options.amplitude) {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Synthetic amplitude must be finite and between zero and one",
        ));
    }
    if options.pause_cycles > 100 {
        return Err(PlayerError::new(
            ErrorCode::ValueOutOfRange,
            "Pause cycles must not exceed 100",
        ));
    }
    let entries = device_entries()?;
    let entry = select_device(entries, options.device_id.as_deref())?;
    let config = negotiate(&entry, options.sample_rate)?;
    let format = config.sample_format();
    let stream_config = StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size: BufferSize::Default,
    };
    let source_frames = u64::from(stream_config.sample_rate).saturating_mul(options.seconds);
    let render_frames = source_frames.saturating_add(options.ring_capacity_frames as u64);
    let (mut producer, consumer) = output_ring(options.ring_capacity_frames)?;
    let prefill = options
        .ring_capacity_frames
        .min(usize::try_from(render_frames).unwrap_or(usize::MAX));
    let mut next_frame = 0_u64;
    while next_frame < prefill as u64 {
        let frame = sine_frame(next_frame, stream_config.sample_rate, options.amplitude);
        if !producer.push(frame) {
            break;
        }
        next_frame += 1;
    }

    let stream_errors = Arc::new(AtomicU64::new(0));
    let stream = build_stream(
        &entry.device,
        &stream_config,
        format,
        consumer,
        Arc::clone(&stream_errors),
        None,
    )?;
    stream
        .play()
        .map_err(|error| output_error("Audio stream could not start", error))?;

    let stop = Arc::new(AtomicBool::new(false));
    let render_stop = Arc::clone(&stop);
    let sample_rate = stream_config.sample_rate;
    let amplitude = options.amplitude;
    let render = thread::spawn(move || {
        render_synthetic(
            producer,
            next_frame,
            source_frames,
            render_frames,
            sample_rate,
            amplitude,
            render_stop,
        )
    });
    run_schedule(&stream, options.seconds, options.pause_cycles)?;

    let shutdown_started = Instant::now();
    stop.store(true, Ordering::Release);
    let _ = stream.pause();
    drop(stream);
    let producer = render
        .join()
        .map_err(|_| PlayerError::new(ErrorCode::Internal, "Synthetic render worker terminated"))?;
    let shutdown_time_ms = shutdown_started.elapsed().as_millis();

    Ok(OutputTestReport {
        schema: "stemplayer-output-test-v1",
        device_id: entry.report.id,
        device_name: entry.report.name,
        negotiated: NegotiatedConfigReport {
            channels: stream_config.channels,
            sample_rate: stream_config.sample_rate,
            sample_format: format.to_string(),
            buffer_size: "default".to_owned(),
        },
        duration_seconds: options.seconds,
        pause_cycles: options.pause_cycles,
        metrics: producer.metrics(),
        stream_errors: stream_errors.load(Ordering::Relaxed),
        shutdown_time_ms,
    })
}

fn run_schedule(stream: &Stream, seconds: u64, pause_cycles: u32) -> Result<(), PlayerError> {
    let steps = u64::from(pause_cycles).saturating_mul(2).saturating_add(1);
    let segment = Duration::from_millis(seconds.saturating_mul(1_000) / steps);
    for _ in 0..pause_cycles {
        thread::sleep(segment);
        stream
            .pause()
            .map_err(|error| output_error("Audio stream could not pause", error))?;
        thread::sleep(segment);
        stream
            .play()
            .map_err(|error| output_error("Audio stream could not resume", error))?;
    }
    thread::sleep(segment);
    Ok(())
}

fn device_entries() -> Result<Vec<DeviceEntry>, PlayerError> {
    let host = cpal::default_host();
    let default_id = host
        .default_output_device()
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string());
    let devices = host
        .output_devices()
        .map_err(|error| output_error("Output devices could not be enumerated", error))?;
    let mut entries = Vec::new();
    for device in devices {
        let hardware_id = match device.id() {
            Ok(id) => id.to_string(),
            Err(_) => continue,
        };
        let ranges = match device.supported_output_configs() {
            Ok(configs) => configs.collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        let default = device.default_output_config().ok();
        let name = device
            .description()
            .map(|description| description.name().to_owned())
            .unwrap_or_else(|_| "Unnamed output device".to_owned());
        let report = DeviceReport {
            id: endpoint_id(&hardware_id),
            name,
            is_default: default_id.as_ref() == Some(&hardware_id),
            default_configuration: default.as_ref().map(config_report),
            supported_configurations: ranges.iter().map(range_report).collect(),
            stereo_supported: ranges.iter().any(|range| range.channels() == 2),
        };
        entries.push(DeviceEntry {
            report,
            device,
            ranges,
            default,
        });
    }
    Ok(entries)
}

fn select_device(
    entries: Vec<DeviceEntry>,
    selected_id: Option<&str>,
) -> Result<DeviceEntry, PlayerError> {
    let index = selected_index(entries.iter().map(|entry| &entry.report), selected_id);
    let selected = index.and_then(|index| entries.into_iter().nth(index));
    selected
        .filter(|entry| entry.report.stereo_supported)
        .ok_or_else(|| {
            PlayerError::new(
                ErrorCode::FeatureUnsupported,
                "No safe stereo output configuration is available",
            )
            .with_user_action("Choose a stereo-capable output device")
        })
}

fn negotiate(
    entry: &DeviceEntry,
    requested_rate: Option<u32>,
) -> Result<SupportedStreamConfig, PlayerError> {
    let default_rate = entry
        .default
        .as_ref()
        .map(SupportedStreamConfig::sample_rate);
    entry
        .ranges
        .iter()
        .filter(|range| range.channels() == 2)
        .filter(|range| supported_format(range.sample_format()))
        .filter_map(|range| {
            let rate = requested_rate
                .filter(|rate| range.contains_rate(*rate))
                .or_else(|| default_rate.filter(|rate| range.contains_rate(*rate)))
                .or_else(|| {
                    [48_000, 44_100]
                        .into_iter()
                        .find(|rate| range.contains_rate(*rate))
                })
                .unwrap_or(range.min_sample_rate());
            let score = (
                usize::from(requested_rate != Some(rate)),
                usize::from(default_rate != Some(rate)),
                format_rank(range.sample_format()),
                rate,
            );
            (*range)
                .try_with_sample_rate(rate)
                .map(|config| (score, config))
        })
        .min_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, config)| config)
        .ok_or_else(|| {
            PlayerError::new(
                ErrorCode::FeatureUnsupported,
                "The selected device has no supported stereo sample format",
            )
        })
}

fn supported_format(format: SampleFormat) -> bool {
    matches!(
        format,
        SampleFormat::F32
            | SampleFormat::F64
            | SampleFormat::I16
            | SampleFormat::U16
            | SampleFormat::I32
            | SampleFormat::U32
    )
}

fn format_rank(format: SampleFormat) -> usize {
    match format {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        SampleFormat::U16 => 2,
        SampleFormat::I32 => 3,
        SampleFormat::U32 => 4,
        SampleFormat::F64 => 5,
        _ => usize::MAX,
    }
}

fn range_report(config: &SupportedStreamConfigRange) -> ConfigReport {
    ConfigReport {
        channels: config.channels(),
        minimum_sample_rate: config.min_sample_rate(),
        maximum_sample_rate: config.max_sample_rate(),
        sample_format: config.sample_format().to_string(),
        buffer_size: buffer_size_report(config.buffer_size()),
    }
}

fn config_report(config: &SupportedStreamConfig) -> ConfigReport {
    ConfigReport {
        channels: config.channels(),
        minimum_sample_rate: config.sample_rate(),
        maximum_sample_rate: config.sample_rate(),
        sample_format: config.sample_format().to_string(),
        buffer_size: buffer_size_report(config.buffer_size()),
    }
}

fn buffer_size_report(buffer: &SupportedBufferSize) -> String {
    match buffer {
        SupportedBufferSize::Range { min, max } => format!("{min}-{max} frames"),
        SupportedBufferSize::Unknown => "unknown".to_owned(),
    }
}

fn build_stream(
    device: &Device,
    config: &StreamConfig,
    format: SampleFormat,
    consumer: OutputConsumer,
    stream_errors: Arc<AtomicU64>,
    timeout: Option<Duration>,
) -> Result<Stream, PlayerError> {
    match format {
        SampleFormat::F32 => {
            build_typed_stream::<f32>(device, config, consumer, stream_errors, timeout)
        }
        SampleFormat::F64 => {
            build_typed_stream::<f64>(device, config, consumer, stream_errors, timeout)
        }
        SampleFormat::I16 => {
            build_typed_stream::<i16>(device, config, consumer, stream_errors, timeout)
        }
        SampleFormat::U16 => {
            build_typed_stream::<u16>(device, config, consumer, stream_errors, timeout)
        }
        SampleFormat::I32 => {
            build_typed_stream::<i32>(device, config, consumer, stream_errors, timeout)
        }
        SampleFormat::U32 => {
            build_typed_stream::<u32>(device, config, consumer, stream_errors, timeout)
        }
        _ => Err(PlayerError::new(
            ErrorCode::FeatureUnsupported,
            "The negotiated device sample format is unsupported",
        )),
    }
}

fn build_typed_stream<T>(
    device: &Device,
    config: &StreamConfig,
    mut consumer: OutputConsumer,
    stream_errors: Arc<AtomicU64>,
    timeout: Option<Duration>,
) -> Result<Stream, PlayerError>
where
    T: SizedSample + OutputSample,
{
    let channels = usize::from(config.channels);
    let sample_rate = config.sample_rate;
    device
        .build_output_stream(
            *config,
            move |output: &mut [T], info| {
                let started = Instant::now();
                let frames = output.len() / channels;
                consumer.write(output, channels);
                consumer.record_presentation(info.timestamp().playback.as_nanos(), sample_rate);
                consumer.record_callback(frames, started.elapsed());
            },
            move |_| {
                stream_errors.fetch_add(1, Ordering::Relaxed);
            },
            timeout,
        )
        .map_err(|error| output_error("Audio stream could not be created", error))
}

fn render_synthetic(
    mut producer: OutputProducer,
    mut frame_index: u64,
    source_frames: u64,
    render_frames: u64,
    sample_rate: u32,
    amplitude: f32,
    stop: Arc<AtomicBool>,
) -> OutputProducer {
    while frame_index < render_frames && !stop.load(Ordering::Acquire) {
        if producer.available() == 0 {
            thread::sleep(Duration::from_millis(1));
            continue;
        }
        let frame = if frame_index < source_frames {
            sine_frame(frame_index, sample_rate, amplitude)
        } else {
            StereoFrame::default()
        };
        if producer.push(frame) {
            frame_index += 1;
        }
    }
    producer
}

fn sine_frame(frame_index: u64, sample_rate: u32, amplitude: f32) -> StereoFrame {
    let phase = (frame_index % u64::from(sample_rate)) as f32 * 440.0 * TAU / sample_rate as f32;
    let sample = phase.sin() * amplitude;
    StereoFrame {
        left: sample,
        right: sample,
    }
}

fn output_error(
    summary: &str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> PlayerError {
    PlayerError::new(ErrorCode::Internal, summary).with_source(source)
}

fn stream_closed() -> PlayerError {
    PlayerError::new(ErrorCode::PlaybackNotReady, "Audio output must be reopened")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_output_capacity_is_checked_before_device_access() {
        let error = match DeviceOutput::open(None, 0) {
            Ok(_) => panic!("zero capacity was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.code, ErrorCode::ValueOutOfRange);
    }

    #[test]
    fn amplitude_is_checked_before_device_access() {
        let options = OutputTestOptions {
            amplitude: f32::NAN,
            ..OutputTestOptions::default()
        };
        assert!(run_output_test(options).is_err());
    }

    #[test]
    fn pause_cycle_limit_is_checked_before_device_access() {
        let options = OutputTestOptions {
            pause_cycles: 101,
            ..OutputTestOptions::default()
        };
        assert!(run_output_test(options).is_err());
    }

    #[test]
    fn format_policy_prefers_float_then_common_integers() {
        assert_eq!(format_rank(SampleFormat::F32), 0);
        assert!(format_rank(SampleFormat::I16) < format_rank(SampleFormat::F64));
    }

    #[test]
    fn synthetic_source_is_stereo_and_bounded() {
        let frame = sine_frame(100, 48_000, 0.1);
        assert_eq!(frame.left, frame.right);
        assert!(frame.left.abs() <= 0.1);
    }
}
