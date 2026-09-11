#![no_main]

use libfuzzer_sys::fuzz_target;
use stemplayer_network::MediaPlaylist;
use url::Url;

const MAX_INPUT: usize = 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let url = Url::parse("https://example.test/stemset/drums.m3u8").expect("static URL");
    for sample_rate in [44_100_u32, 48_000] {
        for preroll in [0_usize, 1024] {
            let _ = MediaPlaylist::parse(data, &url, sample_rate, preroll);
        }
    }
});
