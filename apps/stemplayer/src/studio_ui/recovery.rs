use crate::playlist::Playlist;
use stemplayer_domain::{
    BoundedText, ErrorCode, GenerationId, PlayerError, PlayerSnapshot, SourceLocator,
    TransportState,
};

pub(super) fn is_new_end(
    previous: Option<(GenerationId, TransportState)>,
    snapshot: &PlayerSnapshot,
) -> bool {
    snapshot.transport_state() == TransportState::Ended
        && previous != Some((snapshot.generation(), TransportState::Ended))
}

pub(super) fn admit_playlist_recovery(
    playlist: &mut Playlist,
    playlist_source: bool,
    source_failed: bool,
    play: bool,
    admit: impl FnOnce(Option<SourceLocator>, bool) -> Result<bool, PlayerError>,
) -> Result<bool, PlayerError> {
    let mut candidate = playlist.clone();
    let next = if playlist_source {
        if source_failed {
            candidate.next_after_error()
        } else {
            candidate.next()
        }
    } else {
        None
    };
    if next.is_none() && !source_failed {
        return Ok(false);
    }
    let source = next
        .as_ref()
        .map(|(_, path)| {
            let path = path.to_str().ok_or_else(|| {
                PlayerError::new(
                    ErrorCode::ManifestPathUnsafe,
                    "Source path is not valid Unicode",
                )
            })?;
            BoundedText::new(path).map(SourceLocator::LocalPath)
        })
        .transpose()?;
    let play = play && source.is_some();
    if !admit(source, play)? {
        return Ok(false);
    }
    *playlist = candidate;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playlist::PlaylistItem;

    fn playlist() -> Playlist {
        let mut playlist = Playlist::default();
        playlist.append(
            (0..3)
                .map(|index| PlaylistItem {
                    label: format!("Track {index}"),
                    source: format!("track-{index}.mp3").into(),
                })
                .collect(),
        );
        playlist.select(2, true, false);
        playlist
    }

    #[test]
    fn coalesced_ends_from_distinct_generations_each_advance_once() {
        let first = GenerationId::INITIAL;
        let second = first.next().unwrap();
        let ended = |generation| {
            PlayerSnapshot::new(TransportState::Ended, generation, None, None, vec![]).unwrap()
        };
        assert!(is_new_end(None, &ended(first)));
        assert!(is_new_end(
            Some((first, TransportState::Playing)),
            &ended(first)
        ));
        assert!(!is_new_end(
            Some((first, TransportState::Ended)),
            &ended(first)
        ));
        assert!(is_new_end(
            Some((first, TransportState::Ended)),
            &ended(second)
        ));
        assert!(!is_new_end(
            Some((second, TransportState::Ended)),
            &ended(second)
        ));
    }

    #[test]
    fn stale_full_closed_and_exhausted_admission_leave_the_playlist_unchanged() {
        for source_failed in [false, true] {
            for rejection in [
                None,
                Some(ErrorCode::CommandQueueFull),
                Some(ErrorCode::PlaybackSessionClosed),
                Some(ErrorCode::ValueOutOfRange),
            ] {
                let mut playlist = playlist();
                let rows = playlist.rows();
                let sources = playlist.sources();
                let result = admit_playlist_recovery(
                    &mut playlist,
                    true,
                    source_failed,
                    true,
                    |source, play| {
                        assert_eq!(
                            source,
                            Some(SourceLocator::LocalPath(
                                BoundedText::new("track-1.mp3").unwrap()
                            ))
                        );
                        assert!(play);
                        rejection.map_or(Ok(false), |code| {
                            Err(PlayerError::new(code, "Synthetic rejection"))
                        })
                    },
                );
                assert_eq!(result.is_err(), rejection.is_some());
                if let Ok(admitted) = result {
                    assert!(!admitted);
                }
                assert_eq!(playlist.rows(), rows);
                assert_eq!(playlist.sources(), sources);
                assert_eq!(playlist.selected(), Some(2));
                assert_eq!(playlist.active_source(), Some("track-0.mp3".into()));
            }
        }
    }

    #[test]
    fn successful_recovery_commits_selection_and_associated_play_together() {
        for source_failed in [false, true] {
            for play in [false, true] {
                let mut playlist = playlist();
                assert!(
                    admit_playlist_recovery(
                        &mut playlist,
                        true,
                        source_failed,
                        play,
                        |source, admitted_play| {
                            assert_eq!(
                                source,
                                Some(SourceLocator::LocalPath(
                                    BoundedText::new("track-1.mp3").unwrap()
                                ))
                            );
                            assert_eq!(admitted_play, play);
                            Ok(true)
                        },
                    )
                    .unwrap()
                );
                assert_eq!(playlist.selected(), Some(1));
                assert_eq!(playlist.active_source(), Some("track-1.mp3".into()));
                assert_eq!(
                    playlist
                        .rows()
                        .iter()
                        .map(|(_, selected)| *selected)
                        .collect::<Vec<_>>(),
                    [false, true, false]
                );
            }
        }
    }

    #[test]
    fn exhausted_end_retains_the_source_and_unrelated_errors_only_unload() {
        let mut playlist = playlist();
        playlist.activate(2);
        assert!(
            !admit_playlist_recovery(&mut playlist, true, false, true, |_, _| {
                panic!("An exhausted normal end must not unload")
            })
            .unwrap()
        );
        let rows = playlist.rows();
        assert!(
            admit_playlist_recovery(&mut playlist, false, true, true, |source, play| {
                assert_eq!(source, None);
                assert!(!play);
                Ok(true)
            })
            .unwrap()
        );
        assert_eq!(playlist.rows(), rows);
        assert_eq!(playlist.active_source(), Some("track-2.mp3".into()));
    }
}
