use sha2::{Digest, Sha256};
use std::array;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;
use stemplayer_domain::{ErrorCode, GenerationId, PlayerError, SampleRate, SourceFrame};

use crate::analyzer::WAVEFORM_COLUMNS;
use crate::{
    CacheStatus, ConsistencySummary, Peak, PeakLevel, SourceFingerprint, WaveformSnapshot,
};

const MAGIC: &[u8; 8] = b"SPWF0001";
const SCHEMA: u16 = 1;
const EXTENSION: &str = "spwf";
const MAX_RECORD_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SCAN_ENTRIES: usize = 4_096;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
thread_local! {
    static WRITE_FAILURE: std::cell::Cell<Option<std::io::ErrorKind>> = const { std::cell::Cell::new(None) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheConfig {
    pub enabled: bool,
    pub capacity_bytes: u64,
    pub maximum_entries: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            capacity_bytes: 512 * 1024 * 1024,
            maximum_entries: MAX_SCAN_ENTRIES,
        }
    }
}

#[derive(Debug)]
pub struct CacheResult {
    pub status: CacheStatus,
    pub snapshot: Option<WaveformSnapshot>,
    pub bytes: u64,
}

pub struct WaveformCache {
    root: PathBuf,
    enabled: bool,
    capacity_bytes: AtomicU64,
    maximum_entries: usize,
    operation: Mutex<()>,
}

impl WaveformCache {
    pub fn new(root: PathBuf, config: CacheConfig) -> Result<Self, PlayerError> {
        if config.capacity_bytes == 0
            || config.maximum_entries == 0
            || config.maximum_entries > MAX_SCAN_ENTRIES
        {
            return Err(error("Waveform cache limits are invalid"));
        }
        ensure_root(&root)?;
        Ok(Self {
            root,
            enabled: config.enabled,
            capacity_bytes: AtomicU64::new(config.capacity_bytes),
            maximum_entries: config.maximum_entries,
            operation: Mutex::new(()),
        })
    }

    pub fn load(&self, fingerprint: SourceFingerprint, generation: GenerationId) -> CacheResult {
        if !self.enabled {
            return result(CacheStatus::Disabled);
        }
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = self.path(fingerprint);
        let bytes = match read_record(&path) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return result(CacheStatus::Miss);
            }
            Err(_) => {
                let _ = fs::remove_file(path);
                return result(CacheStatus::Error);
            }
        };
        match decode(&bytes, fingerprint, generation) {
            Ok(snapshot) => {
                touch(&path);
                CacheResult {
                    status: CacheStatus::Hit,
                    snapshot: Some(snapshot),
                    bytes: bytes.len() as u64,
                }
            }
            Err(_) => {
                let _ = fs::remove_file(path);
                result(CacheStatus::Error)
            }
        }
    }

    pub fn write(&self, snapshot: &WaveformSnapshot) -> Result<(), PlayerError> {
        if !self.enabled {
            return Ok(());
        }
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bytes = encode(snapshot)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES
            || bytes.len() as u64 > self.capacity_bytes.load(Ordering::Acquire)
        {
            return Err(error("Waveform cache record exceeds its limit"));
        }
        self.evict(bytes.len() as u64, Some(snapshot.fingerprint))?;
        let final_path = self.path(snapshot.fingerprint);
        if final_path.exists() {
            return Ok(());
        }
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temporary = self.root.join(format!(
            ".{}-{}-{id}.tmp",
            snapshot.fingerprint.hex(),
            std::process::id()
        ));
        let write_result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|source| {
                    io_error("Waveform cache temporary file cannot be created", source)
                })?;
            #[cfg(test)]
            if let Some(kind) = WRITE_FAILURE.get() {
                return Err(io_error(
                    "Waveform cache record cannot be flushed",
                    std::io::Error::from(kind),
                ));
            }
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|source| io_error("Waveform cache record cannot be flushed", source))?;
            let verified = read_record(&temporary)
                .map_err(|source| io_error("Waveform cache record cannot be validated", source))?;
            decode(&verified, snapshot.fingerprint, snapshot.generation)?;
            fs::rename(&temporary, &final_path)
                .map_err(|source| io_error("Waveform cache record cannot be committed", source))
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result
    }

    pub fn clear_current(&self, fingerprint: SourceFingerprint) -> Result<(), PlayerError> {
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        remove_if_present(&self.path(fingerprint))
    }

    pub fn clear_all(&self) -> Result<(), PlayerError> {
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for entry in entries(&self.root)? {
            remove_if_present(&entry.path)?;
        }
        Ok(())
    }

    pub fn set_capacity_bytes(&self, capacity_bytes: u64) -> Result<(), PlayerError> {
        if capacity_bytes == 0 {
            return Err(error("Waveform cache capacity is invalid"));
        }
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut entries = entries(&self.root)?;
        entries.sort_by_key(|entry| entry.modified);
        let mut bytes = entries.iter().map(|entry| entry.bytes).sum::<u64>();
        while bytes > capacity_bytes || entries.len() > self.maximum_entries {
            let entry = entries.remove(0);
            remove_if_present(&entry.path)?;
            bytes = bytes.saturating_sub(entry.bytes);
        }
        self.capacity_bytes.store(capacity_bytes, Ordering::Release);
        Ok(())
    }

    fn path(&self, fingerprint: SourceFingerprint) -> PathBuf {
        self.root
            .join(format!("{}.{}", fingerprint.hex(), EXTENSION))
    }

    fn evict(
        &self,
        incoming_bytes: u64,
        retained: Option<SourceFingerprint>,
    ) -> Result<(), PlayerError> {
        let retained = retained.map(|value| self.path(value));
        let mut entries = entries(&self.root)?;
        entries.sort_by_key(|entry| entry.modified);
        let mut bytes = entries.iter().map(|entry| entry.bytes).sum::<u64>();
        while bytes.saturating_add(incoming_bytes) > self.capacity_bytes.load(Ordering::Acquire)
            || entries.len() >= self.maximum_entries
        {
            let Some(index) = entries
                .iter()
                .position(|entry| retained.as_ref() != Some(&entry.path))
            else {
                break;
            };
            let entry = entries.remove(index);
            remove_if_present(&entry.path)?;
            bytes = bytes.saturating_sub(entry.bytes);
        }
        Ok(())
    }
}

