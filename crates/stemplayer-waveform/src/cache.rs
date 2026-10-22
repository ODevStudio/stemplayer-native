use sha2::{Digest, Sha256};
use std::array;
use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;
use stemplayer_domain::{ErrorCode, GenerationId, PlayerError, SampleRate, SourceFrame};

use crate::analyzer::WAVEFORM_COLUMNS;
use crate::cache_io::CacheFileAccess;
use crate::cache_root::CacheRoot;
use crate::{
    CacheStatus, ConsistencySummary, Peak, PeakLevel, SourceFingerprint, WaveformSnapshot,
};

#[cfg(windows)]
#[path = "cache_windows.rs"]
mod windows;

const MAGIC: &[u8; 8] = b"SPWF0001";
const SCHEMA: u16 = 1;
const EXTENSION: &str = "spwf";
const MAX_RECORD_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SCAN_ENTRIES: usize = 4_096;
const MAX_RECOVERY_ENTRIES: usize = MAX_SCAN_ENTRIES * 2;
#[cfg(not(windows))]
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
    root: CacheRoot,
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
        let root = CacheRoot::new(root)?;
        Ok(Self {
            root,
            enabled: config.enabled,
            capacity_bytes: AtomicU64::new(config.capacity_bytes),
            maximum_entries: config.maximum_entries,
            operation: Mutex::new(()),
        })
    }

    pub fn load(
        &self,
        fingerprint: SourceFingerprint,
        generation: GenerationId,
        expected_sample_rate: SampleRate,
        expected_timeline: Range<SourceFrame>,
    ) -> CacheResult {
        if !self.enabled {
            return result(CacheStatus::Disabled);
        }
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Ok(_root) = self.root.guard() else {
            return result(CacheStatus::Error);
        };
        if self.cleanup_temporary().is_err() {
            return result(CacheStatus::Error);
        }
        let name = self.name(fingerprint);
        let bytes = match read_record(&self.root, &name) {
            Ok(bytes) => bytes,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return result(CacheStatus::Miss);
            }
            Err(_) => {
                let _ = self.root.remove(&name);
                return result(CacheStatus::Error);
            }
        };
        match decode(&bytes, fingerprint, generation) {
            Ok(snapshot)
                if snapshot.sample_rate == expected_sample_rate
                    && snapshot.start_frame == expected_timeline.start
                    && snapshot.end_frame_exclusive == expected_timeline.end =>
            {
                touch(&self.root, &name);
                CacheResult {
                    status: CacheStatus::Hit,
                    snapshot: Some(snapshot),
                    bytes: bytes.len() as u64,
                }
            }
            Ok(_) | Err(_) => {
                let _ = self.root.remove(&name);
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
        let _root = self.root.guard()?;
        self.cleanup_temporary()?;
        let bytes = encode(snapshot)?;
        if bytes.len() as u64 > MAX_RECORD_BYTES
            || bytes.len() as u64 > self.capacity_bytes.load(Ordering::Acquire)
        {
            return Err(error("Waveform cache record exceeds its limit"));
        }
        #[cfg(windows)]
        {
            self.write_guarded(snapshot, &bytes)
        }
        #[cfg(not(windows))]
        {
            self.write_temporary(snapshot, &bytes)
        }
    }

    #[cfg(not(windows))]
    fn write_temporary(
        &self,
        snapshot: &WaveformSnapshot,
        bytes: &[u8],
    ) -> Result<(), PlayerError> {
        let final_name = self.name(snapshot.fingerprint);
        if self
            .root
            .metadata(&final_name)
            .is_ok_and(|metadata| metadata.regular)
        {
            return Ok(());
        }
        self.evict(bytes.len() as u64, Some(snapshot.fingerprint))?;
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let temporary = PathBuf::from(format!(
            ".{}-{}-{id}.tmp",
            snapshot.fingerprint.hex(),
            std::process::id()
        ));
        let write_result = (|| {
            let mut file = self
                .root
                .open(&temporary, CacheFileAccess::Create)
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
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|source| io_error("Waveform cache record cannot be flushed", source))?;
            drop(file);
            let verified = read_record(&self.root, &temporary)
                .map_err(|source| io_error("Waveform cache record cannot be validated", source))?;
            decode(&verified, snapshot.fingerprint, snapshot.generation)?;
            self.root
                .rename(&temporary, &final_name)
                .map_err(|source| io_error("Waveform cache record cannot be committed", source))
        })();
        if write_result.is_err() {
            let _ = self.root.remove(&temporary);
        }
        write_result
    }

    pub fn clear_current(&self, fingerprint: SourceFingerprint) -> Result<(), PlayerError> {
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _root = self.root.guard()?;
        self.cleanup_temporary()?;
        remove_if_present(&self.root, &self.name(fingerprint))
    }

    pub fn clear_all(&self) -> Result<(), PlayerError> {
        let _guard = self
            .operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _root = self.root.guard()?;
        self.cleanup_temporary()?;
        for entry in entries(&self.root)? {
            remove_if_present(&self.root, &entry.name)?;
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
        let _root = self.root.guard()?;
        self.cleanup_temporary()?;
        let mut entries = entries(&self.root)?;
        entries.sort_by_key(|entry| entry.modified);
        let mut bytes = total_bytes(&entries)?;
        while bytes > capacity_bytes || entries.len() > self.maximum_entries {
            let entry = entries.remove(0);
            remove_if_present(&self.root, &entry.name)?;
            bytes = bytes.saturating_sub(entry.bytes);
        }
        self.capacity_bytes.store(capacity_bytes, Ordering::Release);
        Ok(())
    }

    fn name(&self, fingerprint: SourceFingerprint) -> PathBuf {
        PathBuf::from(format!("{}.{}", fingerprint.hex(), EXTENSION))
    }

    #[cfg(test)]
    fn path(&self, fingerprint: SourceFingerprint) -> PathBuf {
        self.root.path.join(self.name(fingerprint))
    }

    fn cleanup_temporary(&self) -> Result<(), PlayerError> {
        for name in self
            .root
            .names(MAX_RECOVERY_ENTRIES)
            .map_err(|source| io_error("Waveform cache directory cannot be read", source))?
        {
            if name.extension().and_then(|value| value.to_str()) == Some("tmp") {
                remove_if_present(&self.root, &name)?;
            }
        }
        Ok(())
    }

    fn evict(
        &self,
        incoming_bytes: u64,
        retained: Option<SourceFingerprint>,
    ) -> Result<(), PlayerError> {
        let retained = retained.map(|value| self.name(value));
        let mut entries = entries(&self.root)?;
        entries.sort_by_key(|entry| entry.modified);
        let mut bytes = total_bytes(&entries)?;
        while bytes.saturating_add(incoming_bytes) > self.capacity_bytes.load(Ordering::Acquire)
            || entries.len() >= self.maximum_entries
        {
            let Some(index) = entries
                .iter()
                .position(|entry| retained.as_ref() != Some(&entry.name))
            else {
                break;
            };
            let entry = entries.remove(index);
            remove_if_present(&self.root, &entry.name)?;
            bytes = bytes.saturating_sub(entry.bytes);
        }
        Ok(())
    }
}

