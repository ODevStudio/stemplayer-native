use serde::Serialize;
use stemplayer_domain::{ErrorCode, PlayerError, StemRole, TrackRole};
use unicode_normalization::UnicodeNormalization;

use crate::Mp4Structure;

const MAX_TRACK_NAME_BYTES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mp4RoleInput {
    pub track_id: u32,
    pub native_role: Option<String>,
    pub name: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleMetadataSource {
    NativeInstruments,
    TrackName,
    StandardOrder,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResolvedMp4Track {
    pub track_id: u32,
    pub role: TrackRole,
    pub source: RoleMetadataSource,
}

pub fn resolve_mp4_roles(tracks: &[Mp4RoleInput]) -> Result<Vec<ResolvedMp4Track>, PlayerError> {
    if !matches!(tracks.len(), 4 | 5) {
        return Err(role_error(
            ErrorCode::TrackCountUnsupported,
            "Stem container must expose exactly four or five routed audio tracks",
        ));
    }
    if tracks.iter().enumerate().any(|(index, track)| {
        tracks[..index]
            .iter()
            .any(|other| other.track_id == track.track_id)
    }) {
        return Err(role_error(
            ErrorCode::TrackRoleAmbiguous,
            "Stem container contains duplicate track identifiers",
        ));
    }
    validate_lengths(tracks)?;

    let resolved = if tracks.iter().any(|track| track.native_role.is_some()) {
        resolve_native(tracks)?
    } else {
        resolve_names_or_order(tracks)?
    };
    validate_role_set(&resolved)?;
    Ok(resolved)
}

pub fn resolve_mp4_structure_roles(
    structure: &Mp4Structure,
) -> Result<Vec<ResolvedMp4Track>, PlayerError> {
    let tracks = structure
        .tracks
        .iter()
        .filter(|track| track.is_audio)
        .collect::<Vec<_>>();
    let native_roles = match structure.native_roles.as_deref() {
        None => vec![None; tracks.len()],
        Some(roles) if roles.len() == tracks.len() => roles.iter().map(Some).collect(),
        Some(roles) if roles.len() == 4 && tracks.len() == 5 => std::iter::once(None)
            .chain(roles.iter().map(Some))
            .collect(),
        Some(_) => {
            return Err(role_error(
                ErrorCode::TrackRoleAmbiguous,
                "Native stem metadata does not match the audio tracks",
            ));
        }
    };
    let inputs = tracks
        .into_iter()
        .zip(native_roles)
        .enumerate()
        .map(|(index, (track, native_role))| Mp4RoleInput {
            track_id: track.track_id,
            native_role: native_role.map(ToOwned::to_owned).or_else(|| {
                (index == 0 && structure.native_roles.is_some()).then(|| "master".to_owned())
            }),
            name: track.name.clone(),
        })
        .collect::<Vec<_>>();
    resolve_mp4_roles(&inputs)
}

fn resolve_native(tracks: &[Mp4RoleInput]) -> Result<Vec<ResolvedMp4Track>, PlayerError> {
    if tracks.iter().any(|track| track.native_role.is_none()) {
        return Err(role_error(
            ErrorCode::TrackRoleAmbiguous,
            "Native stem role metadata is incomplete",
        ));
    }
    tracks
        .iter()
        .map(|track| {
            let role = parse_role(track.native_role.as_deref().unwrap()).ok_or_else(|| {
                role_error(
                    ErrorCode::MetadataMalformed,
                    "Native stem role metadata is invalid",
                )
            })?;
            if track
                .name
                .as_deref()
                .and_then(parse_role)
                .is_some_and(|named| named != role)
            {
                return Err(role_error(
                    ErrorCode::TrackRoleAmbiguous,
                    "Native stem role metadata disagrees with the track name",
                ));
            }
            Ok(ResolvedMp4Track {
                track_id: track.track_id,
                role,
                source: RoleMetadataSource::NativeInstruments,
            })
        })
        .collect()
}

fn resolve_names_or_order(tracks: &[Mp4RoleInput]) -> Result<Vec<ResolvedMp4Track>, PlayerError> {
    let named = tracks
        .iter()
        .map(|track| track.name.as_deref().and_then(parse_role))
        .collect::<Vec<_>>();
    if named.iter().any(Option::is_some) {
        if named.iter().any(Option::is_none) {
            return Err(role_error(
                ErrorCode::TrackRoleAmbiguous,
                "Track names provide only a partial stem role mapping",
            ));
        }
        return tracks
            .iter()
            .zip(named)
            .map(|(track, role)| {
                Ok(ResolvedMp4Track {
                    track_id: track.track_id,
                    role: role.unwrap(),
                    source: RoleMetadataSource::TrackName,
                })
            })
            .collect();
    }
    if tracks.len() != 5 {
        return Err(role_error(
            ErrorCode::NotAStemFile,
            "Unnamed tracks do not match the qualified five-track layout",
        ));
    }
    let order = [
        TrackRole::Master,
        TrackRole::Stem(StemRole::Drums),
        TrackRole::Stem(StemRole::Bass),
        TrackRole::Stem(StemRole::Other),
        TrackRole::Stem(StemRole::Vocals),
    ];
    Ok(tracks
        .iter()
        .zip(order)
        .map(|(track, role)| ResolvedMp4Track {
            track_id: track.track_id,
            role,
            source: RoleMetadataSource::StandardOrder,
        })
        .collect())
}

fn validate_lengths(tracks: &[Mp4RoleInput]) -> Result<(), PlayerError> {
    if tracks.iter().any(|track| {
        track
            .native_role
            .iter()
            .chain(track.name.iter())
            .any(|value| value.len() > MAX_TRACK_NAME_BYTES)
    }) {
        return Err(role_error(
            ErrorCode::MetadataTooLarge,
            "Stem role metadata exceeds the supported size",
        ));
    }
    Ok(())
}

fn validate_role_set(tracks: &[ResolvedMp4Track]) -> Result<(), PlayerError> {
    let count = |role| tracks.iter().filter(|track| track.role == role).count();
    let stems_are_unique = StemRole::ALL
        .into_iter()
        .all(|role| count(TrackRole::Stem(role)) == 1);
    if !stems_are_unique || count(TrackRole::Master) > 1 {
        return Err(role_error(
            ErrorCode::TrackRoleAmbiguous,
            "Stem roles are missing or duplicated",
        ));
    }
    Ok(())
}

fn parse_role(value: &str) -> Option<TrackRole> {
    let normalized = value.nfkc().collect::<String>().trim().to_lowercase();
    match normalized.as_str() {
        "master" => Some(TrackRole::Master),
        "drums" => Some(TrackRole::Stem(StemRole::Drums)),
        "bass" => Some(TrackRole::Stem(StemRole::Bass)),
        "other" => Some(TrackRole::Stem(StemRole::Other)),
        "vocals" => Some(TrackRole::Stem(StemRole::Vocals)),
        _ => None,
    }
}

fn role_error(code: ErrorCode, summary: &str) -> PlayerError {
    PlayerError::new(code, summary)
        .with_user_action("Choose a stem container with unambiguous track roles")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hierarchy_resolves_native_names_and_five_track_order() {
        let named = inputs(&["DRUMS", "Bass", "Other", "Vocals"]);
        assert!(
            resolve_mp4_roles(&named)
                .unwrap()
                .iter()
                .all(|track| track.source == RoleMetadataSource::TrackName)
        );

        let unnamed = (0..5)
            .map(|index| Mp4RoleInput {
                track_id: index,
                native_role: None,
                name: Some("unknown".to_owned()),
            })
            .collect::<Vec<_>>();
        let ordered = resolve_mp4_roles(&unnamed).unwrap();
        assert_eq!(ordered[0].role, TrackRole::Master);
        assert!(
            ordered
                .iter()
                .all(|track| track.source == RoleMetadataSource::StandardOrder)
        );

        let native = named
            .into_iter()
            .zip(["drums", "bass", "other", "vocals"])
            .map(|(track, role)| Mp4RoleInput {
                native_role: Some(role.to_owned()),
                ..track
            })
            .collect::<Vec<_>>();
        assert!(
            resolve_mp4_roles(&native)
                .unwrap()
                .iter()
                .all(|track| track.source == RoleMetadataSource::NativeInstruments)
        );
    }

    #[test]
    fn ambiguity_partial_metadata_and_duplicates_fail_closed() {
        let mut partial = inputs(&["drums", "bass", "mystery", "vocals"]);
        assert_eq!(
            resolve_mp4_roles(&partial).unwrap_err().code,
            ErrorCode::TrackRoleAmbiguous
        );

        partial[0].native_role = Some("drums".to_owned());
        assert_eq!(
            resolve_mp4_roles(&partial).unwrap_err().code,
            ErrorCode::TrackRoleAmbiguous
        );

        let duplicate = inputs(&["drums", "drums", "other", "vocals"]);
        assert_eq!(
            resolve_mp4_roles(&duplicate).unwrap_err().code,
            ErrorCode::TrackRoleAmbiguous
        );
    }

    #[test]
    fn four_native_entries_map_after_an_implicit_master() {
        let mut tracks = inputs(&["unknown", "unknown", "unknown", "unknown", "unknown"]);
        for (track, role) in tracks
            .iter_mut()
            .skip(1)
            .zip(["drums", "bass", "other", "vocals"])
        {
            track.native_role = Some(role.to_owned());
        }
        tracks[0].native_role = Some("master".to_owned());
        let resolved = resolve_mp4_roles(&tracks).unwrap();
        assert_eq!(resolved[0].role, TrackRole::Master);
        assert!(
            resolved
                .iter()
                .all(|track| track.source == RoleMetadataSource::NativeInstruments)
        );
    }

    fn inputs(names: &[&str]) -> Vec<Mp4RoleInput> {
        names
            .iter()
            .enumerate()
            .map(|(index, name)| Mp4RoleInput {
                track_id: u32::try_from(index).unwrap(),
                native_role: None,
                name: Some((*name).to_owned()),
            })
            .collect()
    }
}
