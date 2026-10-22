use super::*;

impl WaveformCache {
    pub(super) fn write_guarded(
        &self,
        snapshot: &WaveformSnapshot,
        bytes: &[u8],
    ) -> Result<(), PlayerError> {
        let name = self.name(snapshot.fingerprint);
        match read_record(&self.root, &name) {
            Ok(existing) => {
                if decode(&existing, snapshot.fingerprint, snapshot.generation).is_ok_and(
                    |cached| {
                        cached.sample_rate == snapshot.sample_rate
                            && cached.start_frame == snapshot.start_frame
                            && cached.end_frame_exclusive == snapshot.end_frame_exclusive
                    },
                ) {
                    return Ok(());
                }
                remove_if_present(&self.root, &name)?;
            }
            Err(source) if source.kind() == std::io::ErrorKind::InvalidData => {
                remove_if_present(&self.root, &name)?;
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(io_error(
                    "Waveform cache record cannot be validated",
                    source,
                ));
            }
        }
        self.evict(bytes.len() as u64, Some(snapshot.fingerprint))?;
        let mut file = self
            .root
            .open(&name, CacheFileAccess::Create)
            .map_err(|source| io_error("Waveform cache record cannot be created", source))?;
        let write_result = (|| {
            #[cfg(test)]
            if let Some(kind) = WRITE_FAILURE.get() {
                return Err(io_error(
                    "Waveform cache record cannot be flushed",
                    std::io::Error::from(kind),
                ));
            }
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|source| io_error("Waveform cache record cannot be flushed", source))?;
            let verified = read_record(&self.root, &name)
                .map_err(|source| io_error("Waveform cache record cannot be validated", source))?;
            if verified.as_slice() != bytes {
                return Err(error("Waveform cache record changed while writing"));
            }
            decode(&verified, snapshot.fingerprint, snapshot.generation)?;
            Ok(())
        })();
        drop(file);
        if write_result.is_err() {
            let _ = self.root.remove(&name);
        }
        write_result
    }
}
