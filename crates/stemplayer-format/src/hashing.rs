use crate::source::{BoundedReader, FileIdentity, source_error};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::SyncSender;
use stemplayer_domain::{ErrorCode, PlayerError};

const HASH_BLOCK_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HashProgress {
    pub file_index: usize,
    pub bytes_hashed: u64,
    pub total_bytes: u64,
}

pub(crate) fn verify_sha256_with_identity(
    reader: BoundedReader,
    identity: &FileIdentity,
    expected: &str,
    file_index: usize,
    cancelled: &AtomicBool,
    progress: Option<&SyncSender<HashProgress>>,
) -> Result<u64, PlayerError> {
    reader.ensure_identity(identity)?;
    verify_sha256(reader, expected, file_index, cancelled, progress)
}

pub fn verify_sha256(
    mut reader: BoundedReader,
    expected: &str,
    file_index: usize,
    cancelled: &AtomicBool,
    progress: Option<&SyncSender<HashProgress>>,
) -> Result<u64, PlayerError> {
    let total = reader.identity().len();
    let mut offset = 0_u64;
    let mut buffer = vec![0_u8; HASH_BLOCK_BYTES];
    let mut hasher = Sha256::new();
    while offset < total {
        let count = usize::try_from((total - offset).min(HASH_BLOCK_BYTES as u64)).unwrap();
        reader.read_exact_at(offset, &mut buffer[..count], cancelled)?;
        hasher.update(&buffer[..count]);
        offset += count as u64;
        if let Some(sender) = progress {
            let _ = sender.try_send(HashProgress {
                file_index,
                bytes_hashed: offset,
                total_bytes: total,
            });
        }
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual != expected {
        return Err(source_error(
            ErrorCode::ManifestSha256Mismatch,
            "Bundle file SHA-256 does not match the manifest",
        ));
    }
    Ok(reader.bytes_read())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn hash_matching_replacement_is_rejected_before_verification() {
        let root = std::env::temp_dir().join(format!(
            "stemplayer-hash-identity-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("track");
        fs::write(&path, b"inspected").unwrap();
        let identity = BoundedReader::open(&path).unwrap().identity().clone();
        fs::write(&path, b"replacement matching the declared hash").unwrap();
        let expected = format!(
            "{:x}",
            Sha256::digest(b"replacement matching the declared hash")
        );
        let result = verify_sha256_with_identity(
            BoundedReader::open(&path).unwrap(),
            &identity,
            &expected,
            0,
            &AtomicBool::new(false),
            None,
        );
        fs::remove_dir_all(root).unwrap();
        assert_eq!(result.unwrap_err().code, ErrorCode::BundleInconsistent);
    }
}
