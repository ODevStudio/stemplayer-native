#![no_main]

use libfuzzer_sys::fuzz_target;
use url::Url;

#[path = "../../crates/stemplayer-network/src/url_policy.rs"]
mod url_policy;

fuzz_target!(|data: &[u8]| {
    if data.len() > 4096 {
        return;
    }
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = url_policy::entry_url(text, false);
    let _ = url_policy::entry_url(text, true);
    let base = Url::parse("https://example.test/a/stemset.json").expect("static URL");
    let _ = url_policy::child_url(&base, text);
});
