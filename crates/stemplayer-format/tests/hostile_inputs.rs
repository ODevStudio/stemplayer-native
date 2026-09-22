use std::sync::atomic::AtomicBool;
use stemplayer_format::{BoundedReader, inspect_mp4_structure, inspect_wave, parse_manifest};

#[path = "../../../tools/test-support/scratch.rs"]
mod scratch;
use scratch::Scratch;

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn mutated(seed: &[u8], round: u64) -> Vec<u8> {
    let mut state = 0x9e37_79b9_7f4a_7c15 ^ round.wrapping_mul(0xa076_1d64_78bd_642f);
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
            let count = (next(&mut state) as usize % 16) + 1;
            let fill = next(&mut state) as u8;
            bytes.splice(index..index, std::iter::repeat_n(fill, count));
        }
        3 => {
            bytes.extend((0..32).map(|_| next(&mut state) as u8));
        }
        _ => {
            if bytes.len() >= 8 {
                let index = next(&mut state) as usize % (bytes.len() - 3);
                bytes[index..index + 4].copy_from_slice(&(next(&mut state) as u32).to_le_bytes());
            } else {
                bytes.extend((0..8).map(|_| next(&mut state) as u8));
            }
        }
    }
    bytes.truncate(16 * 1024);
    bytes
}

fn valid_wav() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&40_u32.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&44_100_u32.to_le_bytes());
    bytes.extend_from_slice(&176_400_u32.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes
}

fn mp4_seed() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&24_u32.to_be_bytes());
    bytes.extend_from_slice(b"ftypM4A \0\0\0\0M4A isom");
    bytes.extend_from_slice(&8_u32.to_be_bytes());
    bytes.extend_from_slice(b"moov");
    bytes
}

#[test]
fn hostile_manifest_mutations_never_escape_the_parser_contract() {
    let seed = br#"{"schema_version":1,"outputs":[{"logical_type":"drums","filename":"drums.wav"},{"logical_type":"bass","filename":"bass.wav"},{"logical_type":"other","filename":"other.wav"},{"logical_type":"vocals","filename":"vocals.wav"}]}"#;
    assert!(parse_manifest(seed).is_ok());
    for round in 0..512 {
        let bytes = mutated(seed, round);
        let _ = parse_manifest(&bytes);
    }
}

#[test]
fn hostile_wav_and_mp4_mutations_never_panic_or_read_unbounded_ranges() {
    let scratch = Scratch::new().unwrap();
    let cancelled = AtomicBool::new(false);
    let path = scratch.write("input.bin", &valid_wav()).unwrap();
    assert!(inspect_wave(BoundedReader::open(&path).unwrap(), &cancelled).is_ok());

    for seed in [valid_wav(), mp4_seed()] {
        for round in 0..384 {
            let bytes = mutated(&seed, round);
            scratch.write("input.bin", &bytes).unwrap();
            if let Ok(reader) = BoundedReader::open(&path) {
                let _ = inspect_wave(reader, &cancelled);
            }
            if let Ok(reader) = BoundedReader::open(&path) {
                let _ = inspect_mp4_structure(reader, &cancelled);
            }
        }
    }
}
