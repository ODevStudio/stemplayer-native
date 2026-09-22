#![no_main]

use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;
use stemplayer_platform::load_settings;

#[path = "../../tools/test-support/scratch.rs"]
mod scratch;

const MAX_INPUT: usize = 128 * 1024;
static SCRATCH: OnceLock<scratch::Scratch> = OnceLock::new();

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let scratch = SCRATCH.get_or_init(|| scratch::Scratch::new().expect("private fuzz directory"));
    let path = scratch
        .write("settings.json", data)
        .expect("fuzz input write");
    let _ = load_settings(&path);
});
