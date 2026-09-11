use crate::source::{BoundedReader, source_error};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::SyncSender;
use stemplayer_domain::{ErrorCode, PlayerError};

const HASH_BLOCK_BYTES: usize = 64 * 1024;

/// Hash a caller-bounded byte slice without file or network access.
/// This shares the existing SHA-256 implementation with build/support evidence.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HashProgress {
    pub file_index: usize,
    pub bytes_hashed: u64,
    pub total_bytes: u64,
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
mod digest_tests {
    use super::sha256_bytes;

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
