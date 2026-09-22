use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new() -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "stemplayer-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Self::create(path)
    }

    fn create(path: PathBuf) -> io::Result<Self> {
        let mut builder = DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.recursive(false).create(&path)?;
        Ok(Self(path))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn write(&self, name: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        let path = self.path().join(name);
        match fs::remove_file(&path) {
            Ok(()) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?
            .write_all(bytes)?;
        Ok(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_directories_are_never_reused() {
        let directory = Scratch::new().unwrap();
        let sentinel = directory.write("input", b"preserve").unwrap();
        assert!(Scratch::create(directory.path().to_path_buf()).is_err());
        assert_eq!(fs::read(sentinel).unwrap(), b"preserve");
    }

    #[test]
    fn replacement_does_not_modify_link_targets() {
        let directory = Scratch::new().unwrap();
        let original = directory.write("original", b"preserve").unwrap();
        let input = directory.path().join("input");
        fs::hard_link(&original, &input).unwrap();
        directory.write("input", b"new input").unwrap();
        assert_eq!(fs::read(original).unwrap(), b"preserve");
        assert_eq!(fs::read(input).unwrap(), b"new input");
    }

    #[cfg(unix)]
    #[test]
    fn directories_are_private_and_symlinks_are_not_followed() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let directory = Scratch::new().unwrap();
        assert_eq!(
            fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let original = directory.write("original", b"preserve").unwrap();
        symlink(&original, directory.path().join("input")).unwrap();
        directory.write("input", b"new input").unwrap();
        assert_eq!(fs::read(original).unwrap(), b"preserve");
        let link = directory.path().join("directory-link");
        symlink(directory.path(), &link).unwrap();
        assert!(Scratch::create(link).is_err());
    }
}