struct Entry {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

fn read_record(path: &Path) -> std::io::Result<Vec<u8>> {
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "waveform cache record is not a bounded regular file",
        )
    };
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES
    {
        return Err(invalid());
    }
    let file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_RECORD_BYTES {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(invalid());
    }
    Ok(bytes)
}

fn entries(root: &Path) -> Result<Vec<Entry>, PlayerError> {
    let mut entries = Vec::new();
    let directory = fs::read_dir(root)
        .map_err(|source| io_error("Waveform cache directory cannot be read", source))?;
    for item in directory.take(MAX_SCAN_ENTRIES + 1) {
        let item =
            item.map_err(|source| io_error("Waveform cache entry cannot be read", source))?;
        if entries.len() == MAX_SCAN_ENTRIES {
            return Err(error("Waveform cache directory exceeds its scan limit"));
        }
        let path = item.path();
        if path.extension().and_then(|value| value.to_str()) != Some(EXTENSION) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|source| io_error("Waveform cache metadata is unavailable", source))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        entries.push(Entry {
            path,
            bytes: metadata.len(),
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        });
    }
    Ok(entries)
}

fn ensure_root(root: &Path) -> Result<(), PlayerError> {
    fs::create_dir_all(root)
        .map_err(|source| io_error("Waveform cache directory cannot be created", source))?;
    let metadata = fs::symlink_metadata(root)
        .map_err(|source| io_error("Waveform cache directory metadata is unavailable", source))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(error("Waveform cache root is unsafe"));
    }
    for item in fs::read_dir(root)
        .map_err(|source| io_error("Waveform cache directory cannot be read", source))?
        .take(MAX_SCAN_ENTRIES)
    {
        let path = item
            .map_err(|source| io_error("Waveform cache entry cannot be read", source))?
            .path();
        if path.extension().and_then(|value| value.to_str()) == Some("tmp") {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

fn encode(snapshot: &WaveformSnapshot) -> Result<Vec<u8>, PlayerError> {
    if snapshot.levels.len() != 1 || snapshot.progress != 1.0 {
        return Err(error("Only complete single-level waveforms can be cached"));
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&SCHEMA.to_le_bytes());
    bytes.extend_from_slice(snapshot.fingerprint.as_bytes());
    bytes.extend_from_slice(&snapshot.sample_rate.get().to_le_bytes());
    bytes.extend_from_slice(&snapshot.start_frame.get().to_le_bytes());
    bytes.extend_from_slice(&snapshot.end_frame_exclusive.get().to_le_bytes());
    bytes.push(snapshot.levels.len() as u8);
    for level in snapshot.levels.iter() {
        let count = level.roles[0].len();
        if level.roles.iter().any(|role| role.len() != count) {
            return Err(error("Waveform cache role lengths differ"));
        }
        bytes.extend_from_slice(&level.frames_per_peak.to_le_bytes());
        bytes.extend_from_slice(&(count as u64).to_le_bytes());
        for role in &level.roles {
            for peak in role.iter() {
                if !peak.minimum.is_finite() || !peak.maximum.is_finite() {
                    return Err(error("Waveform cache peak is not finite"));
                }
                bytes.extend_from_slice(&peak.minimum.to_le_bytes());
                bytes.extend_from_slice(&peak.maximum.to_le_bytes());
            }
        }
    }
    if let Some(summary) = snapshot.consistency {
        bytes.push(1);
        bytes.extend_from_slice(&summary.peak.to_le_bytes());
        bytes.extend_from_slice(&summary.rms.to_le_bytes());
        bytes.extend_from_slice(&summary.dbfs.to_le_bytes());
        bytes.extend_from_slice(&summary.frames_examined.to_le_bytes());
        bytes.extend_from_slice(&summary.non_finite_count.to_le_bytes());
    } else {
        bytes.push(0);
    }
    let checksum = Sha256::digest(&bytes);
    bytes.extend_from_slice(&checksum);
    Ok(bytes)
}

fn decode(
    bytes: &[u8],
    fingerprint: SourceFingerprint,
    generation: GenerationId,
) -> Result<WaveformSnapshot, PlayerError> {
    if bytes.len() < 32
        || Sha256::digest(&bytes[..bytes.len() - 32]).as_slice() != &bytes[bytes.len() - 32..]
    {
        return Err(error("Waveform cache checksum is invalid"));
    }
    let mut reader = Reader::new(&bytes[..bytes.len() - 32]);
    if reader.take(8)? != MAGIC
        || reader.u16()? != SCHEMA
        || reader.take(32)? != fingerprint.as_bytes()
    {
        return Err(error("Waveform cache identity or version is invalid"));
    }
    let sample_rate = SampleRate::new(reader.u32()?)?;
    let start = SourceFrame::new(reader.i64()?);
    let end = SourceFrame::new(reader.i64()?);
    let total = end
        .get()
        .checked_sub(start.get())
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| error("Waveform cache timeline is invalid"))?;
    let level_count = usize::from(reader.u8()?);
    if level_count != 1 {
        return Err(error("Waveform cache timeline is invalid"));
    }
    let expected_resolution = u32::try_from(total.div_ceil(WAVEFORM_COLUMNS).max(1))
        .map_err(|_| error("Waveform cache peak resolution is invalid"))?;
    let mut levels = Vec::with_capacity(level_count);
    for _ in 0..level_count {
        let resolution = reader.u32()?;
        if resolution != expected_resolution {
            return Err(error("Waveform cache peak resolution is invalid"));
        }
        let count = usize::try_from(reader.u64()?)
            .map_err(|_| error("Waveform cache peak count is invalid"))?;
        let expected_count =
            usize::try_from(total.div_ceil(u64::from(resolution))).unwrap_or(usize::MAX);
        let serialized_bytes = count
            .checked_mul(4 * 2 * size_of::<f32>())
            .ok_or_else(|| error("Waveform cache peak count is invalid"))?;
        if count != expected_count || serialized_bytes > reader.remaining() {
            return Err(error(
                "Waveform cache peak count does not match its timeline",
            ));
        }
        let mut roles: [Vec<Peak>; 4] = array::from_fn(|_| Vec::new());
        for role in &mut roles {
            role.try_reserve_exact(count)
                .map_err(|_| error("Waveform cache peak allocation failed"))?;
            for _ in 0..count {
                let peak = Peak {
                    minimum: reader.f32()?,
                    maximum: reader.f32()?,
                };
                if !peak.minimum.is_finite() || !peak.maximum.is_finite() {
                    return Err(error("Waveform cache peak is not finite"));
                }
                role.push(peak);
            }
        }
        levels.push(PeakLevel {
            frames_per_peak: resolution,
            roles: roles.map(Into::into),
        });
    }
    let consistency = match reader.u8()? {
        0 => None,
        1 => {
            let summary = ConsistencySummary {
                peak: reader.f32()?,
                rms: reader.f32()?,
                dbfs: reader.f32()?,
                frames_examined: reader.u64()?,
                non_finite_count: reader.u64()?,
            };
            if !summary.peak.is_finite()
                || summary.peak < 0.0
                || !summary.rms.is_finite()
                || summary.rms < 0.0
                || !(summary.dbfs.is_finite() || summary.dbfs == f32::NEG_INFINITY)
            {
                return Err(error("Waveform cache consistency data is invalid"));
            }
            Some(summary)
        }
        _ => return Err(error("Waveform cache consistency flag is invalid")),
    };
    if !reader.is_empty() {
        return Err(error("Waveform cache has trailing data"));
    }
    Ok(WaveformSnapshot {
        generation,
        fingerprint,
        start_frame: start,
        end_frame_exclusive: end,
        analyzed_end_frame: end,
        sample_rate,
        levels: levels.into(),
        progress: 1.0,
        cache_status: CacheStatus::Hit,
        consistency,
    })
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], PlayerError> {
        let end = self
            .offset
            .checked_add(count)
            .ok_or_else(|| error("Waveform cache offset overflowed"))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| error("Waveform cache is truncated"))?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, PlayerError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, PlayerError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, PlayerError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, PlayerError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> Result<i64, PlayerError> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32, PlayerError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.offset
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

fn touch(path: &Path) {
    if let Ok(mut file) = OpenOptions::new().read(true).write(true).open(path) {
        let mut byte = [0];
        if file.read_exact(&mut byte).is_ok() && file.seek(SeekFrom::Start(0)).is_ok() {
            let _ = file.write_all(&byte);
        }
    }
}

fn remove_if_present(path: &Path) -> Result<(), PlayerError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error("Waveform cache entry cannot be removed", source)),
    }
}

