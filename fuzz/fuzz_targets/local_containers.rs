#![no_main]

use libfuzzer_sys::fuzz_target;
use std::sync::{OnceLock, atomic::AtomicBool};
use stemplayer_format::{BoundedReader, inspect_mp4_structure, inspect_wave};

#[path = "../../tools/test-support/scratch.rs"]
mod scratch;

const MAX_INPUT: usize = 2 * 1024 * 1024;
static SCRATCH: OnceLock<scratch::Scratch> = OnceLock::new();

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let scratch = SCRATCH.get_or_init(|| scratch::Scratch::new().expect("private fuzz directory"));
    let path = scratch.write("input.bin", data).expect("fuzz input write");
    let cancelled = AtomicBool::new(false);
    if let Ok(reader) = BoundedReader::open(&path) {
        let _ = inspect_wave(reader, &cancelled);
    }
    if let Ok(reader) = BoundedReader::open(&path) {
        let _ = inspect_mp4_structure(reader, &cancelled);
    }
});
