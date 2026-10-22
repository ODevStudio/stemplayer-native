use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::ops::Deref;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use stemplayer_domain::{ErrorCode, PlayerError};
use stemplayer_format::{BoundedReader, FileIdentity, Mp4Structure, inspect_mp4_structure};
use symphonia::core::formats::FormatReader;
use symphonia::core::formats::probe::{Hint, Probe};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{TimeBase, Timestamp};

static NON_MP4_PROBE: LazyLock<Probe> = LazyLock::new(non_mp4_probe);

pub(crate) struct FileAccess {
    file: File,
    pub(crate) identity: FileIdentity,
}

impl FileAccess {
    pub(crate) fn ensure_unchanged(&self) -> Result<(), PlayerError> {
        self.identity.ensure_file(&self.file)
    }
}

struct FileSource(Arc<FileAccess>);

impl Read for FileSource {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.0.ensure_unchanged().map_err(io::Error::other)?;
        let count = (&self.0.file).read(buffer)?;
        self.0.ensure_unchanged().map_err(io::Error::other)?;
        Ok(count)
    }
}

impl Seek for FileSource {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.0.ensure_unchanged().map_err(io::Error::other)?;
        let position = (&self.0.file).seek(position)?;
        self.0.ensure_unchanged().map_err(io::Error::other)?;
        Ok(position)
    }
}

impl MediaSource for FileSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.0.identity.len())
    }
}

pub(crate) struct CancellableSource<T, C> {
    inner: T,
    cancelled: C,
}

impl<T, C: Deref<Target = AtomicBool>> CancellableSource<T, C> {
    pub(crate) fn new(inner: T, cancelled: C) -> Self {
        Self { inner, cancelled }
    }

    fn check(&self) -> io::Result<()> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(io::Error::other("Audio source operation was cancelled"))
        } else {
            Ok(())
        }
    }
}

impl<T: Read, C: Deref<Target = AtomicBool>> Read for CancellableSource<T, C> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        let length = buffer.len().min(stemplayer_format::MAX_RANGE_BYTES);
        let count = self.inner.read(&mut buffer[..length])?;
        self.check()?;
        Ok(count)
    }
}

impl<T: Seek, C: Deref<Target = AtomicBool>> Seek for CancellableSource<T, C> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.check()?;
        let position = self.inner.seek(position)?;
        self.check()?;
        Ok(position)
    }
}

impl<T: MediaSource, C: Deref<Target = AtomicBool> + Send + Sync> MediaSource
    for CancellableSource<T, C>
{
    fn is_seekable(&self) -> bool {
        self.inner.is_seekable()
    }

    fn byte_len(&self) -> Option<u64> {
        self.inner.byte_len()
    }
}

pub(crate) struct ProbedFile<'a> {
    pub(crate) format: Box<dyn FormatReader + 'a>,
    pub(crate) access: Arc<FileAccess>,
    pub(crate) structure: Option<Mp4Structure>,
}

pub(crate) struct Mp4SeekGuard {
    time_base: TimeBase,
    companions: Vec<(TimeBase, u64)>,
}

impl Mp4SeekGuard {
    pub(crate) fn new(
        format: &dyn FormatReader,
        structure: &Mp4Structure,
        track_id: u32,
    ) -> Result<Self, PlayerError> {
        let time_base = format
            .tracks()
            .iter()
            .find(|track| track.id == track_id)
            .and_then(|track| track.time_base)
            .ok_or_else(seek_coverage_error)?;
        let companions = format
            .tracks()
            .iter()
            .filter(|track| track.id != track_id)
            .map(|track| {
                let time_base = track.time_base.ok_or_else(seek_coverage_error)?;
                let duration = structure
                    .tracks
                    .iter()
                    .find(|value| value.track_id == track.id)
                    .and_then(|track| track.media_duration)
                    .ok_or_else(seek_coverage_error)?;
                Ok((time_base, duration))
            })
            .collect::<Result<Vec<_>, PlayerError>>()?;
        Ok(Self {
            time_base,
            companions,
        })
    }

    pub(crate) fn check(&self, target: i64) -> Result<(), PlayerError> {
        let time = self
            .time_base
            .calc_time(Timestamp::new(target))
            .ok_or_else(seek_coverage_error)?;
        for (time_base, duration) in &self.companions {
            let target = time_base
                .calc_timestamp(time)
                .and_then(|value| u64::try_from(value.get()).ok())
                .ok_or_else(seek_coverage_error)?;
            if target >= *duration {
                return Err(seek_coverage_error());
            }
        }
        Ok(())
    }
}

fn seek_coverage_error() -> PlayerError {
    PlayerError::new(
        ErrorCode::CodecUnsupported,
        "MP4 companion tracks do not cover the audio seek range",
    )
}

