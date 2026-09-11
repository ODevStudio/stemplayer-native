#![no_main]

use libfuzzer_sys::fuzz_target;
use stemplayer_format::{parse_manifest, safe_basename, MAX_MANIFEST_BYTES};

fuzz_target!(|data: &[u8]| {
    if data.len() <= MAX_MANIFEST_BYTES {
        let _ = parse_manifest(data);
    }
    if data.len() <= 4096 {
        if let Ok(text) = std::str::from_utf8(data) {
            let _ = safe_basename(text);
        }
    }
});
