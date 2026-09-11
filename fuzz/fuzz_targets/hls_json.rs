#![no_main]

use libfuzzer_sys::fuzz_target;
use stemplayer_network::{HlsManifest, HlsWaveform};
use url::Url;

const MAX_INPUT: usize = 2 * 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let base = Url::parse("https://example.test/stemset/stemset.json").expect("static URL");
    let _ = HlsManifest::parse(data, &base);
    let _ = HlsWaveform::parse(data);
});