fn result(status: CacheStatus) -> CacheResult {
    CacheResult {
        status,
        snapshot: None,
        bytes: 0,
    }
}

fn error(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::AudioDataInvalid, summary)
}

fn io_error(summary: &str, source: std::io::Error) -> PlayerError {
    error(summary).with_source(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PeakAnalyzer;
    use std::sync::atomic::{AtomicU64, Ordering};

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
    fn miss_write_hit_corruption_and_clear_are_disposable() {
        let fixture = Fixture::new();
        let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
        let fingerprint = SourceFingerprint::from_bytes([9; 32]);
        assert_eq!(
            cache.load(fingerprint, GenerationId::INITIAL).status,
            CacheStatus::Miss
        );
        cache.write(&snapshot(fingerprint)).unwrap();
        assert_eq!(
            cache.load(fingerprint, GenerationId::INITIAL).status,
            CacheStatus::Hit
        );
        fs::write(cache.path(fingerprint), b"corrupt").unwrap();
        assert_eq!(
            cache.load(fingerprint, GenerationId::INITIAL).status,
            CacheStatus::Error
        );
        assert_eq!(
            cache.load(fingerprint, GenerationId::INITIAL).status,
            CacheStatus::Miss
        );
        cache.clear_all().unwrap();
    }

    #[test]
    fn record_reader_rejects_oversized_and_non_regular_files() {
        let fixture = Fixture::new();
        let path = fixture.0.join("record.spwf");
        let file = fs::File::create(&path).unwrap();
        file.set_len(MAX_RECORD_BYTES + 1).unwrap();
        drop(file);
        assert_eq!(
            read_record(&path).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert_eq!(
            read_record(&path).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn single_level_ordinary_waveform_round_trips() {
        let fixture = Fixture::new();
        let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
        let fingerprint = SourceFingerprint::from_bytes([8; 32]);
        cache.write(&ordinary_snapshot(fingerprint)).unwrap();
        let restored = cache
            .load(fingerprint, GenerationId::INITIAL)
            .snapshot
            .unwrap();
        assert_eq!(restored.levels.len(), 1);
        assert_eq!(restored.levels[0].roles[0].len(), 1_024);
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
        assert_eq!(
            cache.load(fingerprint, GenerationId::INITIAL).status,
            CacheStatus::Error
        );
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
        assert_eq!(
            cache.load(fingerprint, GenerationId::INITIAL).status,
            CacheStatus::Error
        );

        cache.write(&waveform).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        bytes[63..67].copy_from_slice(&0_u32.to_le_bytes());
        let checksum_start = bytes.len() - 32;
        let checksum = Sha256::digest(&bytes[..checksum_start]);
        bytes[checksum_start..].copy_from_slice(&checksum);
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            cache.load(fingerprint, GenerationId::INITIAL).status,
            CacheStatus::Error
        );
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
        assert_eq!(
            cache.load(first, GenerationId::INITIAL).status,
            CacheStatus::Miss
        );
        assert_eq!(
            cache.load(second, GenerationId::INITIAL).status,
            CacheStatus::Hit
        );
    }

    #[test]
    fn reducing_capacity_evicts_existing_records() {
        let fixture = Fixture::new();
        let cache = WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap();
        let fingerprint = SourceFingerprint::from_bytes([3; 32]);
        cache.write(&snapshot(fingerprint)).unwrap();
        cache.set_capacity_bytes(1).unwrap();
        assert_eq!(
            cache.load(fingerprint, GenerationId::INITIAL).status,
            CacheStatus::Miss
        );
        assert!(cache.write(&snapshot(fingerprint)).is_err());
    }

    #[test]
    fn concurrent_same_key_writes_publish_one_valid_record() {
        let fixture = Fixture::new();
        let cache = std::sync::Arc::new(
            WaveformCache::new(fixture.0.clone(), CacheConfig::default()).unwrap(),
        );
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
        assert_eq!(
            cache.load(fingerprint, GenerationId::INITIAL).status,
            CacheStatus::Hit
        );
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
            assert!(fs::read_dir(&fixture.0).unwrap().next().is_none());
        }
    }
}
