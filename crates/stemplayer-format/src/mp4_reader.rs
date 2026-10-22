use crate::{BoundedReader, MAX_RANGE_BYTES};
use std::sync::atomic::{AtomicBool, Ordering};
use stemplayer_domain::{ErrorCode, PlayerError};

pub(crate) enum Mp4Reader<'a> {
    File(&'a mut BoundedReader),
    Bytes { bytes: &'a [u8], bytes_read: u64 },
}

impl Mp4Reader<'_> {
    pub(crate) fn len(&self) -> u64 {
        match self {
            Self::File(reader) => reader.identity().len(),
            Self::Bytes { bytes, .. } => bytes.len() as u64,
        }
    }

    pub(crate) fn bytes_read(&self) -> u64 {
        match self {
            Self::File(reader) => reader.bytes_read(),
            Self::Bytes { bytes_read, .. } => *bytes_read,
        }
    }

    pub(crate) fn read_exact_at(
        &mut self,
        offset: u64,
        buffer: &mut [u8],
        cancelled: &AtomicBool,
    ) -> Result<(), PlayerError> {
        match self {
            Self::File(reader) => reader.read_exact_at(offset, buffer, cancelled),
            Self::Bytes { bytes, bytes_read } => {
                if cancelled.load(Ordering::Acquire) {
                    return Err(PlayerError::new(
                        ErrorCode::BundleAborted,
                        "MP4 inspection was cancelled",
                    ));
                }
                let start = usize::try_from(offset).ok();
                let range = start.and_then(|start| {
                    start
                        .checked_add(buffer.len())
                        .and_then(|end| bytes.get(start..end))
                });
                let source = range
                    .filter(|_| buffer.len() <= MAX_RANGE_BYTES)
                    .ok_or_else(|| {
                        PlayerError::new(ErrorCode::ContainerMalformed, "MP4 read range is invalid")
                    })?;
                buffer.copy_from_slice(source);
                *bytes_read = bytes_read.saturating_add(buffer.len() as u64);
                Ok(())
            }
        }
    }
}
