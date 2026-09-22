use stemplayer_platform::load_settings;

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
fn corrupt_and_oversized_settings_always_recover_without_panics() {
    let scratch = Scratch::new().unwrap();
    let seed = br#"{"schema_version":4,"diagnostics_visible":false,"recent_sources_enabled":true,"recent_sources":[],"playlist_sources":[],"theme":0,"waveform_cache_enabled":false,"waveform_cache_limit_mb":256}"#;
    for round in 0..512_u64 {
        let mut state = 0xa076_1d64_78bd_642f ^ round.wrapping_mul(0xe703_7ed1_a0b4_28db);
        let mut bytes = seed.to_vec();
        match next(&mut state) % 4 {
            0 if !bytes.is_empty() => {
                let index = next(&mut state) as usize % bytes.len();
                bytes[index] ^= (next(&mut state) as u8) | 1;
            }
            1 => {
                let keep = next(&mut state) as usize % (bytes.len() + 1);
                bytes.truncate(keep);
            }
            2 => bytes.extend((0..128).map(|_| next(&mut state) as u8)),
            _ => {
                let count = next(&mut state) as usize % 4096;
                bytes = (0..count).map(|_| next(&mut state) as u8).collect();
            }
        }
        let path = scratch.write("settings.json", &bytes).unwrap();
        let loaded = load_settings(&path);
        assert_eq!(loaded.settings().schema_version, 4);
    }

    let path = scratch
        .write("settings.json", &vec![b'x'; 64 * 1024 + 1])
        .unwrap();
    let loaded = load_settings(&path);
    assert_eq!(loaded.settings().schema_version, 4);
}
