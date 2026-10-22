#[cfg(not(unix))]
use std::fs;
use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf};
#[cfg(unix)]
use std::time::Duration;
use std::time::SystemTime;

use crate::cache_root::{CacheRoot, unsafe_entry};

pub(super) enum CacheFileAccess {
    Read,
    Update,
    Create,
    Lock,
}

pub(super) struct EntryMetadata {
    pub(super) regular: bool,
    pub(super) bytes: u64,
    pub(super) modified: SystemTime,
}

impl CacheRoot {
    pub(super) fn open(&self, name: &Path, access: CacheFileAccess) -> io::Result<File> {
        validate_name(name)?;
        #[cfg(unix)]
        let file = {
            use rustix::fs::{Mode, OFlags, openat};
            let flags = match access {
                CacheFileAccess::Read => OFlags::RDONLY,
                CacheFileAccess::Update => OFlags::RDWR,
                CacheFileAccess::Create => OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
                CacheFileAccess::Lock => OFlags::RDWR | OFlags::CREATE,
            } | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC;
            File::from(openat(
                &self.directory,
                name,
                flags,
                Mode::RUSR | Mode::WUSR,
            )?)
        };
        #[cfg(not(unix))]
        let file = {
            let mut options = fs::OpenOptions::new();
            match access {
                CacheFileAccess::Read => {
                    options.read(true);
                }
                CacheFileAccess::Update => {
                    options.read(true).write(true);
                }
                CacheFileAccess::Create => {
                    options.write(true).create_new(true);
                }
                CacheFileAccess::Lock => {
                    options.read(true).write(true).create(true).truncate(false);
                }
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options
                    .share_mode(match access {
                        CacheFileAccess::Create => 1,
                        _ => 3,
                    })
                    .custom_flags(0x0020_0000)
                    .security_qos_flags(0);
            }
            options.open(self.path.join(name))?
        };
        let metadata = file.metadata()?;
        if unsafe_entry(&metadata) || !metadata.is_file() {
            return Err(io::Error::other("Waveform cache entry is unsafe"));
        }
        Ok(file)
    }

    pub(super) fn names(&self, limit: usize) -> io::Result<Vec<PathBuf>> {
        #[cfg(unix)]
        let names = {
            use std::os::unix::ffi::OsStrExt;
            rustix::fs::Dir::read_from(&self.directory)?.map(|entry| {
                entry
                    .map(|entry| {
                        PathBuf::from(std::ffi::OsStr::from_bytes(entry.file_name().to_bytes()))
                    })
                    .map_err(io::Error::from)
            })
        };
        #[cfg(not(unix))]
        let names = fs::read_dir(&self.path)?
            .map(|entry| entry.map(|entry| PathBuf::from(entry.file_name())));
        let mut bounded = Vec::new();
        for (index, name) in names.take(limit + 3).enumerate() {
            if index == limit + 2 {
                return Err(io::Error::other(
                    "Waveform cache directory exceeds its scan limit",
                ));
            }
            let name = name?;
            if name == Path::new(".") || name == Path::new("..") {
                continue;
            }
            validate_name(&name)?;
            if bounded.len() == limit {
                return Err(io::Error::other(
                    "Waveform cache directory exceeds its scan limit",
                ));
            }
            bounded.push(name);
        }
        Ok(bounded)
    }

    pub(super) fn metadata(&self, name: &Path) -> io::Result<EntryMetadata> {
        validate_name(name)?;
        #[cfg(unix)]
        {
            use rustix::fs::{AtFlags, FileType, statat};
            let metadata = statat(&self.directory, name, AtFlags::SYMLINK_NOFOLLOW)?;
            let modified = SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_secs(
                    u64::try_from(metadata.st_mtime).unwrap_or_default(),
                ))
                .and_then(|time| {
                    time.checked_add(Duration::from_nanos(
                        u64::try_from(i128::from(metadata.st_mtime_nsec)).unwrap_or_default(),
                    ))
                })
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Ok(EntryMetadata {
                regular: FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile,
                bytes: u64::try_from(metadata.st_size)
                    .map_err(|_| io::Error::other("Waveform cache entry size is invalid"))?,
                modified,
            })
        }
        #[cfg(not(unix))]
        {
            let metadata = fs::symlink_metadata(self.path.join(name))?;
            Ok(EntryMetadata {
                regular: !unsafe_entry(&metadata) && metadata.is_file(),
                bytes: metadata.len(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            })
        }
    }

    pub(super) fn remove(&self, name: &Path) -> io::Result<()> {
        validate_name(name)?;
        #[cfg(unix)]
        {
            rustix::fs::unlinkat(&self.directory, name, rustix::fs::AtFlags::empty())?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            fs::remove_file(self.path.join(name))
        }
    }

    #[cfg(not(windows))]
    pub(super) fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        validate_name(from)?;
        validate_name(to)?;
        #[cfg(unix)]
        {
            rustix::fs::renameat(&self.directory, from, &self.directory, to)?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            fs::rename(self.path.join(from), self.path.join(to))
        }
    }
}

fn validate_name(name: &Path) -> io::Result<()> {
    let mut parts = name.components();
    if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() {
        return Err(io::Error::other("Waveform cache entry name is unsafe"));
    }
    Ok(())
}
