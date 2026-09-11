#![no_main]

use libfuzzer_sys::fuzz_target;
use stemplayer_format::{parse_manifest, safe_basename};

fuzz_target!(|data: &[u8]| {
    let _ = parse_manifest(data);
    if data.len() <= 4096 {
        if let Ok(text) = std::str::from_utf8(data) {
            let _ = safe_basename(text);
        }
    }
});
