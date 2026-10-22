use super::*;

#[test]
fn hostile_entry_sizes_cannot_overflow_quota_accounting() {
    let entries = [u64::MAX, 1].map(|bytes| Entry {
        name: PathBuf::from("synthetic.spwf"),
        bytes,
        modified: SystemTime::UNIX_EPOCH,
    });
    assert!(total_bytes(&entries).is_err());
}

#[test]
fn directory_relative_cache_operations_reject_escape_names() {
    let fixture = Fixture::new();
    let sentinel = fixture.0.join("outside.spwf");
    fs::write(&sentinel, b"outside waveform").unwrap();
    let root = CacheRoot::new(fixture.0.join("cache")).unwrap();

    for name in ["../outside.spwf", "nested/record.spwf", ".", ".."] {
        let name = Path::new(name);
        assert!(root.open(name, CacheFileAccess::Create).is_err());
        assert!(root.metadata(name).is_err());
        assert!(root.remove(name).is_err());
        #[cfg(not(windows))]
        assert!(root.rename(Path::new("temporary.tmp"), name).is_err());
    }

    assert_eq!(fs::read(sentinel).unwrap(), b"outside waveform");
}

#[test]
fn independent_instances_share_the_operation_lock_and_entry_quota() {
    let fixture = Fixture::new();
    let config = CacheConfig {
        maximum_entries: 1,
        ..CacheConfig::default()
    };
    let first = WaveformCache::new(fixture.0.clone(), config).unwrap();
    let second = WaveformCache::new(fixture.0.clone(), config).unwrap();
    let first_key = SourceFingerprint::from_bytes([1; 32]);
    let second_key = SourceFingerprint::from_bytes([2; 32]);
    let guard = first.root.guard().unwrap();

    assert!(second.write(&snapshot(second_key)).is_err());
    assert!(second.clear_all().is_err());
    assert_eq!(load(&second, second_key).status, CacheStatus::Error);
    drop(guard);

    first.write(&snapshot(first_key)).unwrap();
    second.write(&snapshot(second_key)).unwrap();
    assert_eq!(entries(&first.root).unwrap().len(), 1);
    assert_eq!(load(&first, first_key).status, CacheStatus::Miss);
    assert_eq!(load(&first, second_key).status, CacheStatus::Hit);
}

#[test]
fn legacy_overfull_roots_can_be_trimmed_and_cleared() {
    let fixture = Fixture::new();
    for index in 0..=MAX_SCAN_ENTRIES {
        fs::write(fixture.0.join(format!("{index:064x}.spwf")), []).unwrap();
    }
    let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
    cache
        .write(&snapshot(SourceFingerprint::from_bytes([255; 32])))
        .unwrap();
    assert_eq!(entries(&cache.root).unwrap().len(), MAX_SCAN_ENTRIES);

    fs::write(fixture.0.join("overflow-one.spwf"), []).unwrap();
    fs::write(fixture.0.join("overflow-two.spwf"), []).unwrap();
    cache.clear_all().unwrap();
    assert!(entries(&cache.root).unwrap().is_empty());
}

#[test]
fn cache_root_replacement_never_removes_outside_sentinels() {
    let fixture = Fixture::new();
    let root = fixture.0.join("cache");
    let moved = fixture.0.join("moved-cache");
    let outside = fixture.0.join("outside");
    fs::create_dir(&outside).unwrap();
    let key = SourceFingerprint::from_bytes([12; 32]);
    let sentinel = outside.join(format!("{}.spwf", key.hex()));
    let temporary = outside.join("outside.tmp");
    fs::write(&sentinel, b"outside waveform").unwrap();
    fs::write(&temporary, b"outside temporary").unwrap();
    let cache = WaveformCache::new(root.clone(), CacheConfig::default()).unwrap();
    cache.write(&snapshot(key)).unwrap();

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        assert!(fs::rename(&root, &moved).is_err());
        assert!(
            fs::OpenOptions::new()
                .write(true)
                .share_mode(7)
                .custom_flags(0x0200_0000)
                .open(&root)
                .is_err()
        );
        cache.clear_all().unwrap();
    }
    #[cfg(unix)]
    {
        fs::rename(&root, &moved).unwrap();
        std::os::unix::fs::symlink(&outside, &root).unwrap();
        assert!(cache.clear_current(key).is_err());
        assert!(cache.clear_all().is_err());
        assert!(cache.set_capacity_bytes(1).is_err());
        assert!(cache.write(&snapshot(key)).is_err());
        assert_eq!(load(&cache, key).status, CacheStatus::Error);
        fs::remove_file(&root).unwrap();
    }

    assert_eq!(fs::read(&sentinel).unwrap(), b"outside waveform");
    assert_eq!(fs::read(&temporary).unwrap(), b"outside temporary");
}

