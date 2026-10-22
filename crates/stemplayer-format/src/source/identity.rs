use super::{io_error, local_file, source_error};
use std::fmt;
use std::fs::{self, File, Metadata};
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;
use stemplayer_domain::{ErrorCode, PlayerError};

#[derive(Clone)]
pub struct FileIdentity {
    version: FileVersion,
    witness: Arc<File>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileVersion {
    len: u64,
    modified: SystemTime,
    file_key: (u64, u64),
}

impl PartialEq for FileIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
    }
}

impl Eq for FileIdentity {}

impl fmt::Debug for FileIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileIdentity")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl FileIdentity {
    pub fn open(path: &Path) -> Result<(File, Self), PlayerError> {
        let identity = Self::open_reader(path)?;
        let file = identity
            .witness
            .try_clone()
            .map_err(|error| io_error("Bundle file handle cannot be retained", error))?;
        Ok((file, identity))
    }

    pub fn from_file(file: &File) -> Result<Self, PlayerError> {
        let witness = file
            .try_clone()
            .map_err(|error| io_error("Bundle file handle cannot be retained", error))?;
        Self::from_owned_file(witness)
    }

    pub fn ensure_file(&self, file: &File) -> Result<(), PlayerError> {
        if FileVersion::from_file(file)? != self.version {
            return Err(source_error(
                ErrorCode::BundleInconsistent,
                "A bundle file changed during access",
            ));
        }
        Ok(())
    }

    pub const fn len(&self) -> u64 {
        self.version.len
    }

    pub const fn is_empty(&self) -> bool {
        self.version.len == 0
    }

    pub(super) fn open_reader(path: &Path) -> Result<Self, PlayerError> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| io_error("Bundle file metadata is unavailable", error))?;
        validate_regular_file(&metadata)?;
        let file = local_file::open(path)
            .map_err(|error| io_error("Bundle file cannot be opened", error))?;
        Self::from_owned_file(file)
    }

    pub(super) fn file(&self) -> &File {
        &self.witness
    }

    pub(super) fn ensure_unchanged(&self) -> Result<(), PlayerError> {
        let metadata = self
            .witness
            .metadata()
            .map_err(|error| io_error("Bundle file metadata is unavailable", error))?;
        validate_regular_file(&metadata)?;
        let modified = metadata
            .modified()
            .map_err(|error| io_error("Bundle file modification time is unavailable", error))?;
        if metadata.len() != self.version.len || modified != self.version.modified {
            return Err(source_error(
                ErrorCode::BundleInconsistent,
                "A bundle file changed during access",
            ));
        }
        Ok(())
    }

    fn from_owned_file(file: File) -> Result<Self, PlayerError> {
        Ok(Self {
            version: FileVersion::from_file(&file)?,
            witness: Arc::new(file),
        })
    }
}

impl FileVersion {
    fn from_file(file: &File) -> Result<Self, PlayerError> {
        let metadata = file
            .metadata()
            .map_err(|error| io_error("Bundle file metadata is unavailable", error))?;
        validate_regular_file(&metadata)?;
        Ok(Self {
            len: metadata.len(),
            modified: metadata
                .modified()
                .map_err(|error| io_error("Bundle file modification time is unavailable", error))?,
            file_key: local_file::file_key(file, &metadata)
                .map_err(|error| io_error("Bundle file identity is unavailable", error))?,
        })
    }
}

fn validate_regular_file(metadata: &Metadata) -> Result<(), PlayerError> {
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(source_error(
            ErrorCode::ManifestPathUnsafe,
            "Referenced bundle entry is not a regular file",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(source_error(
                ErrorCode::ManifestPathUnsafe,
                "Referenced bundle entry is a reparse point",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "stemplayer-identity-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn cloned_identity_retains_the_unlinked_original_object() {
        let dir = TestDir::new();
        let path = dir.0.join("source");
        fs::write(&path, b"original").unwrap();
        let identity = FileIdentity::open_reader(&path).unwrap();
        let cloned = identity.clone();
        drop(identity);
        let retired = dir.0.join("retired");
        fs::rename(&path, &retired).unwrap();
        fs::remove_file(retired).unwrap();
        fs::write(&path, b"replaced").unwrap();
        let mut bytes = [0; 8];
        cloned.file().read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"original");
        assert_ne!(cloned, FileIdentity::open_reader(&path).unwrap());
        assert!(!format!("{cloned:?}").contains(path.to_string_lossy().as_ref()));
    }

    #[test]
    fn native_open_rejects_a_link_substituted_after_metadata_validation() {
        let dir = TestDir::new();
        let target = dir.0.join("target");
        let path = dir.0.join("source");
        fs::write(&target, b"target").unwrap();
        fs::write(&path, b"source").unwrap();
        validate_regular_file(&fs::symlink_metadata(&path).unwrap()).unwrap();
        fs::remove_file(&path).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &path).unwrap();
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_file(&target, &path) {
            assert!(matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
            ));
            return;
        }
        if let Ok(file) = local_file::open(&path) {
            assert_eq!(
                FileIdentity::from_file(&file).unwrap_err().code,
                ErrorCode::ManifestPathUnsafe
            );
        }
    }
}
