use super::*;

pub(super) fn open_local_session(
    path: &Path,
    generation: GenerationId,
    cancelled: &AtomicBool,
) -> Result<PlaybackSession, PlayerError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(PlayerError::new(
            ErrorCode::BundleAborted,
            "Source operation was cancelled",
        ));
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "mp4" => PlaybackSession::open_compressed(path, generation, cancelled),
        "m4a" => match PlaybackSession::open_compressed(path, generation, cancelled) {
            Err(error) if error.code != ErrorCode::BundleAborted => {
                PlaybackSession::open_audio_cancelled(path, generation, cancelled)
            }
            result => result,
        },
        "mp3" | "flac" | "ogg" | "oga" => {
            PlaybackSession::open_audio_cancelled(path, generation, cancelled)
        }
        "wav" if !role_suffixed_wav(path) => {
            PlaybackSession::open_audio_cancelled(path, generation, cancelled)
        }
        "json" | "wav" if path.is_file() => {
            let group = stemplayer_platform::select_local_source(path)?;
            let bundle = StudioBundle::inspect(group, false, cancelled)?;
            PlaybackSession::new(bundle, generation)
        }
        _ if path.is_dir() => {
            let group = stemplayer_platform::select_local_source(path)?;
            let bundle = StudioBundle::inspect(group, false, cancelled)?;
            PlaybackSession::new(bundle, generation)
        }
        _ => Err(PlayerError::new(
            ErrorCode::CodecUnsupported,
            "Selected file is not a supported audio or stem source",
        )),
    }
}

pub(super) fn waveform_fingerprint(
    session: &PlaybackSession,
) -> Result<SourceFingerprint, PlayerError> {
    let evidence = session
        .fingerprint_inputs()
        .into_iter()
        .map(|(descriptor, path)| SourceEvidence::from_path(&path, descriptor))
        .collect::<Result<Vec<_>, _>>()?;
    SourceFingerprint::from_evidence(&evidence)
}

fn role_suffixed_wav(path: &std::path::Path) -> bool {
    path.file_stem()
        .and_then(|value| value.to_str())
        .and_then(|value| value.rsplit('.').next())
        .is_some_and(|role| {
            ["drums", "bass", "other", "vocals", "master"]
                .iter()
                .any(|candidate| role.eq_ignore_ascii_case(candidate))
        })
}
