use std::collections::BTreeMap;
use std::fs::{self, File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;
use stemplayer_domain::{ErrorCode, PlayerError};
use unicode_normalization::UnicodeNormalization;

pub const MAX_RANGE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileIdentity {
    len: u64,
    modified: Option<SystemTime>,
}

impl FileIdentity {
    fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }

    pub const fn len(&self) -> u64 {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

pub struct SelectedFile {
    basename: String,
    path: PathBuf,
}

impl SelectedFile {
    pub fn new(basename: String, path: PathBuf) -> Self {
        Self { basename, path }
    }
}

pub struct SourceGroup {
    display_name: String,
    files: BTreeMap<String, SelectedFile>,
    manifest_key: String,
}

impl SourceGroup {
    pub fn new(
        display_name: String,
        selected_files: Vec<SelectedFile>,
    ) -> Result<Self, PlayerError> {
        let mut files = BTreeMap::new();
        for selected in selected_files {
            let Ok(key) = normalized_key(&selected.basename) else {
                continue;
            };
            if files.insert(key, selected).is_some() {
                return Err(source_error(
                    ErrorCode::ManifestFileAmbiguous,
                    "Selected files contain a normalized name collision",
                ));
            }
        }
        let manifest_key = normalized_key("manifest.json")?;
        if !files.contains_key(&manifest_key) {
            return Err(source_error(
                ErrorCode::ManifestFileMissing,
                "The selected source has no manifest.json",
            ));
        }
        Ok(Self {
            display_name,
            files,
            manifest_key,
        })
    }

    pub fn manifest_reader(&self) -> Result<BoundedReader, PlayerError> {
        self.reader_by_key(&self.manifest_key)
    }

    pub fn reader(&self, basename: &str) -> Result<BoundedReader, PlayerError> {
        let key = normalized_key(basename)?;
        self.reader_by_key(&key)
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    fn reader_by_key(&self, key: &str) -> Result<BoundedReader, PlayerError> {
        let selected = self.files.get(key).ok_or_else(|| {
            source_error(
                ErrorCode::ManifestFileMissing,
                "A referenced bundle file is missing",
            )
        })?;
        BoundedReader::open(&selected.path)
    }
}

pub struct BoundedReader {
    file: File,
    identity: FileIdentity,
    bytes_read: u64,
}

impl BoundedReader {
    pub fn open(path: &Path) -> Result<Self, PlayerError> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| io_error("Bundle file metadata is unavailable", error))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(source_error(
                ErrorCode::ManifestPathUnsafe,
                "Referenced bundle entry is not a regular file",
            ));
        }
        let file =
            File::open(path).map_err(|error| io_error("Bundle file cannot be opened", error))?;
        Ok(Self {
            file,
            identity: FileIdentity::from_metadata(&metadata),
            bytes_read: 0,
        })
    }

    pub fn identity(&self) -> &FileIdentity {
        &self.identity
    }

    pub const fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    pub fn read_exact_at(
        &mut self,
        offset: u64,
        buffer: &mut [u8],
        cancelled: &AtomicBool,
    ) -> Result<(), PlayerError> {
        if buffer.len() > MAX_RANGE_BYTES {
            return Err(source_error(
                ErrorCode::ValueOutOfRange,
                "Requested source range exceeds the bounded read limit",
            ));
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(source_error(
                ErrorCode::BundleAborted,
                "Source operation was cancelled",
            ));
        }
        let end = offset
            .checked_add(buffer.len() as u64)
            .ok_or_else(|| source_error(ErrorCode::WavMalformed, "Source range overflowed"))?;
        if end > self.identity.len {
            return Err(source_error(
                ErrorCode::WavDataTruncated,
                "Source range is truncated",
            ));
        }
        self.ensure_unchanged()?;
        self.file
            .seek(SeekFrom::Start(offset))
            .and_then(|_| self.file.read_exact(buffer))
            .map_err(|error| io_error("Bundle range cannot be read", error))?;
        self.bytes_read = self.bytes_read.saturating_add(buffer.len() as u64);
        self.ensure_unchanged()
    }

    pub fn ensure_identity(&self, expected: &FileIdentity) -> Result<(), PlayerError> {
        if &self.identity != expected {
            return Err(source_error(
                ErrorCode::BundleInconsistent,
                "A bundle file changed after validation",
            ));
        }
        Ok(())
    }

    fn ensure_unchanged(&self) -> Result<(), PlayerError> {
        let metadata = self
            .file
            .metadata()
            .map_err(|error| io_error("Bundle file metadata is unavailable", error))?;
        if FileIdentity::from_metadata(&metadata) != self.identity {
            return Err(source_error(
                ErrorCode::BundleInconsistent,
                "A bundle file changed during access",
            ));
        }
        Ok(())
    }
}

pub fn safe_basename(name: &str) -> Result<(), PlayerError> {
    let nfc = name.nfc().collect::<String>();
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    if name.is_empty()
        || name != nfc
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':'])
        || name.chars().any(char::is_control)
        || name.ends_with(['.', ' '])
        || Path::new(name).is_absolute()
        || reserved
    {
        return Err(source_error(
            ErrorCode::ManifestPathUnsafe,
            "Manifest filename is not a safe normalized basename",
        ));
    }
    Ok(())
}

pub fn normalized_key(name: &str) -> Result<String, PlayerError> {
    safe_basename(name)?;
    Ok(name.to_lowercase().nfc().collect::<String>())
}

pub(crate) fn source_error(code: ErrorCode, summary: &str) -> PlayerError {
    PlayerError::new(code, summary).with_user_action("Choose a valid Studio WAV bundle")
}

fn io_error(summary: &str, error: std::io::Error) -> PlayerError {
    source_error(ErrorCode::BundleInconsistent, summary).with_source(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_names_fail_closed() {
        for name in [
            "",
            ".",
            "..",
            "../bass.wav",
            "C:track.wav",
            "\\\\server\\track.wav",
            "https:track.wav",
            "NUL.wav",
            "track.wav.",
            "line\nbreak.wav",
            "e\u{301}.wav",
        ] {
            assert!(safe_basename(name).is_err(), "{name:?}");
        }
        assert!(safe_basename("\u{e9}.wav").is_ok());
        assert_eq!(normalized_key("DRUMS.WAV").unwrap(), "drums.wav");
        assert_eq!(
            normalized_key("\u{c9}.WAV").unwrap(),
            normalized_key("\u{e9}.wav").unwrap()
        );
    }
    #[test]
    fn unsafe_unreferenced_entries_are_ignored() {
        let group = SourceGroup::new(
            "test".to_owned(),
            vec![
                SelectedFile::new("manifest.json".to_owned(), PathBuf::from("manifest.json")),
                SelectedFile::new("../ignored".to_owned(), PathBuf::from("ignored")),
            ],
        )
        .unwrap();
        assert!(group.files.contains_key("manifest.json"));
        assert_eq!(group.files.len(), 1);
    }
}
