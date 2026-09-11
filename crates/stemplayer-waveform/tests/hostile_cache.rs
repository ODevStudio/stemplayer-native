use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use stemplayer_domain::{GenerationId, SampleRate, SourceFrame};
use stemplayer_waveform::{CacheConfig, SourceFingerprint, WaveformCache};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "stemplayer-hostile-cache-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn hostile_cache_records_fail_closed_without_panics_or_large_allocations() {
    let fixture = Fixture::new();
    let cache = WaveformCache::new(
        fixture.0.clone(),
        CacheConfig {
            enabled: true,
            capacity_bytes: 4 * 1024 * 1024,
            maximum_entries: 16,
        },
    )
    .unwrap();
    let fingerprint = SourceFingerprint::from_bytes([0x5a; 32]);
    let path = fixture.0.join(format!("{}.spwf", fingerprint.hex()));

    for round in 0..512_u64 {
        let mut state = 0x243f_6a88_85a3_08d3 ^ round.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let payload_len = next(&mut state) as usize % 4096;
        let mut record = (0..payload_len)
            .map(|_| next(&mut state) as u8)
            .collect::<Vec<_>>();
        if round % 2 == 0 {
            let checksum = Sha256::digest(&record);
            record.extend_from_slice(&checksum);
        }
        fs::write(&path, &record).unwrap();
        let _ = cache.load(
            fingerprint,
            GenerationId::INITIAL,
            SampleRate::new(48_000).unwrap(),
            SourceFrame::new(0)..SourceFrame::new(257),
        );
    }
}
