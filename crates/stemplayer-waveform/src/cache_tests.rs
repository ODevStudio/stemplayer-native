use super::*;
use crate::PeakAnalyzer;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "cache_boundary_tests.rs"]
mod boundary;

#[cfg(windows)]
#[path = "cache_windows_tests.rs"]
mod windows;

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "stemplayer-waveform-cache-{}-{id}",
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

fn snapshot(fingerprint: SourceFingerprint) -> WaveformSnapshot {
    let mut analyzer = PeakAnalyzer::new(
        GenerationId::INITIAL,
        fingerprint,
        SourceFrame::new(0),
        SourceFrame::new(257),
        SampleRate::new(48_000).unwrap(),
    )
    .unwrap();
    analyzer
        .ingest(SourceFrame::new(0), 257, 8, &vec![0.25; 257 * 8])
        .unwrap();
    analyzer.finish(CacheStatus::Miss).unwrap()
}

fn load(cache: &WaveformCache, fingerprint: SourceFingerprint) -> CacheResult {
    cache.load(
        fingerprint,
        GenerationId::INITIAL,
        SampleRate::new(48_000).unwrap(),
        SourceFrame::new(0)..SourceFrame::new(257),
    )
}

fn ordinary_snapshot(fingerprint: SourceFingerprint) -> WaveformSnapshot {
    let frames = 4_096_usize;
    let mut analyzer = PeakAnalyzer::new(
        GenerationId::INITIAL,
        fingerprint,
        SourceFrame::new(0),
        SourceFrame::new(frames as i64),
        SampleRate::new(48_000).unwrap(),
    )
    .unwrap();
    analyzer.enable_frequency_bands().unwrap();
    analyzer
        .ingest(
            SourceFrame::new(0),
            frames as u32,
            8,
            &vec![0.25; frames * 8],
        )
        .unwrap();
    analyzer.finish(CacheStatus::Miss).unwrap()
}

#[test]
fn stale_temporary_files_are_cleaned_on_first_cache_operation() {
    let fixture = Fixture::new();
    let temporary = fixture.0.join("stale.tmp");
    fs::write(&temporary, b"partial").unwrap();
    let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    assert!(temporary.exists());

    assert_eq!(
        load(&cache, SourceFingerprint::from_bytes([8; 32])).status,
        CacheStatus::Miss
    );
    assert!(!temporary.exists());
}

#[test]
fn miss_write_hit_corruption_and_clear_are_disposable() {
    let fixture = Fixture::new();
    let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    let fingerprint = SourceFingerprint::from_bytes([9; 32]);
    assert_eq!(load(&cache, fingerprint).status, CacheStatus::Miss);
    cache.write(&snapshot(fingerprint)).unwrap();
    assert_eq!(load(&cache, fingerprint).status, CacheStatus::Hit);
    fs::write(cache.path(fingerprint), b"corrupt").unwrap();
    assert_eq!(load(&cache, fingerprint).status, CacheStatus::Error);
    assert_eq!(load(&cache, fingerprint).status, CacheStatus::Miss);
    cache.clear_all().unwrap();
}

#[test]
fn cache_hit_requires_matching_source_metadata() {
    let fixture = Fixture::new();
    let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    let fingerprint = SourceFingerprint::from_bytes([11; 32]);
    let waveform = snapshot(fingerprint);

    for (sample_rate, timeline) in [
        (
            SampleRate::new(44_100).unwrap(),
            SourceFrame::new(0)..SourceFrame::new(257),
        ),
        (
            SampleRate::new(48_000).unwrap(),
            SourceFrame::new(1)..SourceFrame::new(257),
        ),
        (
            SampleRate::new(48_000).unwrap(),
            SourceFrame::new(0)..SourceFrame::new(258),
        ),
    ] {
        cache.write(&waveform).unwrap();
        let result = cache.load(fingerprint, GenerationId::INITIAL, sample_rate, timeline);
        assert_eq!(result.status, CacheStatus::Error);
        assert!(result.snapshot.is_none());
    }
}