pub(crate) fn probe_file<'a, C>(
    path: &Path,
    expected: Option<&FileIdentity>,
    require_mp4: bool,
    cancelled: C,
) -> Result<ProbedFile<'a>, PlayerError>
where
    C: Deref<Target = AtomicBool> + Clone + Send + Sync + 'a,
{
    check_cancelled(&cancelled)?;
    let mut reader = BoundedReader::open(path)?;
    if let Some(expected) = expected {
        reader.ensure_identity(expected)?;
    }
    let identity = reader.identity().clone();
    let mut header = [0_u8; 8];
    if identity.len() >= 8 {
        reader.read_exact_at(0, &mut header, &cancelled)?;
    }
    let structure = match inspect_mp4_structure(reader, &cancelled) {
        Ok(structure) => Some(structure),
        Err(error)
            if require_mp4 || error.code == ErrorCode::BundleAborted || &header[4..] == b"ftyp" =>
        {
            return Err(error);
        }
        Err(_) => None,
    };
    let (file, reopened_identity) = FileIdentity::open(path)?;
    if reopened_identity != identity {
        return Err(PlayerError::new(
            ErrorCode::BundleInconsistent,
            "Audio source changed during inspection",
        ));
    }
    let access = Arc::new(FileAccess { file, identity });
    let stream = MediaSourceStream::new(
        Box::new(CancellableSource::new(
            FileSource(Arc::clone(&access)),
            cancelled.clone(),
        )),
        Default::default(),
    );
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let probe = if structure.is_some() {
        symphonia::default::get_probe()
    } else {
        &NON_MP4_PROBE
    };
    let format = probe
        .probe(
            &hint,
            stream,
            Default::default(),
            MetadataOptions::default(),
        )
        .map_err(|_| {
            operation_error(&access, &cancelled, "Audio source cannot be demultiplexed")
        })?;
    access.ensure_unchanged()?;
    check_cancelled(&cancelled)?;
    Ok(ProbedFile {
        format,
        access,
        structure,
    })
}

fn non_mp4_probe() -> Probe {
    use symphonia::default::formats::{AdtsReader, FlacReader, MpaReader, OggReader, WavReader};

    let mut probe = Probe::default();
    probe.register_format::<AdtsReader<'_>>();
    probe.register_format::<FlacReader<'_>>();
    probe.register_format::<MpaReader<'_>>();
    probe.register_format::<OggReader<'_>>();
    probe.register_format::<WavReader<'_>>();
    probe
}

pub(crate) fn check_cancelled(cancelled: &AtomicBool) -> Result<(), PlayerError> {
    if cancelled.load(Ordering::Acquire) {
        Err(PlayerError::new(
            ErrorCode::BundleAborted,
            "Audio source operation was cancelled",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn operation_error(
    access: &FileAccess,
    cancelled: &AtomicBool,
    summary: &str,
) -> PlayerError {
    check_cancelled(cancelled)
        .and_then(|()| access.ensure_unchanged())
        .err()
        .unwrap_or_else(|| PlayerError::new(ErrorCode::DemuxFailed, summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn companion_seek_coverage_uses_converted_exclusive_endpoints() {
        let guard = Mp4SeekGuard {
            time_base: TimeBase::try_from_recip(44_100).unwrap(),
            companions: vec![(TimeBase::try_from_recip(48_000).unwrap(), 48_000)],
        };
        assert!(guard.check(44_099).is_ok());
        assert_eq!(
            guard.check(44_100).unwrap_err().code,
            ErrorCode::CodecUnsupported
        );
        let short = Mp4SeekGuard {
            time_base: TimeBase::try_from_recip(44_100).unwrap(),
            companions: vec![(TimeBase::try_from_recip(44_100).unwrap(), 1)],
        };
        assert!(short.check(0).is_ok());
        assert_eq!(
            short.check(64).unwrap_err().code,
            ErrorCode::CodecUnsupported
        );
    }

    struct CancelDuringIo {
        cancelled: Arc<AtomicBool>,
        operations: usize,
    }

    impl Read for CancelDuringIo {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.operations += 1;
            buffer.fill(0);
            self.cancelled.store(true, Ordering::Release);
            Ok(buffer.len())
        }
    }

    impl Seek for CancelDuringIo {
        fn seek(&mut self, _position: SeekFrom) -> io::Result<u64> {
            self.operations += 1;
            self.cancelled.store(true, Ordering::Release);
            Ok(0)
        }
    }

    #[test]
    fn cancellation_during_a_read_stops_read_exact_without_retrying() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let reader = CancelDuringIo {
            cancelled: Arc::clone(&cancelled),
            operations: 0,
        };
        let mut source = CancellableSource::new(reader, cancelled);
        let mut bytes = vec![0; 128 * 1024];
        assert!(source.read_exact(&mut bytes).is_err());
        assert_eq!(source.inner.operations, 1);
        assert!(source.seek(SeekFrom::Start(0)).is_err());
        assert_eq!(source.inner.operations, 1);
    }

    #[test]
    fn cancellation_during_a_seek_is_not_reported_as_success() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let reader = CancelDuringIo {
            cancelled: Arc::clone(&cancelled),
            operations: 0,
        };
        let mut source = CancellableSource::new(reader, cancelled);
        assert!(source.seek(SeekFrom::Start(0)).is_err());
        assert_eq!(source.inner.operations, 1);
    }
}
