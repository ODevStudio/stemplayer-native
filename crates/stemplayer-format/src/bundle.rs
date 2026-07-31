use crate::hashing::{HashProgress, verify_sha256};
use crate::manifest::{ManifestTrack, StudioManifest, parse_manifest};
use crate::source::{BoundedReader, SourceGroup, normalized_key, source_error};
use crate::wav::{SampleEncoding, WaveInfo, inspect_wave};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::SyncSender;
use stemplayer_domain::{ErrorCode, PlayerError};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StudioRole {
    Drums,
    Bass,
    Other,
    Vocals,
    Master,
}

impl StudioRole {
    pub const STEMS: [Self; 4] = [Self::Drums, Self::Bass, Self::Other, Self::Vocals];

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "drums" => Some(Self::Drums),
            "bass" => Some(Self::Bass),
            "other" => Some(Self::Other),
            "vocals" => Some(Self::Vocals),
            "master" => Some(Self::Master),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HashStatus {
    Absent,
    Declared,
    Verified,
}

#[derive(Clone, Debug, Serialize)]
pub struct StudioTrackReport {
    pub role: StudioRole,
    pub sample_rate: u32,
    pub channels: u8,
    pub frames: u64,
    pub bits_per_sample: u16,
    pub encoding: SampleEncoding,
    pub hash_status: HashStatus,
    pub bytes_read: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct StudioBundleReport {
    pub schema: &'static str,
    pub schema_version: u8,
    pub display_name: String,
    pub sample_rate: u32,
    pub frames: u64,
    pub duration_microseconds: u64,
    pub has_master: bool,
    pub tracks: Vec<StudioTrackReport>,
    pub ignored_outputs: Vec<String>,
    pub warnings: Vec<String>,
    pub bytes_read: u64,
    pub maximum_open_files: u8,
    pub peak_pending_blocks: u8,
}

pub struct StudioTrack {
    role: StudioRole,
    filename: String,
    info: WaveInfo,
}

impl StudioTrack {
    pub const fn role(&self) -> StudioRole {
        self.role
    }

    pub const fn info(&self) -> &WaveInfo {
        &self.info
    }
}

pub struct StudioBundle {
    group: SourceGroup,
    tracks: Vec<StudioTrack>,
    report: StudioBundleReport,
}

impl StudioBundle {
    pub fn inspect(
        group: SourceGroup,
        verify_hashes: bool,
        cancelled: &AtomicBool,
    ) -> Result<Self, PlayerError> {
        Self::inspect_with_progress(group, verify_hashes, cancelled, None)
    }

    pub fn inspect_with_progress(
        group: SourceGroup,
        verify_hashes: bool,
        cancelled: &AtomicBool,
        progress: Option<&SyncSender<HashProgress>>,
    ) -> Result<Self, PlayerError> {
        let (manifest, manifest_bytes_read) = match group.manifest_reader()? {
            Some(mut manifest_reader) => {
                if manifest_reader.identity().len() > crate::MAX_MANIFEST_BYTES as u64 {
                    return Err(source_error(
                        ErrorCode::ManifestTooLarge,
                        "Manifest exceeds the 1 MiB limit",
                    ));
                }
                let mut manifest_bytes = vec![0_u8; manifest_reader.identity().len() as usize];
                manifest_reader.read_exact_at(0, &mut manifest_bytes, cancelled)?;
                let manifest = parse_manifest(&manifest_bytes)?;
                (manifest, manifest_reader.bytes_read())
            }
            None => (infer_manifest(&group)?, 0),
        };

        let mut names = BTreeSet::new();
        let mut tracks = Vec::with_capacity(manifest.tracks.len());
        let mut reports = Vec::with_capacity(manifest.tracks.len());
        let mut total_bytes_read = manifest_bytes_read;
        for (file_index, declared) in manifest.tracks.iter().enumerate() {
            if !names.insert(normalized_key(&declared.filename)?) {
                return Err(source_error(
                    ErrorCode::ManifestFileAmbiguous,
                    "Multiple roles reference the same normalized filename",
                ));
            }
            let reader = group.reader(&declared.filename)?;
            let info = inspect_wave(reader, cancelled)?;
            validate_declaration(declared, &info)?;
            let mut bytes_read = info.inspection_bytes_read;
            let hash_status = match (&declared.sha256, verify_hashes) {
                (None, _) => HashStatus::Absent,
                (Some(_), false) => HashStatus::Declared,
                (Some(hash), true) => {
                    bytes_read = bytes_read.saturating_add(verify_sha256(
                        group.reader(&declared.filename)?,
                        hash,
                        file_index,
                        cancelled,
                        progress,
                    )?);
                    HashStatus::Verified
                }
            };
            total_bytes_read = total_bytes_read.saturating_add(bytes_read);
            reports.push(StudioTrackReport {
                role: declared.role,
                sample_rate: info.sample_rate,
                channels: 2,
                frames: info.frames,
                bits_per_sample: info.encoding.bits_per_sample(),
                encoding: info.encoding,
                hash_status,
                bytes_read,
            });
            tracks.push(StudioTrack {
                role: declared.role,
                filename: declared.filename.clone(),
                info,
            });
        }
        let first = tracks.first().ok_or_else(|| {
            source_error(
                ErrorCode::ManifestRoleMissing,
                "Bundle has no routed tracks",
            )
        })?;
        if tracks.iter().any(|track| {
            track.info.sample_rate != first.info.sample_rate
                || track.info.frames != first.info.frames
        }) {
            return Err(source_error(
                ErrorCode::BundleInconsistent,
                "Routed WAV tracks do not share sample rate and frame count",
            ));
        }
        let sample_rate = first.info.sample_rate;
        let frames = first.info.frames;
        let warnings = manifest
            .unknown_fields
            .iter()
            .map(|field| format!("Unknown top-level field ignored: {field}"))
            .collect();
        let report = StudioBundleReport {
            schema: "stemplayer-studio-bundle-v1",
            schema_version: manifest.schema_version,
            display_name: group.display_name().to_owned(),
            sample_rate,
            frames,
            duration_microseconds: frames.saturating_mul(1_000_000) / u64::from(sample_rate),
            has_master: tracks.iter().any(|track| track.role == StudioRole::Master),
            tracks: reports,
            ignored_outputs: manifest.ignored_outputs,
            warnings,
            bytes_read: total_bytes_read,
            maximum_open_files: 1,
            peak_pending_blocks: 1,
        };
        Ok(Self {
            group,
            tracks,
            report,
        })
    }

