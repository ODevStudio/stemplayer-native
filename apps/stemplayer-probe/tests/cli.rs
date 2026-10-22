use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

fn probe(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_stemplayer-probe"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn version_and_config_are_machine_readable() {
    let version = probe(&["version"]);
    assert!(version.status.success());
    let value: Value = serde_json::from_slice(&version.stdout).unwrap();
    assert_eq!(value["schema"], "stemplayer-probe-v1");

    let config = probe(&["validate-config"]);
    assert!(config.status.success());
    let value: Value = serde_json::from_slice(&config.stdout).unwrap();
    assert_eq!(value["status"], "valid");
    assert!(
        value["features"]
            .as_array()
            .unwrap()
            .contains(&Value::from("local-wav"))
    );
}

#[test]
fn callback_audit_is_machine_readable() {
    let output = probe(&["callback-audit"]);
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], "stemplayer-callback-audit-v1");
}

#[test]
fn studio_commands_inspect_validate_and_decode_bounded_windows() {
    let dir = fixture();
    let folder = dir.to_str().unwrap();
    let inspect = probe(&["inspect", folder]);
    assert!(
        inspect.status.success(),
        "{}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    let value: Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(value["schema"], "stemplayer-studio-bundle-v1");
    assert_eq!(value["frames"], 4);
    assert_eq!(value["maximum_open_files"], 4);

    let manifest = dir.join("manifest.json");
    let validate = probe(&["validate", manifest.to_str().unwrap()]);
    assert!(validate.status.success());

    let decode = probe(&[
        "decode-window",
        folder,
        "--start-frame",
        "1",
        "--frames",
        "2",
    ]);
    assert!(
        decode.status.success(),
        "{}",
        String::from_utf8_lossy(&decode.stderr)
    );
    let value: Value = serde_json::from_slice(&decode.stdout).unwrap();
    assert_eq!(value["schema"], "stemplayer-decode-window-v1");
    assert_eq!(value["windows"].as_array().unwrap().len(), 4);
    assert_eq!(value["windows"][0]["frames"], 2);
    assert_eq!(value["windows"][0]["bytes_read"], 8);

    let playback = probe(&["playback-sim", folder]);
    assert!(
        playback.status.success(),
        "{}",
        String::from_utf8_lossy(&playback.stderr)
    );
    let value: Value = serde_json::from_slice(&playback.stdout).unwrap();
    assert_eq!(value["schema"], "stemplayer-playback-sim-v1");
    assert_eq!(value["state"], "ENDED");
    assert_eq!(value["rendered_frames"], 4);
    assert_eq!(value["end_frame_exclusive"], 4);
    assert_eq!(value["ring_overflows"], 0);
    assert_eq!(value["maximum_pending_blocks"], 1);

    let resampled = probe(&["playback-sim", folder, "--output-rate", "96000"]);
    assert!(
        resampled.status.success(),
        "{}",
        String::from_utf8_lossy(&resampled.stderr)
    );
    let value: Value = serde_json::from_slice(&resampled.stdout).unwrap();
    assert_eq!(value["source_rate"], 48_000);
    assert_eq!(value["output_rate"], 96_000);
    assert_eq!(value["rendered_frames"], 8);
    assert_eq!(value["resampler_input_frames"], 4);
    assert_eq!(value["resampler_output_frames"], 8);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn later_phase_command_surface_has_stable_unsupported_errors() {
    for arguments in [
        &["render", "fixture", "output.wav"][..],
        &["benchmark", "fixture"][..],
        &["cache-inspect", "cache"][..],
    ] {
        let output = probe(arguments);
        assert_eq!(output.status.code(), Some(4));
        let value: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(value["code"], "E_PLAYBACK_FEATURE_UNSUPPORTED");
    }
    #[cfg(not(feature = "hls"))]
    {
        let output = probe(&["hls-validate", "https://example.invalid/stemset.json"]);
        assert_eq!(output.status.code(), Some(4));
        let value: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(value["code"], "E_PLAYBACK_FEATURE_UNSUPPORTED");
    }
}

#[test]
fn usage_validation_and_output_order_are_deterministic() {
    let usage = probe(&["decode-window", "source", "--frames", "2"]);
    assert_eq!(usage.status.code(), Some(2));

    let validation = probe(&["inspect", ""]);
    assert_eq!(validation.status.code(), Some(3));
    let output_test_validation = probe(&["output-test", "--seconds", "0"]);
    assert_eq!(output_test_validation.status.code(), Some(3));
    let oversized = probe(&[
        "decode-window",
        "source",
        "--start-frame",
        "0",
        "--frames",
        "4097",
    ]);
    assert_eq!(oversized.status.code(), Some(3));

    let first = probe(&["version"]);
    let second = probe(&["version"]);
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stdout.ends_with(b"\n"));
}

#[test]
fn container_inspection_rejects_content_instead_of_trusting_the_extension() {
    let path = std::env::temp_dir().join(format!(
        "stemplayer-invalid-{}.stem.m4a",
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, b"\0\0\0\x08free").unwrap();
    let output = probe(&["container-inspect", path.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(3));
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["code"], "E_CONTAINER_NOT_MP4");
    assert_eq!(value["recoverability"], "choose-another-source");
    fs::remove_file(path).unwrap();
}

fn fixture() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "stemplayer-probe-phase4-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    for role in ["drums", "bass", "other", "vocals"] {
        fs::write(path.join(format!("{role}.wav")), wave()).unwrap();
    }
    fs::write(
        path.join("manifest.json"),
        r#"{"schema_version":2,"outputs":[
            {"logical_type":"drums","filename":"drums.wav"},
            {"logical_type":"bass","filename":"bass.wav"},
            {"logical_type":"other","filename":"other.wav"},
            {"logical_type":"vocals","filename":"vocals.wav"}]}"#,
    )
    .unwrap();
    path
}

fn wave() -> Vec<u8> {
    let samples = [i16::MIN, 0, 1, -1, i16::MAX, 2, 3, 4];
    let mut data = Vec::new();
    for sample in samples {
        data.extend_from_slice(&sample.to_le_bytes());
    }
    let mut body = Vec::new();
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&48_000_u32.to_le_bytes());
    body.extend_from_slice(&192_000_u32.to_le_bytes());
    body.extend_from_slice(&4_u16.to_le_bytes());
    body.extend_from_slice(&16_u16.to_le_bytes());
    body.extend_from_slice(b"data");
    body.extend_from_slice(&(data.len() as u32).to_le_bytes());
    body.extend_from_slice(&data);
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(&body);
    wav
}
