use crate::BoundedReader;
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use stemplayer_domain::{ErrorCode, PlayerError};

const MAX_SAMPLES: u32 = 1_000_000;
const MAX_CHUNKS: u32 = 250_000;
const MAX_RUNS: u32 = 250_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "values", rename_all = "snake_case")]
pub enum Mp4SampleSizes {
    Fixed { size: u32, count: u32 },
    Variable(Vec<u32>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Mp4Chunk {
    pub offset: u64,
    pub first_sample: u32,
    pub sample_count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Mp4TimingRun {
    pub sample_count: u32,
    pub sample_delta: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Mp4PacketIndex {
    pub sample_count: u32,
    pub duration: u64,
    pub encoded_bytes: u64,
    pub sample_sizes: Mp4SampleSizes,
    pub chunks: Vec<Mp4Chunk>,
    pub timing: Vec<Mp4TimingRun>,
}

impl Mp4PacketIndex {
    pub(crate) fn retained_bytes(&self) -> usize {
        let sample_bytes = match &self.sample_sizes {
            Mp4SampleSizes::Fixed { .. } => 0,
            Mp4SampleSizes::Variable(sizes) => sizes.len() * size_of::<u32>(),
        };
        sample_bytes
            .saturating_add(self.chunks.len() * size_of::<Mp4Chunk>())
            .saturating_add(self.timing.len() * size_of::<Mp4TimingRun>())
    }

    pub fn packet_range(&self, sample: u32) -> Result<(u64, u32), PlayerError> {
        if sample >= self.sample_count {
            return Err(malformed("MP4 sample index is outside the packet table"));
        }
        let chunk_index = self
            .chunks
            .partition_point(|chunk| chunk.first_sample <= sample)
            .checked_sub(1)
            .ok_or_else(|| malformed("MP4 sample has no containing chunk"))?;
        let chunk = self.chunks[chunk_index];
        if sample >= chunk.first_sample + chunk.sample_count {
            return Err(malformed("MP4 sample has no containing chunk"));
        }
        let mut offset = chunk.offset;
        for index in chunk.first_sample..sample {
            offset = offset
                .checked_add(u64::from(self.sample_sizes.get(index)?))
                .ok_or_else(|| malformed("MP4 packet offset overflowed"))?;
        }
        Ok((offset, self.sample_sizes.get(sample)?))
    }

    pub fn sample_at_or_before(&self, timestamp: u64) -> u32 {
        let mut first_sample = 0_u32;
        let mut first_timestamp = 0_u64;
        for run in &self.timing {
            let run_duration = u64::from(run.sample_count) * u64::from(run.sample_delta);
            if timestamp < first_timestamp.saturating_add(run_duration) {
                let within =
                    timestamp.saturating_sub(first_timestamp) / u64::from(run.sample_delta);
                return first_sample.saturating_add(within as u32);
            }
            first_sample = first_sample.saturating_add(run.sample_count);
            first_timestamp = first_timestamp.saturating_add(run_duration);
        }
        self.sample_count.saturating_sub(1)
    }
}

impl Mp4SampleSizes {
    fn count(&self) -> u32 {
        match self {
            Self::Fixed { count, .. } => *count,
            Self::Variable(sizes) => sizes.len() as u32,
        }
    }

    fn get(&self, index: u32) -> Result<u32, PlayerError> {
        match self {
            Self::Fixed { size, count } if index < *count => Ok(*size),
            Self::Variable(sizes) => sizes
                .get(index as usize)
                .copied()
                .ok_or_else(|| malformed("MP4 sample size is missing")),
            _ => Err(malformed("MP4 sample size is missing")),
        }
    }

    fn encoded_bytes(&self) -> Result<u64, PlayerError> {
        match self {
            Self::Fixed { size, count } => Ok(u64::from(*size) * u64::from(*count)),
            Self::Variable(sizes) => sizes.iter().try_fold(0_u64, |total, size| {
                total
                    .checked_add(u64::from(*size))
                    .ok_or_else(|| malformed("MP4 encoded-byte total overflowed"))
            }),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SampleToChunk {
    first_chunk: u32,
    samples_per_chunk: u32,
    sample_description_index: u32,
}

#[derive(Default)]
pub(crate) struct Mp4TableBuilder {
    sample_sizes: Option<Mp4SampleSizes>,
    chunk_offsets: Option<Vec<u64>>,
    sample_to_chunk: Option<Vec<SampleToChunk>>,
    timing: Option<Vec<Mp4TimingRun>>,
}

impl Mp4TableBuilder {
    pub(crate) fn set_sample_sizes(&mut self, value: Mp4SampleSizes) -> Result<(), PlayerError> {
        replace_once(&mut self.sample_sizes, value, "sample-size")
    }

    pub(crate) fn set_chunk_offsets(&mut self, value: Vec<u64>) -> Result<(), PlayerError> {
        replace_once(&mut self.chunk_offsets, value, "chunk-offset")
    }

    pub(crate) fn set_sample_to_chunk(
        &mut self,
        value: Vec<SampleToChunk>,
    ) -> Result<(), PlayerError> {
        replace_once(&mut self.sample_to_chunk, value, "sample-to-chunk")
    }

    pub(crate) fn set_timing(&mut self, value: Vec<Mp4TimingRun>) -> Result<(), PlayerError> {
        replace_once(&mut self.timing, value, "time-to-sample")
    }

    pub(crate) fn finish(self, file_bytes: u64) -> Result<Mp4PacketIndex, PlayerError> {
        let sample_sizes = self
            .sample_sizes
            .ok_or_else(|| malformed("MP4 audio track has no sample-size table"))?;
        let chunk_offsets = self
            .chunk_offsets
            .filter(|value| !value.is_empty())
            .ok_or_else(|| malformed("MP4 audio track has no chunk offsets"))?;
        let sample_to_chunk = self
            .sample_to_chunk
            .filter(|value| !value.is_empty())
            .ok_or_else(|| malformed("MP4 audio track has no sample-to-chunk table"))?;
        let timing = self
            .timing
            .filter(|value| !value.is_empty())
            .ok_or_else(|| malformed("MP4 audio track has no time-to-sample table"))?;
        let sample_count = sample_sizes.count();
        if sample_count == 0 {
            return Err(malformed("MP4 audio track has no encoded samples"));
        }
        let timing_samples = timing.iter().try_fold(0_u32, |total, run| {
            if run.sample_count == 0 || run.sample_delta == 0 {
                return Err(malformed("MP4 time-to-sample run is invalid"));
            }
            total
                .checked_add(run.sample_count)
                .ok_or_else(|| malformed("MP4 timing sample count overflowed"))
        })?;
        if timing_samples != sample_count {
            return Err(malformed("MP4 timing and sample-size counts disagree"));
        }
        let duration = timing.iter().try_fold(0_u64, |total, run| {
            total
                .checked_add(u64::from(run.sample_count) * u64::from(run.sample_delta))
                .ok_or_else(|| malformed("MP4 media duration overflowed"))
        })?;
        let chunks = build_chunks(&sample_sizes, &chunk_offsets, &sample_to_chunk, file_bytes)?;
        Ok(Mp4PacketIndex {
            sample_count,
            duration,
            encoded_bytes: sample_sizes.encoded_bytes()?,
            sample_sizes,
            chunks,
            timing,
        })
    }
}

fn build_chunks(
    sizes: &Mp4SampleSizes,
    offsets: &[u64],
    mappings: &[SampleToChunk],
    file_bytes: u64,
) -> Result<Vec<Mp4Chunk>, PlayerError> {
    if mappings[0].first_chunk != 1
        || mappings
            .iter()
            .any(|entry| entry.samples_per_chunk == 0 || entry.sample_description_index == 0)
        || mappings
            .windows(2)
            .any(|pair| pair[0].first_chunk >= pair[1].first_chunk)
    {
        return Err(malformed("MP4 sample-to-chunk table is invalid"));
    }
    let mut chunks = Vec::with_capacity(offsets.len());
    let mut mapping = 0_usize;
    let mut first_sample = 0_u32;
    let mut previous_end = 0_u64;
    for (index, offset) in offsets.iter().copied().enumerate() {
        let chunk_number = index as u32 + 1;
        if mapping + 1 < mappings.len() && mappings[mapping + 1].first_chunk == chunk_number {
            mapping += 1;
        }
        let sample_count = mappings[mapping].samples_per_chunk;
        let last_sample = first_sample
            .checked_add(sample_count)
            .ok_or_else(|| malformed("MP4 chunk sample count overflowed"))?;
        if last_sample > sizes.count() {
            return Err(malformed("MP4 chunk table exceeds the sample count"));
        }
        let chunk_bytes = (first_sample..last_sample).try_fold(0_u64, |total, sample| {
            total
                .checked_add(u64::from(sizes.get(sample)?))
                .ok_or_else(|| malformed("MP4 chunk byte count overflowed"))
        })?;
        let end = offset
            .checked_add(chunk_bytes)
            .filter(|value| *value <= file_bytes)
            .ok_or_else(|| malformed("MP4 chunk range is outside the file"))?;
        if index > 0 && offset < previous_end {
            return Err(malformed("MP4 chunks overlap or run backwards"));
        }
        chunks.push(Mp4Chunk {
            offset,
            first_sample,
            sample_count,
        });
        first_sample = last_sample;
        previous_end = end;
    }
    if first_sample != sizes.count()
        || mappings
            .last()
            .is_some_and(|entry| entry.first_chunk > offsets.len() as u32)
    {
        return Err(malformed("MP4 chunk and sample counts disagree"));
    }
    Ok(chunks)
}

pub(crate) fn read_sample_sizes(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    body: u64,
    end: u64,
) -> Result<Mp4SampleSizes, PlayerError> {
    let header = read_array::<12>(
        reader,
        cancelled,
        body,
        end,
        "MP4 sample sizes are truncated",
    )?;
    if header[0..4] != [0; 4] {
        return Err(malformed("MP4 sample-size flags are unsupported"));
    }
    let fixed_size = u32::from_be_bytes(header[4..8].try_into().unwrap());
    let count = u32::from_be_bytes(header[8..12].try_into().unwrap());
    validate_count(count, MAX_SAMPLES, "sample")?;
    if fixed_size != 0 {
        if body.checked_add(12) != Some(end) {
            return Err(malformed("MP4 fixed sample-size table is invalid"));
        }
        return Ok(Mp4SampleSizes::Fixed {
            size: fixed_size,
            count,
        });
    }
    if body
        .checked_add(12)
        .and_then(|value| value.checked_add(u64::from(count) * 4))
        != Some(end)
    {
        return Err(malformed("MP4 sample-size table range is invalid"));
    }
    let mut sizes = vec![0_u32; count as usize];
    read_u32_entries(reader, cancelled, body + 12, &mut sizes)?;
    if sizes.contains(&0) {
        return Err(malformed("MP4 sample size must be positive"));
    }
    Ok(Mp4SampleSizes::Variable(sizes))
}

pub(crate) fn read_chunk_offsets(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    body: u64,
    end: u64,
    wide: bool,
) -> Result<Vec<u64>, PlayerError> {
    let header = read_array::<8>(
        reader,
        cancelled,
        body,
        end,
        "MP4 chunk offsets are truncated",
    )?;
    if header[..4] != [0; 4] {
        return Err(malformed("MP4 chunk-offset flags are unsupported"));
    }
    let count = u32::from_be_bytes(header[4..8].try_into().unwrap());
    validate_count(count, MAX_CHUNKS, "chunk")?;
    let width = if wide { 8 } else { 4 };
    validate_table_range(body, end, count, width, "chunk-offset")?;
    let mut offsets = Vec::with_capacity(count as usize);
    for index in 0..count {
        let offset = body + 8 + u64::from(index) * u64::from(width);
        offsets.push(if wide {
            u64::from_be_bytes(read_array::<8>(
                reader,
                cancelled,
                offset,
                end,
                "MP4 chunk offset is truncated",
            )?)
        } else {
            u64::from(u32::from_be_bytes(read_array::<4>(
                reader,
                cancelled,
                offset,
                end,
                "MP4 chunk offset is truncated",
            )?))
        });
    }
    Ok(offsets)
}

pub(crate) fn read_sample_to_chunk(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    body: u64,
    end: u64,
) -> Result<Vec<SampleToChunk>, PlayerError> {
    let header = read_array::<8>(
        reader,
        cancelled,
        body,
        end,
        "MP4 sample-to-chunk table is truncated",
    )?;
    if header[..4] != [0; 4] {
        return Err(malformed("MP4 sample-to-chunk flags are unsupported"));
    }
    let count = u32::from_be_bytes(header[4..8].try_into().unwrap());
    validate_count(count, MAX_RUNS, "sample-to-chunk run")?;
    validate_table_range(body, end, count, 12, "sample-to-chunk")?;
    let mut entries = Vec::with_capacity(count as usize);
    for index in 0..count {
        let offset = body + 8 + u64::from(index) * 12;
        let bytes = read_array::<12>(
            reader,
            cancelled,
            offset,
            end,
            "MP4 sample-to-chunk entry is truncated",
        )?;
        entries.push(SampleToChunk {
            first_chunk: u32::from_be_bytes(bytes[..4].try_into().unwrap()),
            samples_per_chunk: u32::from_be_bytes(bytes[4..8].try_into().unwrap()),
            sample_description_index: u32::from_be_bytes(bytes[8..12].try_into().unwrap()),
        });
    }
    Ok(entries)
}

pub(crate) fn read_timing(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    body: u64,
    end: u64,
) -> Result<Vec<Mp4TimingRun>, PlayerError> {
    let header = read_array::<8>(
        reader,
        cancelled,
        body,
        end,
        "MP4 time-to-sample table is truncated",
    )?;
    if header[..4] != [0; 4] {
        return Err(malformed("MP4 time-to-sample flags are unsupported"));
    }
    let count = u32::from_be_bytes(header[4..8].try_into().unwrap());
    validate_count(count, MAX_RUNS, "time-to-sample run")?;
    validate_table_range(body, end, count, 8, "time-to-sample")?;
    let mut entries = Vec::with_capacity(count as usize);
    for index in 0..count {
        let offset = body + 8 + u64::from(index) * 8;
        let bytes = read_array::<8>(
            reader,
            cancelled,
            offset,
            end,
            "MP4 time-to-sample entry is truncated",
        )?;
        entries.push(Mp4TimingRun {
            sample_count: u32::from_be_bytes(bytes[..4].try_into().unwrap()),
            sample_delta: u32::from_be_bytes(bytes[4..8].try_into().unwrap()),
        });
    }
    Ok(entries)
}

fn read_u32_entries(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    mut offset: u64,
    output: &mut [u32],
) -> Result<(), PlayerError> {
    let mut buffer = [0_u8; 64 * 1024];
    for chunk in output.chunks_mut(buffer.len() / 4) {
        let bytes = chunk.len() * 4;
        reader.read_exact_at(offset, &mut buffer[..bytes], cancelled)?;
        for (value, encoded) in chunk.iter_mut().zip(buffer[..bytes].chunks_exact(4)) {
            *value = u32::from_be_bytes(encoded.try_into().unwrap());
        }
        offset += bytes as u64;
    }
    Ok(())
}

fn validate_count(count: u32, maximum: u32, name: &str) -> Result<(), PlayerError> {
    if count == 0 || count > maximum {
        return Err(malformed(&format!(
            "MP4 {name} count is outside the supported limit"
        )));
    }
    Ok(())
}

fn validate_table_range(
    body: u64,
    end: u64,
    count: u32,
    width: u32,
    name: &str,
) -> Result<(), PlayerError> {
    if body
        .checked_add(8)
        .and_then(|value| value.checked_add(u64::from(count) * u64::from(width)))
        != Some(end)
    {
        return Err(malformed(&format!("MP4 {name} table range is invalid")));
    }
    Ok(())
}

fn read_array<const N: usize>(
    reader: &mut BoundedReader,
    cancelled: &AtomicBool,
    offset: u64,
    end: u64,
    summary: &str,
) -> Result<[u8; N], PlayerError> {
    if offset.checked_add(N as u64).is_none_or(|value| value > end) {
        return Err(malformed(summary));
    }
    let mut bytes = [0_u8; N];
    reader.read_exact_at(offset, &mut bytes, cancelled)?;
    Ok(bytes)
}

fn replace_once<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<(), PlayerError> {
    if slot.replace(value).is_some() {
        return Err(malformed(&format!(
            "MP4 track contains multiple {name} tables"
        )));
    }
    Ok(())
}

fn malformed(summary: &str) -> PlayerError {
    PlayerError::new(ErrorCode::ContainerMalformed, summary)
        .with_user_action("Choose a valid .stem.m4a or .stem.mp4 file")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_ranges_timing_and_table_disagreements_are_deterministic() {
        let index = builder(vec![100, 110], 3).finish(1_000).unwrap();
        assert_eq!(index.packet_range(0).unwrap(), (100, 5));
        assert_eq!(index.packet_range(1).unwrap(), (105, 5));
        assert_eq!(index.packet_range(2).unwrap(), (110, 5));
        assert_eq!(index.sample_at_or_before(2_048), 2);
        assert_eq!(
            builder(vec![100, 104], 3).finish(1_000).unwrap_err().code,
            ErrorCode::ContainerMalformed
        );
        assert_eq!(
            builder(vec![100, 110], 2).finish(1_000).unwrap_err().code,
            ErrorCode::ContainerMalformed
        );
    }

    fn builder(offsets: Vec<u64>, timed_samples: u32) -> Mp4TableBuilder {
        Mp4TableBuilder {
            sample_sizes: Some(Mp4SampleSizes::Fixed { size: 5, count: 3 }),
            chunk_offsets: Some(offsets),
            sample_to_chunk: Some(vec![
                SampleToChunk {
                    first_chunk: 1,
                    samples_per_chunk: 2,
                    sample_description_index: 1,
                },
                SampleToChunk {
                    first_chunk: 2,
                    samples_per_chunk: 1,
                    sample_description_index: 1,
                },
            ]),
            timing: Some(vec![Mp4TimingRun {
                sample_count: timed_samples,
                sample_delta: 1_024,
            }]),
        }
    }
}