    pub const fn report(&self) -> &StudioBundleReport {
        &self.report
    }

    pub fn track(&self, role: StudioRole) -> Option<&StudioTrack> {
        self.tracks.iter().find(|track| track.role == role)
    }

    pub fn open_track(&self, role: StudioRole) -> Result<BoundedReader, PlayerError> {
        let track = self.track(role).ok_or_else(|| {
            source_error(
                ErrorCode::ManifestRoleMissing,
                "Requested role is not present",
            )
        })?;
        let reader = self.group.reader(&track.filename)?;
        reader.ensure_identity(&track.info.identity)?;
        Ok(reader)
    }

    pub fn fingerprint_inputs(&self) -> Vec<(String, PathBuf)> {
        let mut inputs = self.group.manifest_path().map_or_else(Vec::new, |path| {
            vec![("studio-manifest-v1".to_owned(), path.to_path_buf())]
        });
        inputs.extend(self.tracks.iter().filter_map(|track| {
            self.group.path(&track.filename).map(|path| {
                (
                    format!(
                        "{:?}:{}:{}:{:?}",
                        track.role, track.info.sample_rate, track.info.frames, track.info.encoding
                    ),
                    path.to_path_buf(),
                )
            })
        }));
        inputs
    }
}

fn infer_manifest(group: &SourceGroup) -> Result<StudioManifest, PlayerError> {
    let mut tracks = Vec::new();
    for filename in group.filenames() {
        let Some(role) = inferred_role(filename) else {
            continue;
        };
        if tracks
            .iter()
            .any(|track: &ManifestTrack| track.role == role)
        {
            return Err(source_error(
                ErrorCode::ManifestRoleDuplicate,
                "Inferred WAV bundle contains a role more than once",
            ));
        }
        tracks.push(ManifestTrack {
            role,
            filename: filename.to_owned(),
            sample_rate: None,
            channels: None,
            frames: None,
            subtype: None,
            sha256: None,
        });
    }
    for role in StudioRole::STEMS {
        if !tracks.iter().any(|track| track.role == role) {
            return Err(source_error(
                ErrorCode::ManifestRoleMissing,
                "Inferred WAV bundle is missing a required stem role",
            ));
        }
    }
    Ok(StudioManifest {
        schema_version: 1,
        tracks,
        ignored_outputs: Vec::new(),
        unknown_fields: Vec::new(),
    })
}

fn inferred_role(filename: &str) -> Option<StudioRole> {
    let path = std::path::Path::new(filename);
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
    {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    StudioRole::parse(stem.rsplit('.').next()?.to_ascii_lowercase().as_str())
}

fn validate_declaration(declared: &ManifestTrack, info: &WaveInfo) -> Result<(), PlayerError> {
    if declared
        .sample_rate
        .is_some_and(|value| value != info.sample_rate)
    {
        return Err(source_error(
            ErrorCode::WavSampleRateMismatch,
            "Manifest sample rate does not match the WAV",
        ));
    }
    if declared.channels.is_some_and(|value| value != 2) {
        return Err(source_error(
            ErrorCode::WavChannelsUnsupported,
            "Manifest channels must equal two",
        ));
    }
    if declared.frames.is_some_and(|value| value != info.frames) {
        return Err(source_error(
            ErrorCode::WavFrameCountMismatch,
            "Manifest frame count does not match the WAV",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod inferred_tests {
    use super::*;
    use crate::SelectedFile;

    #[test]
    fn role_suffixed_wav_names_form_a_manifest() {
        let files = [
            "song.drums.wav",
            "song.bass.wav",
            "song.other.wav",
            "song.vocals.wav",
        ]
        .into_iter()
        .map(|name| SelectedFile::new(name.to_owned(), PathBuf::from(name)))
        .collect();
        let group = SourceGroup::inferred("song".to_owned(), files).unwrap();
        let manifest = infer_manifest(&group).unwrap();
        assert_eq!(manifest.tracks.len(), 4);
        assert!(
            manifest
                .tracks
                .iter()
                .any(|track| track.role == StudioRole::Bass)
        );
    }
}
