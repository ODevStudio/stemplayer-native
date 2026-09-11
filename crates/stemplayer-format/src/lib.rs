#![deny(unsafe_code)]

mod bundle;
mod hashing;
mod manifest;
mod mp4;
mod mp4_metadata;
mod mp4_roles;
mod mp4_tables;
mod source;
mod wav;

pub use bundle::{
    HashStatus, StudioBundle, StudioBundleReport, StudioRole, StudioTrack, StudioTrackReport,
};
pub use hashing::{HashProgress, sha256_bytes, verify_sha256};
pub use manifest::{MAX_MANIFEST_BYTES, ManifestTrack, StudioManifest, parse_manifest};
pub use mp4::{MAX_MP4_BYTES, Mp4Edit, Mp4Structure, Mp4TrackStructure, inspect_mp4_structure};
pub use mp4_roles::{
    Mp4RoleInput, ResolvedMp4Track, RoleMetadataSource, resolve_mp4_roles,
    resolve_mp4_structure_roles,
};
pub use mp4_tables::{Mp4Chunk, Mp4PacketIndex, Mp4SampleSizes, Mp4TimingRun};
pub use source::{
    BoundedReader, FileIdentity, MAX_RANGE_BYTES, SelectedFile, SourceGroup, safe_basename,
};
pub use wav::{SampleEncoding, WaveInfo, inspect_wave};
