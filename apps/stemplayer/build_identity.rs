use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use stemplayer_format::sha256_bytes;

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > 1024 * 1024 {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_owned())
}

fn bounded_identifier(value: String) -> String {
    assert!(
        !value.is_empty()
            && value.len() <= 128
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+')
            }),
        "build identifier must be bounded ASCII"
    );
    value
}

pub fn write() {
    let manifest = env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory");
    let manifest = PathBuf::from(manifest);
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .expect("application must be in the workspace");
    let lock = fs::read(root.join("Cargo.lock")).expect("locked workspace is required");
    let toolchain_bytes =
        fs::read(root.join("rust-toolchain.toml")).expect("pinned toolchain is required");
    assert!(lock.len() <= 16 * 1024 * 1024 && toolchain_bytes.len() <= 4096);
    let toolchain_text = std::str::from_utf8(&toolchain_bytes).expect("toolchain must be UTF-8");
    let toolchain = toolchain_text
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "channel").then(|| value.trim().trim_matches('"').to_owned())
        })
        .map(bounded_identifier)
        .expect("toolchain channel must be declared");

    // A source archive must not accidentally inherit the enclosing repository's identity.
    let git_root = git(root, &["rev-parse", "--show-toplevel"]);
    let same_root = git_root
        .as_ref()
        .and_then(|path| fs::canonicalize(path).ok())
        .zip(fs::canonicalize(root).ok())
        .is_some_and(|(actual, expected)| actual == expected);
    let revision = same_root
        .then(|| git(root, &["rev-parse", "HEAD"]))
        .flatten()
        .filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let dirty = same_root
        .then(|| git(root, &["status", "--porcelain=v1", "--untracked-files=normal"]))
        .flatten()
        .map(|status| !status.is_empty());

    // Cover commits, packed refs, worktrees, and edits to any tracked source input.
    if same_root {
        for name in ["HEAD", "index", "packed-refs", "refs"] {
            if let Some(path) = git(root, &["rev-parse", "--git-path", name]) {
                let path = root.join(path);
                if path.exists() {
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }
        }
        if let Some(files) = git(root, &["ls-files", "-z"]) {
            for file in files.split('\0').filter(|file| !file.is_empty()) {
                assert!(!file.contains(['\n', '\r']), "unsupported source filename");
                println!("cargo:rerun-if-changed={}", root.join(file).display());
            }
        }
    }
    for path in [
        "Cargo.lock",
        "Cargo.toml",
        "rust-toolchain.toml",
        "apps",
        "crates",
        "tools",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(path).display());
    }

    let compiler = Command::new(env::var_os("RUSTC").expect("Cargo compiler"))
        .arg("--version")
        .output()
        .expect("compiler identity must be available");
    assert!(compiler.status.success(), "compiler identity failed");
    let compiler = String::from_utf8(compiler.stdout).expect("compiler identity must be UTF-8");
    let compiler = compiler.trim();
    assert!(
        compiler.starts_with("rustc ")
            && compiler.len() <= 128
            && compiler.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b' ' | b'.' | b'(' | b')' | b'_' | b'-')
            }),
        "compiler identity must not contain paths"
    );
    let mut features = env::vars_os()
        .filter_map(|(key, _)| {
            key.to_str()?
                .strip_prefix("CARGO_FEATURE_")
                .map(|feature| feature.to_ascii_lowercase().replace('_', "-"))
        })
        .collect::<Vec<_>>();
    features.sort();
    features.dedup();
    // BTreeMap keeps byte identity stable even if another build dependency enables
    // serde_json/preserve_order. Do not embed timestamps, hostnames, or directories.
    let package = env::var("CARGO_PKG_NAME").expect("package name");
    let version = env::var("CARGO_PKG_VERSION").expect("package version");
    let target = bounded_identifier(env::var("TARGET").expect("Cargo target"));
    let profile = bounded_identifier(env::var("PROFILE").expect("Cargo profile"));
    let info: BTreeMap<&str, Value> = BTreeMap::from([
        ("schema_version", json!(1)),
        ("package", json!(package)),
        ("version", json!(version)),
        ("source_revision", json!(revision)),
        ("source_dirty", json!(dirty)),
        ("rustc", json!(compiler)),
        ("toolchain", json!(toolchain)),
        ("toolchain_sha256", json!(sha256_bytes(&toolchain_bytes))),
        ("cargo_lock_sha256", json!(sha256_bytes(&lock))),
        ("target", json!(target)),
        ("profile", json!(profile)),
        ("features", json!(features)),
    ]);
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::write(
        output.join("build-info.json"),
        serde_json::to_vec(&info).expect("build identity serialization"),
    )
    .expect("build identity must be writable");
}
