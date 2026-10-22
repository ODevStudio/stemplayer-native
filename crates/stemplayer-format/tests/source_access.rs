use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use stemplayer_domain::ErrorCode;
use stemplayer_format::{BoundedReader, FileIdentity};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "stemplayer-source-access-{}-{}",
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
fn identities_are_captured_and_checked_on_opened_handles() {
    let dir = TestDir::new();
    let path = dir.0.join("source");
    fs::write(&path, b"original").unwrap();
    let (file, identity) = FileIdentity::open(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::rename(&path, dir.0.join("original")).unwrap();
    fs::write(&path, b"replaced").unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    let (replacement, replacement_identity) = FileIdentity::open(&path).unwrap();
    assert_eq!(identity.len(), replacement_identity.len());
    assert_ne!(identity, replacement_identity);
    assert_eq!(identity, FileIdentity::from_file(&file).unwrap());
    identity.ensure_file(&file).unwrap();
    assert_eq!(
        identity.ensure_file(&replacement).unwrap_err().code,
        ErrorCode::BundleInconsistent
    );
}

#[test]
fn retained_reader_does_not_follow_a_replaced_path() {
    let dir = TestDir::new();
    let path = dir.0.join("source");
    fs::write(&path, b"original").unwrap();
    let mut reader = BoundedReader::open(&path).unwrap();
    fs::rename(&path, dir.0.join("original")).unwrap();
    fs::write(&path, b"replaced").unwrap();
    let mut bytes = [0; 8];
    reader
        .read_exact_at(0, &mut bytes, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(&bytes, b"original");
}

#[test]
fn symbolic_links_are_not_opened_as_local_sources() {
    let dir = TestDir::new();
    let target = dir.0.join("target");
    let link = dir.0.join("link");
    fs::write(&target, b"target").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_file(&target, &link) {
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
        ));
        return;
    }
    assert_eq!(
        FileIdentity::open(&link).unwrap_err().code,
        ErrorCode::ManifestPathUnsafe
    );
}
