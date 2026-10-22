use std::ffi::OsString;
#[cfg(not(unix))]
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};
use stemplayer_domain::{ErrorCode, PlayerError};

use crate::cache_io::CacheFileAccess;

const LOCK_NAME: &str = ".cache-lock";

pub(super) struct CacheRoot {
    pub(super) path: PathBuf,
    pub(super) directory: File,
    _ancestors: Vec<File>,
}

pub(super) struct CacheGuard {
    _lock: File,
}

#[cfg(unix)]
impl Drop for CacheGuard {
    fn drop(&mut self) {
        let _ = self._lock.unlock();
    }
}

impl CacheRoot {
    pub(super) fn new(path: PathBuf) -> Result<Self, PlayerError> {
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir().map_err(root_error)?.join(path)
        };
        if path.components().count() > 128
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(unsafe_root());
        }
        let _original = match fs::symlink_metadata(&path) {
            Ok(metadata) if unsafe_entry(&metadata) => return Err(unsafe_root()),
            Ok(metadata) => Some(metadata),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => return Err(root_error(source)),
        };
        #[cfg(windows)]
        let _original_ancestors = protect_ancestors(&path, true)?;
        let (existing, missing, _expected) = resolve_existing_parent(&path).map_err(root_error)?;
        let path = missing
            .iter()
            .fold(existing.clone(), |parent, name| parent.join(name));
        if path.components().count() > 128 {
            return Err(unsafe_root());
        }
        let parents = protect_ancestors(&path, true)?;
        #[cfg(unix)]
        let directory = open_unix_root(
            &existing,
            &missing,
            if missing.is_empty() {
                _original.as_ref().unwrap_or(&_expected)
            } else {
                &_expected
            },
        )
        .map_err(root_error)?;
        #[cfg(not(unix))]
        let _created_ancestors = create_root(&path, &existing).map_err(root_error)?;
        #[cfg(not(unix))]
        let directory = open_directory(&path).map_err(root_error)?;
        let ancestors = protect_ancestors(&path, false)?;
        drop(parents);
        let root = Self {
            path,
            directory,
            _ancestors: ancestors,
        };
        root.validate()?;
        Ok(root)
    }

    pub(super) fn guard(&self) -> Result<CacheGuard, PlayerError> {
        self.validate()?;
        let lock = self
            .open(Path::new(LOCK_NAME), CacheFileAccess::Lock)
            .map_err(root_error)?;
        lock.try_lock()
            .map_err(|_| PlayerError::new(ErrorCode::AudioDataInvalid, "Waveform cache is busy"))?;
        self.validate()?;
        Ok(CacheGuard { _lock: lock })
    }

    fn validate(&self) -> Result<(), PlayerError> {
        let metadata = fs::symlink_metadata(&self.path).map_err(root_error)?;
        let original = self.directory.metadata().map_err(root_error)?;
        if unsafe_entry(&metadata)
            || !metadata.is_dir()
            || unsafe_entry(&original)
            || !original.is_dir()
        {
            return Err(unsafe_root());
        }
        #[cfg(unix)]
        ensure_same_directory(&metadata, &original).map_err(root_error)?;
        Ok(())
    }
}

fn protect_ancestors(path: &Path, allow_missing: bool) -> Result<Vec<File>, PlayerError> {
    let directories = Vec::new();
    #[cfg(windows)]
    let mut directories = directories;
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(source) if allow_missing && source.kind() == std::io::ErrorKind::NotFound => break,
            Err(source) => return Err(root_error(source)),
        };
        if unsafe_entry(&metadata) || !metadata.is_dir() {
            return Err(unsafe_root());
        }
        #[cfg(windows)]
        directories.push(open_directory(ancestor).map_err(root_error)?);
    }
    Ok(directories)
}

fn resolve_existing_parent(path: &Path) -> std::io::Result<(PathBuf, Vec<OsString>, fs::Metadata)> {
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(existing) {
            Ok(metadata) => {
                if existing == path && unsafe_entry(&metadata) {
                    return Err(std::io::Error::other("Waveform cache root is unsafe"));
                }
                let expected = fs::metadata(existing)?;
                return Ok((
                    fs::canonicalize(existing)?,
                    missing.into_iter().rev().collect(),
                    expected,
                ));
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                missing.push(existing.file_name().ok_or(source)?.to_os_string());
                existing = existing
                    .parent()
                    .ok_or_else(|| std::io::Error::other("Waveform cache parent is unavailable"))?;
            }
            Err(source) => return Err(source),
        }
    }
}

#[cfg(unix)]
fn open_unix_root(
    existing: &Path,
    missing: &[OsString],
    expected: &fs::Metadata,
) -> std::io::Result<File> {
    use rustix::fs::{Mode, OFlags, mkdirat, openat};
    let mut directory = open_directory(existing)?;
    ensure_same_directory(expected, &directory.metadata()?)?;
    for name in missing {
        match mkdirat(&directory, name, Mode::RWXU) {
            Ok(()) => {}
            Err(rustix::io::Errno::EXIST) => {}
            Err(source) => return Err(source.into()),
        }
        directory = File::from(openat(
            &directory,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
    }
    {
        use std::os::unix::fs::PermissionsExt;
        directory.set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    Ok(directory)
}

#[cfg(unix)]
fn ensure_same_directory(expected: &fs::Metadata, actual: &fs::Metadata) -> std::io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    if unsafe_entry(expected)
        || !expected.is_dir()
        || expected.dev() != actual.dev()
        || expected.ino() != actual.ino()
    {
        return Err(std::io::Error::other("Waveform cache directory changed"));
    }
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn create_root(path: &Path, existing: &Path) -> std::io::Result<Vec<File>> {
    path.ancestors()
        .take_while(|ancestor| *ancestor != existing)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|directory| {
            match fs::create_dir(directory) {
                Ok(()) => {}
                Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(source) => return Err(source),
            }
            open_directory(directory)
        })
        .collect()
}

fn open_directory(path: &Path) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use rustix::fs::{Mode, OFlags, open};
        Ok(File::from(open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?))
    }
    #[cfg(not(unix))]
    {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const FILE_SHARE_READ: u32 = 1;
            const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
            const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
            options
                .share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .security_qos_flags(0);
        }
        let file = options.open(path)?;
        let metadata = file.metadata()?;
        if unsafe_entry(&metadata) || !metadata.is_dir() {
            return Err(std::io::Error::other("Waveform cache directory is unsafe"));
        }
        Ok(file)
    }
}

pub(super) fn unsafe_entry(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn unsafe_root() -> PlayerError {
    PlayerError::new(ErrorCode::AudioDataInvalid, "Waveform cache root is unsafe")
}

fn root_error(source: std::io::Error) -> PlayerError {
    PlayerError::new(
        ErrorCode::AudioDataInvalid,
        "Waveform cache root is unavailable",
    )
    .with_source(source)
}

#[cfg(all(test, unix))]
#[test]
fn dropping_a_cache_guard_unlocks_before_duplicated_descriptors_close() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "stemplayer-cache-lock-drop-{}-{nonce}",
        std::process::id()
    ));
    let lock = File::create_new(&path).unwrap();
    lock.try_lock().unwrap();
    let duplicated = lock.try_clone().unwrap();
    let guard = CacheGuard { _lock: lock };
    drop(guard);
    let contender = File::open(&path).unwrap();
    let acquired = contender.try_lock();
    drop(contender);
    drop(duplicated);
    fs::remove_file(path).unwrap();
    assert!(acquired.is_ok(), "A dropped cache guard retained its lock");
}