#[cfg(windows)]
#[test]
fn an_unprotected_intermediate_blocks_descendant_creation() {
    use std::os::windows::fs::OpenOptionsExt;

    let fixture = Fixture::new();
    let intermediate = fixture.0.join("intermediate");
    let descendant = intermediate.join("nested");
    let root = descendant.join("cache");
    fs::create_dir(&intermediate).unwrap();
    let blocker = fs::OpenOptions::new()
        .write(true)
        .share_mode(7)
        .custom_flags(0x0200_0000)
        .open(&intermediate)
        .unwrap();

    assert!(crate::cache_root::create_root(&root, &fixture.0).is_err());
    assert!(!descendant.exists());
    drop(blocker);
}

#[cfg(windows)]
#[test]
fn newly_created_cache_ancestors_stay_protected() {
    use std::os::windows::fs::OpenOptionsExt;

    let fixture = Fixture::new();
    let intermediate = fixture.0.join("intermediate");
    let descendant = intermediate.join("nested");
    let root = descendant.join("cache");
    let cache = CacheRoot::new(root.clone()).unwrap();

    for directory in [&intermediate, &descendant, &root] {
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

    drop(cache);
    fs::rename(&intermediate, intermediate.with_extension("moved")).unwrap();
}

#[cfg(unix)]
#[test]
fn a_linked_root_is_rejected_but_a_canonical_parent_alias_is_bound() {
    let fixture = Fixture::new();
    let physical = fixture.0.join("physical");
    let alias = fixture.0.join("alias");
    fs::create_dir(&physical).unwrap();
    std::os::unix::fs::symlink(&physical, &alias).unwrap();
    assert!(WaveformCache::new(alias.clone(), CacheConfig::default()).is_err());

    let cache = WaveformCache::new(alias.join("cache"), CacheConfig::default()).unwrap();
    let key = SourceFingerprint::from_bytes([13; 32]);
    cache.write(&snapshot(key)).unwrap();
    assert_eq!(load(&cache, key).status, CacheStatus::Hit);
    assert!(
        physical
            .join("cache")
            .join(format!("{}.spwf", key.hex()))
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn an_open_directory_anchor_stays_inside_the_original_root() {
    let fixture = Fixture::new();
    let root = fixture.0.join("cache");
    let moved = fixture.0.join("moved-cache");
    let outside = fixture.0.join("outside");
    fs::create_dir(&outside).unwrap();
    let key = SourceFingerprint::from_bytes([14; 32]);
    let sentinel = outside.join(format!("{}.spwf", key.hex()));
    fs::write(&sentinel, b"outside waveform").unwrap();
    let cache = WaveformCache::new(root.clone(), CacheConfig::default()).unwrap();
    cache.write(&snapshot(key)).unwrap();
    let guard = cache.root.guard().unwrap();

    fs::rename(&root, &moved).unwrap();
    std::os::unix::fs::symlink(&outside, &root).unwrap();
    remove_if_present(&cache.root, &cache.name(key)).unwrap();
    drop(guard);

    assert!(!moved.join(format!("{}.spwf", key.hex())).exists());
    assert_eq!(fs::read(&sentinel).unwrap(), b"outside waveform");
    fs::remove_file(root).unwrap();
}

#[cfg(unix)]
#[test]
fn a_fifo_cache_entry_is_rejected_without_waiting_for_a_writer() {
    use rustix::fs::{Mode, mkfifoat};
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;

    let fixture = Fixture::new();
    let root = CacheRoot::new(fixture.0.clone()).unwrap();
    mkfifoat(&root.directory, "fifo.spwf", Mode::RUSR | Mode::WUSR).unwrap();
    let (sender, result) = sync_channel(1);
    let worker = std::thread::spawn(move || {
        sender
            .send(read_record(&root, Path::new("fifo.spwf")))
            .unwrap();
    });
    assert!(
        result
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_err()
    );
    worker.join().unwrap();
}
