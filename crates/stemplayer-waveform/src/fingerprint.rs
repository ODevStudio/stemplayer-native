use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::UNIX_EPOCH;
use stemplayer_domain::{ErrorCode, PlayerError};

const RANGE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SourceFingerprint([u8; 32]);

impl SourceFingerprint {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn from_evidence(evidence: &[SourceEvidence]) -> Result<Self, PlayerError> {
        if evidence.is_empty() || evidence.len() > 16 {
            return Err(error("Source fingerprint evidence count is invalid"));
        }
        let mut hasher = Sha256::new();
        hasher.update(b"stemplayer-waveform-source-v1");
        for item in evidence {
            hasher.update(item.size.to_le_bytes());
            hasher.update(item.modified_ns.unwrap_or_default().to_le_bytes());
            hasher.update((item.descriptor.len() as u32).to_le_bytes());
            hasher.update(item.descriptor.as_bytes());
            hasher.update(item.head_hash);
            hasher.update(item.tail_hash);
        }
        Ok(Self(hasher.finalize().into()))
    }
}

#[derive(Clone, Debug)]
pub struct SourceEvidence {
    size: u64,
    modified_ns: Option<u128>,
    descriptor: String,
    head_hash: [u8; 32],
    tail_hash: [u8; 32],
}

impl SourceEvidence {
    pub fn from_path(path: &Path, descriptor: impl Into<String>) -> Result<Self, PlayerError> {
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|source| io_error("Source fingerprint metadata is unavailable", source))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(error("Source fingerprint requires a regular file"));
        }
        let descriptor = descriptor.into();
        if descriptor.len() > 1_024 {
            return Err(error("Source fingerprint descriptor is too large"));
        }
        let mut file = File::open(path)
            .map_err(|source| io_error("Source fingerprint file cannot be opened", source))?;
        let size = metadata.len();
        let head = read_range(&mut file, 0, size.min(RANGE_BYTES as u64) as usize)?;
        let tail_offset = size.saturating_sub(RANGE_BYTES as u64);
        let tail = read_range(&mut file, tail_offset, (size - tail_offset) as usize)?;
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos());
        Ok(Self {
            size,
            modified_ns,
            descriptor,
            head_hash: Sha256::digest(head).into(),
            tail_hash: Sha256::digest(tail).into(),
        })
    }
}

fn read_range(file: &mut File, offset: u64, length: usize) -> Result<Vec<u8>, PlayerError> {
    let mut bytes = vec![0; length];
    file.seek(SeekFrom::Start(offset))
        .and_then(|_| file.read_exact(&mut bytes))
        .map_err(|source| io_error("Source fingerprint range cannot be read", source))?;
    Ok(bytes)
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

    #[test]
    fn fingerprint_does_not_depend_on_filename() {
        let item = SourceEvidence {
            size: 4,
            modified_ns: None,
            descriptor: "drums:pcm-f32:48000:2".to_owned(),
            head_hash: [1; 32],
            tail_hash: [2; 32],
        };
        let first = SourceFingerprint::from_evidence(std::slice::from_ref(&item)).unwrap();
        let second = SourceFingerprint::from_evidence(&[item]).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.hex().len(), 64);
    }
}
