use crate::source::{BoundedReader, source_error};
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
