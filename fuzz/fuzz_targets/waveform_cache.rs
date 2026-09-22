#![no_main]

use libfuzzer_sys::fuzz_target;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use stemplayer_domain::{GenerationId, SampleRate, SourceFrame};
use stemplayer_waveform::{CacheConfig, SourceFingerprint, WaveformCache};

#[path = "../../tools/test-support/scratch.rs"]
mod scratch;

const MAX_INPUT: usize = 4 * 1024 * 1024;
static ROOT: OnceLock<scratch::Scratch> = OnceLock::new();
static CACHE: OnceLock<WaveformCache> = OnceLock::new();

fn root() -> &'static scratch::Scratch {
    ROOT.get_or_init(|| scratch::Scratch::new().expect("private fuzz directory"))
}

fn cache() -> &'static WaveformCache {
    CACHE.get_or_init(|| {
        WaveformCache::new(
            root().path().to_path_buf(),
            CacheConfig {
                enabled: true,
                capacity_bytes: 8 * 1024 * 1024,
                maximum_entries: 16,
            },
        )
        .expect("process-scoped fuzz cache")
    })
}

fn exercise(bytes: &[u8], fingerprint: SourceFingerprint) {
    root()
        .write(&format!("{}.spwf", fingerprint.hex()), bytes)
        .expect("fuzz input write");
    let _ = cache().load(
        fingerprint,
        GenerationId::INITIAL,
        SampleRate::new(48_000).expect("static rate"),
        SourceFrame::new(0)..SourceFrame::new(257),
    );
}

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let fingerprint = SourceFingerprint::from_bytes([0x5a; 32]);
    exercise(data, fingerprint);

    if data.len() <= 64 * 1024 {
        let mut checksummed = data.to_vec();
        checksummed.extend_from_slice(&Sha256::digest(data));
        exercise(&checksummed, fingerprint);
    }
});
