use super::*;
use std::os::windows::fs::OpenOptionsExt;

#[test]
fn guarded_publication_keeps_cache_ancestors_protected() {
    let fixture = Fixture::new();
    let parent = fixture.0.join("intermediate");
    let root = parent.join("cache");
    let cache = WaveformCache::new(root.clone(), CacheConfig::default()).unwrap();
    let waveform = snapshot(SourceFingerprint::from_bytes([14; 32]));

    cache.write(&waveform).unwrap();

    assert_eq!(load(&cache, waveform.fingerprint).status, CacheStatus::Hit);
    assert_eq!(
        fs::read(cache.path(waveform.fingerprint)).unwrap(),
        encode(&waveform).unwrap()
    );
    for directory in [&parent, &root] {
        assert!(fs::rename(directory, directory.with_extension("moved")).is_err());
        assert!(
            fs::OpenOptions::new()
                .write(true)
                .share_mode(7)
                .custom_flags(0x0200_0000)
                .open(directory)
                .is_err()
        );
    }
}

#[test]
fn created_records_allow_verification_but_refuse_other_writers_and_deletion() {
    let fixture = Fixture::new();
    let root = CacheRoot::new(fixture.0.clone()).unwrap();
    let name = Path::new("created.spwf");
    let file = root.open(name, CacheFileAccess::Create).unwrap();

    assert!(root.open(name, CacheFileAccess::Read).is_ok());
    assert!(root.open(name, CacheFileAccess::Update).is_err());
    assert!(root.remove(name).is_err());

    drop(file);
    root.remove(name).unwrap();
}

#[test]
fn partial_corrupt_and_mismatched_records_are_rebuilt_before_reuse() {
    let waveform = snapshot(SourceFingerprint::from_bytes([15; 32]));
    let encoded = encode(&waveform).unwrap();
    let wrong_rate = WaveformSnapshot {
        sample_rate: SampleRate::new(44_100).unwrap(),
        ..waveform.clone()
    };
    let mut analyzer = PeakAnalyzer::new(
        waveform.generation,
        waveform.fingerprint,
        SourceFrame::new(1),
        waveform.end_frame_exclusive,
        waveform.sample_rate,
    )
    .unwrap();
    analyzer
        .ingest(SourceFrame::new(1), 256, 8, &vec![0.25; 256 * 8])
        .unwrap();
    let wrong_start = analyzer.finish(CacheStatus::Miss).unwrap();
    let wrong_end = ordinary_snapshot(waveform.fingerprint);
    for stale in [
        encoded[..encoded.len() / 2].to_vec(),
        b"corrupt synthetic record".to_vec(),
        encode(&wrong_rate).unwrap(),
        encode(&wrong_start).unwrap(),
        encode(&wrong_end).unwrap(),
    ] {
        let fixture = Fixture::new();
        let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
        fs::write(cache.path(waveform.fingerprint), stale).unwrap();

        cache.write(&waveform).unwrap();

        assert_eq!(load(&cache, waveform.fingerprint).status, CacheStatus::Hit);
        assert_eq!(fs::read(cache.path(waveform.fingerprint)).unwrap(), encoded);
    }
}

#[test]
fn oversized_records_are_rebuilt_without_unbounded_reads() {
    let fixture = Fixture::new();
    let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    let waveform = snapshot(SourceFingerprint::from_bytes([21; 32]));
    let file = fs::File::create(cache.path(waveform.fingerprint)).unwrap();
    file.set_len(MAX_RECORD_BYTES + 1).unwrap();
    drop(file);

    cache.write(&waveform).unwrap();

    assert_eq!(load(&cache, waveform.fingerprint).status, CacheStatus::Hit);
}

#[test]
fn valid_protected_records_are_reused_without_writing() {
    let fixture = Fixture::new();
    let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    let waveform = snapshot(SourceFingerprint::from_bytes([16; 32]));
    cache.write(&waveform).unwrap();
    let path = cache.path(waveform.fingerprint);
    let before = fs::read(&path).unwrap();
    let protected = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    WRITE_FAILURE.set(Some(std::io::ErrorKind::StorageFull));

    let result = cache.write(&waveform);

    WRITE_FAILURE.set(None);
    result.unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    drop(protected);
}

#[test]
fn unreadable_or_delete_protected_records_are_not_overwritten() {
    for sharing in [0, 1] {
        let fixture = Fixture::new();
        let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
        let waveform = snapshot(SourceFingerprint::from_bytes([17; 32]));
        let path = cache.path(waveform.fingerprint);
        fs::write(&path, b"protected synthetic record").unwrap();
        let protected = fs::OpenOptions::new()
            .read(true)
            .share_mode(sharing)
            .open(&path)
            .unwrap();

        assert!(cache.write(&waveform).is_err());

        drop(protected);
        assert_eq!(fs::read(path).unwrap(), b"protected synthetic record");
    }
}

#[test]
fn partial_final_records_are_hidden_by_the_lock_and_recoverable_after_release() {
    let fixture = Fixture::new();
    let first = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    let second = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    let waveform = snapshot(SourceFingerprint::from_bytes([18; 32]));
    let guard = first.root.guard().unwrap();
    fs::write(
        first.path(waveform.fingerprint),
        b"partial synthetic record",
    )
    .unwrap();

    assert_eq!(
        load(&second, waveform.fingerprint).status,
        CacheStatus::Error
    );
    assert!(second.write(&waveform).is_err());
    assert_eq!(
        fs::read(first.path(waveform.fingerprint)).unwrap(),
        b"partial synthetic record"
    );

    drop(guard);
    assert_eq!(
        load(&second, waveform.fingerprint).status,
        CacheStatus::Error
    );
    assert!(!second.path(waveform.fingerprint).exists());
    second.write(&waveform).unwrap();
    assert_eq!(load(&first, waveform.fingerprint).status, CacheStatus::Hit);
}

#[test]
fn failed_publication_cleans_its_record_and_preserves_other_records() {
    let fixture = Fixture::new();
    let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    let retained = snapshot(SourceFingerprint::from_bytes([19; 32]));
    let incoming = snapshot(SourceFingerprint::from_bytes([20; 32]));
    cache.write(&retained).unwrap();
    let before = fs::read(cache.path(retained.fingerprint)).unwrap();
    WRITE_FAILURE.set(Some(std::io::ErrorKind::StorageFull));

    let result = cache.write(&incoming);

    WRITE_FAILURE.set(None);
    assert!(result.is_err());
    assert!(!cache.path(incoming.fingerprint).exists());
    assert_eq!(fs::read(cache.path(retained.fingerprint)).unwrap(), before);
    assert_eq!(load(&cache, retained.fingerprint).status, CacheStatus::Hit);
}
