use serde_json::Value;
use std::process::Command;

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
}

#[test]
fn phase_two_command_surface_has_stable_unsupported_errors() {
    for arguments in [
        &["inspect", "fixture"][..],
        &["validate", "fixture"][..],
        &["render", "fixture", "output.wav"][..],
        &["playback-sim", "fixture"][..],
        &["benchmark", "fixture"][..],
        &["cache-inspect", "cache"][..],
        &["hls-validate", "https://example.invalid/stemset.json"][..],
    ] {
        let output = probe(arguments);
        assert_eq!(output.status.code(), Some(4));
        let value: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(value["code"], "E_PLAYBACK_FEATURE_UNSUPPORTED");
    }
}

#[test]
fn usage_validation_and_output_order_are_deterministic() {
    let usage = probe(&["render", "source-only"]);
    assert_eq!(usage.status.code(), Some(2));

    let validation = probe(&["inspect", ""]);
    assert_eq!(validation.status.code(), Some(3));

    let first = probe(&["version"]);
    let second = probe(&["version"]);
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stdout.ends_with(b"\n"));
}
