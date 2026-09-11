#![cfg(feature = "hls")]

use stemplayer_network::{HlsManifest, HlsWaveform, MediaPlaylist};
use url::Url;

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn mutated(seed: &[u8], round: u64) -> Vec<u8> {
    let mut state = 0xd1b5_4a32_d192_ed03 ^ round.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut bytes = seed.to_vec();
    match next(&mut state) % 5 {
        0 if !bytes.is_empty() => {
            let index = next(&mut state) as usize % bytes.len();
            bytes[index] ^= (next(&mut state) as u8) | 1;
        }
        1 => {
            let keep = if bytes.is_empty() {
                0
            } else {
                next(&mut state) as usize % (bytes.len() + 1)
            };
            bytes.truncate(keep);
        }
        2 => {
            let index = if bytes.is_empty() {
                0
            } else {
                next(&mut state) as usize % (bytes.len() + 1)
            };
            let count = (next(&mut state) as usize % 24) + 1;
            bytes.splice(index..index, std::iter::repeat_n(next(&mut state) as u8, count));
        }
        3 => bytes.extend((0..48).map(|_| next(&mut state) as u8)),
        _ => {
            for _ in 0..4 {
                if bytes.is_empty() {
                    break;
                }
                let index = next(&mut state) as usize % bytes.len();
                bytes[index] = next(&mut state) as u8;
            }
        }
    }
    bytes.truncate(64 * 1024);
    bytes
}

fn manifest_seed() -> &'static [u8] {
    br#"{"schema":"stem-hls-v1","manifest_version":1,"format_version":"stem-hls-v1","title":"Synthetic","duration_us":1000000,"sample_rate":48000,"channels":2,"codec":{"name":"AAC-LC","codec_string":"mp4a.40.2","segment_format":"M4A","bitrate_bps":128000},"segment_duration_us":1000000,"segment_count":1,"master_present":false,"master":null,"stems":{"drums":{"playlist":"drums.m3u8","waveform":"drums.json"},"bass":{"playlist":"bass.m3u8","waveform":"bass.json"},"other":{"playlist":"other.m3u8","waveform":"other.json"},"vocals":{"playlist":"vocals.m3u8","waveform":"vocals.json"}},"technical":{"segment_preroll_frames":0}}"#
}

#[test]
fn hostile_hls_json_mutations_fail_closed_without_panics() {
    let base = Url::parse("https://example.test/stemset/stemset.json").unwrap();
    assert!(HlsManifest::parse(manifest_seed(), &base).is_ok());
    let waveform = br#"{"schema":"stem-waveform-v1","samples_per_peak":256,"min":[-0.5,0.0],"max":[0.5,0.25]}"#;
    assert!(HlsWaveform::parse(waveform).is_ok());

    for seed in [manifest_seed(), waveform.as_slice()] {
        for round in 0..512 {
            let bytes = mutated(seed, round);
            let _ = HlsManifest::parse(&bytes, &base);
            let _ = HlsWaveform::parse(&bytes);
        }
    }
}

#[test]
fn hostile_playlist_mutations_fail_closed_without_panics() {
    let url = Url::parse("https://example.test/stemset/drums.m3u8").unwrap();
    let seed = b"#EXTM3U\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:1.000000,\nsegment.m4a\n#EXT-X-ENDLIST\n";
    assert!(MediaPlaylist::parse(seed, &url, 48_000, 0).is_ok());
    for round in 0..768 {
        let bytes = mutated(seed, round);
        for sample_rate in [44_100_u32, 48_000] {
            for preroll in [0_usize, 1024] {
                let _ = MediaPlaylist::parse(&bytes, &url, sample_rate, preroll);
            }
        }
    }
}
