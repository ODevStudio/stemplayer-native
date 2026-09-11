#![no_main]

use libfuzzer_sys::fuzz_target;
use std::fs;
use std::sync::atomic::AtomicBool;
use stemplayer_format::{inspect_mp4_structure, inspect_wave, BoundedReader};

const MAX_INPUT: usize = 2 * 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let path = std::env::temp_dir().join(format!(
        "stemplayer-fuzz-local-containers-{}.bin",
        std::process::id()
    ));
    if fs::write(&path, data).is_err() {
        return;
    }
    let cancelled = AtomicBool::new(false);
    if let Ok(reader) = BoundedReader::open(&path) {
        let _ = inspect_wave(reader, &cancelled);
    }
    if let Ok(reader) = BoundedReader::open(&path) {
        let _ = inspect_mp4_structure(reader, &cancelled);
    }
});