#[test]
fn record_reader_rejects_oversized_and_non_regular_files() {
    let fixture = Fixture::new();
    let root = CacheRoot::new(fixture.0.clone()).unwrap();
    let path = fixture.0.join("record.spwf");
    let file = fs::File::create(&path).unwrap();
    file.set_len(MAX_RECORD_BYTES + 1).unwrap();
    drop(file);
    assert_eq!(
        read_record(&root, Path::new("record.spwf"))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(read_record(&root, Path::new("record.spwf")).is_err());
}

#[test]
fn scan_limit_counts_unrelated_directory_entries() {
    let fixture = Fixture::new();
    let root = CacheRoot::new(fixture.0.clone()).unwrap();
    for index in 0..3 {
        fs::write(fixture.0.join(format!("{index}.txt")), []).unwrap();
    }

    assert!(entries_with_limit(&root, 2).is_err());
}

#[test]
fn single_level_ordinary_waveform_round_trips() {
    let fixture = Fixture::new();
    let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    let fingerprint = SourceFingerprint::from_bytes([8; 32]);
    cache.write(&ordinary_snapshot(fingerprint)).unwrap();
    let restored = cache
        .load(
            fingerprint,
            GenerationId::INITIAL,
            SampleRate::new(48_000).unwrap(),
            SourceFrame::new(0)..SourceFrame::new(4_096),
        )
        .snapshot
        .unwrap();
    assert_eq!(restored.levels.len(), 1);
    assert_eq!(restored.levels[0].roles[0].len(), 1_024);
}

#[test]
fn inverted_peaks_are_rejected_on_encode_and_decode() {
    let fingerprint = SourceFingerprint::from_bytes([10; 32]);
    let mut invalid = snapshot(fingerprint);
    let level = &invalid.levels[0];
    let mut roles = level.roles.clone();
    let mut peaks = roles[0].to_vec();
    peaks[0] = Peak {
        minimum: 1.0,
        maximum: -1.0,
    };
    roles[0] = peaks.into();
    invalid.levels = vec![PeakLevel {
        frames_per_peak: level.frames_per_peak,
        roles,
    }]
    .into();
    assert!(encode(&invalid).is_err());

    let mut encoded = encode(&snapshot(fingerprint)).unwrap();
    let checksum_start = encoded.len() - 32;
    let first_peak = MAGIC.len()
        + size_of::<u16>()
        + 32
        + size_of::<u32>()
        + size_of::<i64>() * 2
        + size_of::<u8>()
        + size_of::<u32>()
        + size_of::<u64>();
    encoded[first_peak..first_peak + 4].copy_from_slice(&1.0_f32.to_le_bytes());
    encoded[first_peak + 4..first_peak + 8].copy_from_slice(&(-1.0_f32).to_le_bytes());
    let checksum = Sha256::digest(&encoded[..checksum_start]);
    encoded[checksum_start..].copy_from_slice(&checksum);

    assert!(decode(&encoded, fingerprint, GenerationId::INITIAL).is_err());
}

#[test]
fn serialized_cache_contains_no_source_names_or_pcm() {
    let fixture = Fixture::new();
    let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    let fingerprint = SourceFingerprint::from_bytes([5; 32]);
    cache.write(&snapshot(fingerprint)).unwrap();
    let bytes = fs::read(cache.path(fingerprint)).unwrap();
    assert!(
        !bytes
            .windows(b"private-song.wav".len())
            .any(|part| part == b"private-song.wav")
    );
    assert!(
        !bytes
            .windows(b"C:\\Users\\Private".len())
            .any(|part| part == b"C:\\Users\\Private")
    );
}

#[test]
fn unknown_version_is_deleted_as_a_miss() {
    let fixture = Fixture::new();
    let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    let fingerprint = SourceFingerprint::from_bytes([6; 32]);
    cache.write(&snapshot(fingerprint)).unwrap();
    let path = cache.path(fingerprint);
    let mut bytes = fs::read(&path).unwrap();
    bytes[8..10].copy_from_slice(&99_u16.to_le_bytes());
    let checksum = Sha256::digest(&bytes[..bytes.len() - 32]);
    let checksum_start = bytes.len() - 32;
    bytes[checksum_start..].copy_from_slice(&checksum);
    fs::write(&path, bytes).unwrap();
    assert_eq!(load(&cache, fingerprint).status, CacheStatus::Error);
    assert!(!path.exists());
}
#[test]
fn hostile_timeline_and_resolution_are_rejected_without_allocation() {
    let fixture = Fixture::new();
    let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    let fingerprint = SourceFingerprint::from_bytes([7; 32]);
    let waveform = snapshot(fingerprint);
    cache.write(&waveform).unwrap();
    let path = cache.path(fingerprint);
    let mut bytes = fs::read(&path).unwrap();
    bytes[46..54].copy_from_slice(&i64::MIN.to_le_bytes());
    bytes[54..62].copy_from_slice(&i64::MAX.to_le_bytes());
    let checksum_start = bytes.len() - 32;
    let checksum = Sha256::digest(&bytes[..checksum_start]);
    bytes[checksum_start..].copy_from_slice(&checksum);
    fs::write(&path, bytes).unwrap();
    assert_eq!(load(&cache, fingerprint).status, CacheStatus::Error);

    cache.write(&waveform).unwrap();
    let mut bytes = fs::read(&path).unwrap();
    bytes[63..67].copy_from_slice(&0_u32.to_le_bytes());
    let checksum_start = bytes.len() - 32;
    let checksum = Sha256::digest(&bytes[..checksum_start]);
    bytes[checksum_start..].copy_from_slice(&checksum);
    fs::write(&path, bytes).unwrap();
    assert_eq!(load(&cache, fingerprint).status, CacheStatus::Error);
}

#[test]
fn entry_limit_evicts_the_oldest_record() {
    let fixture = Fixture::new();
    let cache = WaveformCache::new(
        fixture.0.clone(),
        CacheConfig {
            maximum_entries: 1,
            ..CacheConfig::default()
        },
    )
    .unwrap();
    let first = SourceFingerprint::from_bytes([1; 32]);
    let second = SourceFingerprint::from_bytes([2; 32]);
    cache.write(&snapshot(first)).unwrap();
    cache.write(&snapshot(second)).unwrap();
    assert_eq!(load(&cache, first).status, CacheStatus::Miss);
    assert_eq!(load(&cache, second).status, CacheStatus::Hit);
}

#[test]
fn reducing_capacity_evicts_existing_records() {
    let fixture = Fixture::new();
    let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    let fingerprint = SourceFingerprint::from_bytes([3; 32]);
    cache.write(&snapshot(fingerprint)).unwrap();
    cache.set_capacity_bytes(1).unwrap();
    assert_eq!(load(&cache, fingerprint).status, CacheStatus::Miss);
    assert!(cache.write(&snapshot(fingerprint)).is_err());
}

#[test]
fn concurrent_same_key_writes_publish_one_valid_record() {
    let fixture = Fixture::new();
    let cache =
        std::sync::Arc::new(WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap());
    let fingerprint = SourceFingerprint::from_bytes([4; 32]);
    let waveform = snapshot(fingerprint);
    let handles = (0..2)
        .map(|_| {
            let cache = std::sync::Arc::clone(&cache);
            let waveform = waveform.clone();
            std::thread::spawn(move || cache.write(&waveform))
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().unwrap().unwrap();
    }
    assert_eq!(load(&cache, fingerprint).status, CacheStatus::Hit);
}

#[test]
fn disk_full_and_read_only_failures_leave_no_cache_entry() {
    for (index, kind) in [
        std::io::ErrorKind::StorageFull,
        std::io::ErrorKind::PermissionDenied,
    ]
    .into_iter()
    .enumerate()
    {
        let fixture = Fixture::new();
        let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
        let fingerprint = SourceFingerprint::from_bytes([index as u8; 32]);
        WRITE_FAILURE.set(Some(kind));
        let result = cache.write(&snapshot(fingerprint));
        WRITE_FAILURE.set(None);
        assert!(result.is_err());
        assert!(entries(&cache.root).unwrap().is_empty());
    }
}
