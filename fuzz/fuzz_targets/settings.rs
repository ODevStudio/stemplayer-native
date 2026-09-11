#![no_main]

use libfuzzer_sys::fuzz_target;
use std::fs;
use stemplayer_platform::load_settings;

const MAX_INPUT: usize = 128 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let path = std::env::temp_dir().join(format!(
        "stemplayer-fuzz-settings-{}.json",
        std::process::id()
    ));
    if fs::write(&path, data).is_ok() {
        let _ = load_settings(&path);
    }
});
