use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const REFERENCE_SHA256: &str = "1defc97b148dc21598f8e006d68e12dfb5192deae66b6528cf6360883f0a326d";
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn command(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_stemplayer-support"))
        .args(args)
        .output()
        .expect("support command must launch without a desktop or device")
}

fn json(args: &[&str]) -> Value {
    let output = command(args);
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).expect("versioned JSON report")
}

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "stemplayer-support-test-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn build_identity_is_bounded_and_repeatable() {
    let first = json(&["--build-info"]);
    assert_eq!(first, json(&["--build-info"]));
    assert_eq!(first["schema_version"], 1);
    assert_eq!(first["package"], "stemplayer");
    assert_eq!(first["version"], env!("CARGO_PKG_VERSION"));
    for name in ["cargo_lock_sha256", "toolchain_sha256"] {
        let hash = first[name].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    assert_eq!(first.as_object().unwrap().len(), 12);
    assert!(!first.to_string().contains(env!("CARGO_MANIFEST_DIR")));
}

#[test]
fn report_is_an_allowlist_not_a_settings_or_environment_dump() {
    let report = json(&["--report"]);
    assert_eq!(report["kind"], "local-support-report");
    assert_eq!(report["build"], json(&["--build-info"]));
    assert_eq!(report["qualification"], "not-performed");
    for collected in report["collection"].as_object().unwrap().values() {
        assert_eq!(collected.as_bool(), Some(false));
    }
    assert_eq!(report.as_object().unwrap().len(), 5);
    assert_eq!(report, json(&["--report"]));
}

#[test]
fn benchmark_matches_cross_language_reference_and_has_bounded_work() {
    let one = json(&["--benchmark", "--iterations", "1"]);
    let two = json(&["--benchmark", "--iterations", "2"]);
    for report in [&one, &two] {
        assert_eq!(report["benchmark"]["output_sha256"], REFERENCE_SHA256);
        assert_eq!(report["benchmark"]["sample_mismatches"], 0);
        assert_eq!(report["benchmark"]["ring_overflows"], 0);
        assert_eq!(report["benchmark"]["frames_per_iteration"], 32_768);
        assert_eq!(report["benchmark"]["qualification"], "not-performed");
    }
    assert_eq!(one["benchmark"]["elapsed_ns"].as_array().unwrap().len(), 1);
    assert_eq!(two["benchmark"]["elapsed_ns"].as_array().unwrap().len(), 2);
}

#[test]
fn invalid_or_excessive_requests_fail_without_echoing_arguments() {
    for args in [
        vec![],
        vec!["--unknown"],
        vec!["--benchmark", "--iterations", "0"],
        vec!["--benchmark", "--iterations", "65"],
        vec!["--benchmark", "--iterations", "-1"],
        vec!["--benchmark", "--iterations", "not-a-number"],
        vec!["--report", "--iterations", "1"],
        vec!["--report", "--output"],
        vec!["--benchmark", "--iterations", "1", "--iterations", "2"],
    ] {
        let output = command(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("not-a-number"));
    }
}

#[test]
fn unicode_export_creates_new_files_and_never_overwrites() {
    let directory = Directory::new();
    let path = directory.0.join("report-\u{0394}.json");
    let output = Command::new(env!("CARGO_BIN_EXE_stemplayer-support"))
        .args(["--report", "--output"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    let original = fs::read(&path).unwrap();
    assert!(serde_json::from_slice::<Value>(&original).is_ok());
    let output = Command::new(env!("CARGO_BIN_EXE_stemplayer-support"))
        .args(["--report", "--output"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!String::from_utf8_lossy(&output.stderr).contains(path.to_string_lossy().as_ref()));
}
