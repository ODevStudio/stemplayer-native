use sha2::{Digest, Sha256};
use stemplayer_domain::{GenerationId, SampleRate, SourceFrame};
use stemplayer_waveform::{CacheConfig, SourceFingerprint, WaveformCache};

#[path = "../../../tools/test-support/scratch.rs"]
mod scratch;
use scratch::Scratch;

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn hostile_cache_records_fail_closed_without_panics_or_large_allocations() {
    let fixture = Scratch::new().unwrap();
    let cache = WaveformCache::new(
        fixture.path().to_path_buf(),
        CacheConfig {
            enabled: true,
            capacity_bytes: 4 * 1024 * 1024,
            maximum_entries: 16,
        },
    )
    .unwrap();
    let fingerprint = SourceFingerprint::from_bytes([0x5a; 32]);
    let name = format!("{}.spwf", fingerprint.hex());

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
        fixture.write(&name, &record).unwrap();
        let _ = cache.load(
            fingerprint,
            GenerationId::INITIAL,
            SampleRate::new(48_000).unwrap(),
            SourceFrame::new(0)..SourceFrame::new(257),
        );
    }
}