struct Entry {
    name: PathBuf,
    bytes: u64,
    modified: SystemTime,
}

fn total_bytes(entries: &[Entry]) -> Result<u64, PlayerError> {
    entries.iter().try_fold(0_u64, |total, entry| {
        total
            .checked_add(entry.bytes)
            .ok_or_else(|| error("Waveform cache byte count exceeds its limit"))
    })
}

fn read_record(root: &CacheRoot, name: &Path) -> std::io::Result<Vec<u8>> {
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "waveform cache record is not a bounded regular file",
        )
    };
    let file = root.open(name, CacheFileAccess::Read)?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_RECORD_BYTES {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(invalid());
    }
    Ok(bytes)
}

fn entries(root: &CacheRoot) -> Result<Vec<Entry>, PlayerError> {
    entries_with_limit(root, MAX_RECOVERY_ENTRIES)
}

fn entries_with_limit(root: &CacheRoot, limit: usize) -> Result<Vec<Entry>, PlayerError> {
    let mut entries = Vec::new();
    let directory = root
        .names(limit)
        .map_err(|source| io_error("Waveform cache directory cannot be read", source))?;
    for name in directory {
        if name.extension().and_then(|value| value.to_str()) != Some(EXTENSION) {
            continue;
        }
        let metadata = root
            .metadata(&name)
            .map_err(|source| io_error("Waveform cache metadata is unavailable", source))?;
        if !metadata.regular {
            continue;
        }
        entries.push(Entry {
            name,
            bytes: metadata.bytes,
            modified: metadata.modified,
        });
    }
    Ok(entries)
}

fn valid_peak(peak: &Peak) -> bool {
    peak.minimum.is_finite() && peak.maximum.is_finite() && peak.minimum <= peak.maximum
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
                if !valid_peak(peak) {
                    return Err(error("Waveform cache peak is invalid"));
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
                if !valid_peak(&peak) {
                    return Err(error("Waveform cache peak is invalid"));
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

fn touch(root: &CacheRoot, name: &Path) {
    if let Ok(mut file) = root.open(name, CacheFileAccess::Update) {
        let mut byte = [0];
        if file.read_exact(&mut byte).is_ok() && file.seek(SeekFrom::Start(0)).is_ok() {
            let _ = file.write_all(&byte);
        }
    }
}

fn remove_if_present(root: &CacheRoot, name: &Path) -> Result<(), PlayerError> {
    match root.remove(name) {
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
#[path = "cache_tests.rs"]
mod tests;
