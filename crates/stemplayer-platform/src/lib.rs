#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use stemplayer_domain::{DeviceId, ErrorCode, PlayerError};
use stemplayer_format::{SelectedFile, SourceGroup};

pub const SETTINGS_SCHEMA_VERSION: u32 = 4;
pub const MAX_RECENT_SOURCES: usize = 8;
pub const MAX_PLAYLIST_SOURCES: usize = 512;
const MAX_SETTINGS_BYTES: u64 = 64 * 1024;
const MAX_SETTING_TEXT_CHARS: usize = 4_096;

pub trait PlatformPort: Send + Sync {
    fn default_device(&self) -> Result<Option<DeviceId>, PlayerError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths {
    pub data: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
}

impl AppPaths {
    pub fn discover(app_name: &str) -> Result<Self, String> {
        if app_name.is_empty()
            || !app_name
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || value == b'-')
        {
            return Err(
                "application name must contain only ASCII letters, digits, or hyphens".into(),
            );
        }
        discover_paths(app_name)
    }
}

#[cfg(target_os = "windows")]
fn discover_paths(app_name: &str) -> Result<AppPaths, String> {
    let roaming = env::var_os("APPDATA").ok_or("APPDATA is unavailable")?;
    let local = env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is unavailable")?;
    Ok(AppPaths {
        data: PathBuf::from(&local).join(app_name).join("data"),
        cache: PathBuf::from(local).join(app_name).join("cache"),
        config: PathBuf::from(roaming).join(app_name),
    })
}

#[cfg(target_os = "macos")]
fn discover_paths(app_name: &str) -> Result<AppPaths, String> {
    let home = env::var_os("HOME").ok_or("HOME is unavailable")?;
    let library = PathBuf::from(home).join("Library");
    Ok(AppPaths {
        data: library.join("Application Support").join(app_name),
        cache: library.join("Caches").join(app_name),
        config: library.join("Preferences").join(app_name),
    })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn discover_paths(app_name: &str) -> Result<AppPaths, String> {
    let home = env::var_os("HOME").ok_or("HOME is unavailable")?;
    let home = PathBuf::from(home);
    Ok(AppPaths {
        data: env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join(app_name),
        cache: env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"))
            .join(app_name),
        config: env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join(app_name),
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalSettings {
    pub schema_version: u32,
    pub diagnostics_visible: bool,
    pub selected_device: Option<String>,
    pub recent_sources_enabled: bool,
    pub recent_sources: Vec<String>,
    pub playlist_sources: Vec<String>,
    pub playlist_selected_source: Option<String>,
    pub theme: u8,
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
    pub window_x: Option<i32>,
    pub window_y: Option<i32>,
    pub window_maximized: bool,
    pub waveform_cache_enabled: bool,
    pub waveform_cache_limit_mb: u32,
}

impl Default for LocalSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            diagnostics_visible: false,
            selected_device: None,
            recent_sources_enabled: true,
            recent_sources: Vec::new(),
            playlist_sources: Vec::new(),
            playlist_selected_source: None,
            theme: 0,
            window_width: None,
            window_height: None,
            window_x: None,
            window_y: None,
            window_maximized: false,
            waveform_cache_enabled: false,
            waveform_cache_limit_mb: 256,
        }
    }
}

impl LocalSettings {
    pub fn with_recent_source(&self, source: &str) -> Self {
        if !self.recent_sources_enabled || !valid_setting_text(source) {
            return self.clone();
        }
        let recent_sources = std::iter::once(source.to_owned())
            .chain(
                self.recent_sources
                    .iter()
                    .filter(|value| value.as_str() != source)
                    .cloned(),
            )
            .take(MAX_RECENT_SOURCES)
            .collect();
        Self {
            recent_sources,
            ..self.clone()
        }
    }

    pub fn with_playlist(&self, sources: Vec<String>, selected_source: Option<String>) -> Self {
        Self {
            playlist_sources: sources,
            playlist_selected_source: selected_source,
            ..self.clone()
        }
    }

    pub fn with_recent_sources_enabled(&self, enabled: bool) -> Self {
        Self {
            recent_sources_enabled: enabled,
            recent_sources: if enabled {
                self.recent_sources.clone()
            } else {
                Vec::new()
            },
            ..self.clone()
        }
    }

    pub fn without_recent_sources(&self) -> Self {
        Self {
            recent_sources: Vec::new(),
            ..self.clone()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsLoad {
    Loaded(LocalSettings),
    Migrated(LocalSettings),
    Missing(LocalSettings),
    Recovered(LocalSettings),
}

impl SettingsLoad {
    pub fn settings(&self) -> &LocalSettings {
        match self {
            Self::Loaded(value)
            | Self::Migrated(value)
            | Self::Missing(value)
            | Self::Recovered(value) => value,
        }
    }
}

pub fn load_settings(path: &Path) -> SettingsLoad {
    if fs::metadata(path).is_ok_and(|metadata| metadata.len() > MAX_SETTINGS_BYTES) {
        return SettingsLoad::Recovered(LocalSettings::default());
    }
    let Ok(bytes) = fs::read(path) else {
        return SettingsLoad::Missing(LocalSettings::default());
    };
    match serde_json::from_slice::<LocalSettings>(&bytes) {
        Ok(settings)
            if settings.schema_version == SETTINGS_SCHEMA_VERSION && valid_settings(&settings) =>
        {
            SettingsLoad::Loaded(settings)
        }
        Ok(settings) if matches!(settings.schema_version, 1..=3) && valid_settings(&settings) => {
            SettingsLoad::Migrated(LocalSettings {
                schema_version: SETTINGS_SCHEMA_VERSION,
                ..settings
            })
        }
        Ok(_) | Err(_) => SettingsLoad::Recovered(LocalSettings::default()),
    }
}

pub fn save_settings(path: &Path, settings: &LocalSettings) -> std::io::Result<()> {
    if settings.schema_version != SETTINGS_SCHEMA_VERSION || !valid_settings(settings) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "application settings are invalid",
        ));
    }
    if let Some(parent) = path.parent().filter(|value| !value.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(settings).map_err(std::io::Error::other)?;
    if bytes.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "serialized application settings exceed the bounded size",
        ));
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("settings");
    let suffix = std::process::id();
    let temporary = path.with_file_name(format!(".{file_name}.{suffix}.tmp"));
    let backup = path.with_file_name(format!(".{file_name}.{suffix}.bak"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    if path.exists() {
        if let Err(error) = fs::rename(path, &backup) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temporary, path) {
            let _ = fs::rename(&backup, path);
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        let _ = fs::remove_file(backup);
        Ok(())
    } else {
        fs::rename(temporary, path)
    }
}

fn valid_settings(settings: &LocalSettings) -> bool {
    settings
        .selected_device
        .as_deref()
        .is_none_or(valid_setting_text)
        && settings.recent_sources.len() <= MAX_RECENT_SOURCES
        && settings
            .recent_sources
            .iter()
            .all(|value| valid_setting_text(value))
        && settings.playlist_sources.len() <= MAX_PLAYLIST_SOURCES
        && settings
            .playlist_sources
            .iter()
            .all(|value| valid_setting_text(value))
        && settings
            .playlist_selected_source
            .as_deref()
            .is_none_or(valid_setting_text)
        && settings.theme <= 2
        && valid_window_size(settings.window_width, settings.window_height)
        && valid_window_position(settings.window_x, settings.window_y)
        && (16..=4_096).contains(&settings.waveform_cache_limit_mb)
}

fn valid_window_size(width: Option<u32>, height: Option<u32>) -> bool {
    match (width, height) {
        (None, None) => true,
        (Some(width), Some(height)) => {
            (560..=3_840).contains(&width) && (400..=2_160).contains(&height)
        }
        _ => false,
    }
}

fn valid_window_position(x: Option<i32>, y: Option<i32>) -> bool {
    match (x, y) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            (-1_000_000..=1_000_000).contains(&x) && (-1_000_000..=1_000_000).contains(&y)
        }
        _ => false,
    }
}

fn valid_setting_text(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= MAX_SETTING_TEXT_CHARS
        && !value.chars().any(char::is_control)
}

pub fn select_local_source(selection: &Path) -> Result<SourceGroup, PlayerError> {
    if selection.is_dir() {
        let files = selected_files(selection, |_| true)?;
        return if files
            .iter()
            .any(|file| file.basename().eq_ignore_ascii_case("manifest.json"))
        {
            SourceGroup::new(source_display_name(selection), files)
        } else {
            SourceGroup::inferred(source_display_name(selection), files)
        };
    }
    let metadata = fs::symlink_metadata(selection)
        .map_err(|error| selection_io_error("Selected source is unavailable", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(selection_error(
            ErrorCode::ManifestPathUnsafe,
            "Selection must be a regular file or folder",
        ));
    }
    let root = selection.parent().unwrap_or_else(|| Path::new("."));
    let name = selection
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            selection_error(
                ErrorCode::ManifestPathUnsafe,
                "Selected filename is not valid Unicode",
            )
        })?;
    if selection
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        return SourceGroup::with_manifest(
            source_display_name(root),
            selected_files(root, |_| true)?,
            name,
        );
    }
    if let Some(prefix) = wav_bundle_prefix(name) {
        let files = selected_files(root, |candidate| {
            wav_bundle_prefix(candidate).is_some_and(|value| value.eq_ignore_ascii_case(&prefix))
        })?;
        return SourceGroup::inferred(
            if prefix.is_empty() {
                source_display_name(root)
            } else {
                prefix
            },
            files,
        );
    }
    Err(selection_error(
        ErrorCode::ManifestPathUnsafe,
        "Selection must be JSON or a role-suffixed WAV file",
    ))
}

fn selected_files(
    root: &Path,
    include: impl Fn(&str) -> bool,
) -> Result<Vec<SelectedFile>, PlayerError> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root)
        .map_err(|error| selection_io_error("Selected folder is unavailable", error))?
    {
        let entry =
            entry.map_err(|error| selection_io_error("Selected folder cannot be read", error))?;
        let file_type = entry
            .file_type()
            .map_err(|error| selection_io_error("Selected entry cannot be inspected", error))?;
        if file_type.is_symlink() || !file_type.is_file() {
            continue;
        }
        let Ok(basename) = entry.file_name().into_string() else {
            continue;
        };
        if include(&basename) {
            files.push(SelectedFile::new(basename, entry.path()));
        }
    }
    Ok(files)
}

fn wav_bundle_prefix(name: &str) -> Option<String> {
    let path = Path::new(name);
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wav"))
    {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let (prefix, role) = stem.rsplit_once('.').unwrap_or(("", stem));
    matches!(
        role.to_ascii_lowercase().as_str(),
        "drums" | "bass" | "other" | "vocals" | "master"
    )
    .then(|| prefix.to_owned())
}

pub fn select_local_paths(paths: &[PathBuf]) -> Result<SourceGroup, PlayerError> {
    if paths.len() == 1 {
        return select_local_source(&paths[0]);
    }
    let root = paths
        .first()
        .and_then(|path| path.parent())
        .ok_or_else(|| selection_error(ErrorCode::ManifestFileMissing, "Dropped group is empty"))?;
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        if path.parent() != Some(root) {
            return Err(selection_error(
                ErrorCode::ManifestPathUnsafe,
                "Dropped files must share one folder",
            ));
        }
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| selection_io_error("Dropped file is unavailable", error))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(selection_error(
                ErrorCode::ManifestPathUnsafe,
                "Dropped entry is not a regular file",
            ));
        }
        let basename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                selection_error(
                    ErrorCode::ManifestPathUnsafe,
                    "Dropped filename is not valid Unicode",
                )
            })?
            .to_owned();
        files.push(SelectedFile::new(basename, path.clone()));
    }
    let manifest = paths.iter().find_map(|path| {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
            .then(|| path.file_name()?.to_str())
            .flatten()
    });
    match manifest {
        Some(manifest) => SourceGroup::with_manifest(source_display_name(root), files, manifest),
        None => SourceGroup::inferred(source_display_name(root), files),
    }
}

fn source_display_name(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Studio WAV bundle")
        .to_owned()
}

fn selection_error(code: ErrorCode, summary: &str) -> PlayerError {
    PlayerError::new(code, summary).with_user_action("Choose a valid Studio WAV bundle")
}

fn selection_io_error(summary: &str, error: std::io::Error) -> PlayerError {
    selection_error(ErrorCode::BundleInconsistent, summary).with_source(error)
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenSelection {
    StemContainer(PathBuf),
    StudioManifest(PathBuf),
    StudioFolder(PathBuf),
}

pub trait NativeDialogs: Send + Sync {
    fn open_source(&self) -> Result<Option<OpenSelection>, PlayerError>;
    fn show_in_file_manager(&self, path: &Path) -> Result<(), PlayerError>;
    fn confirm_cache_clear(&self) -> Result<bool, PlayerError>;
}

pub trait DropReceiver: Send + Sync {
    fn receive_drop(&self, paths: &[PathBuf]) -> Result<Option<OpenSelection>, PlayerError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_path(name: &str) -> PathBuf {
        env::temp_dir().join(format!(
            "stemplayer-platform-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn settings_load_or_recover_without_panicking() {
        let missing = test_path("missing");
        assert!(matches!(load_settings(&missing), SettingsLoad::Missing(_)));

        let corrupt = test_path("corrupt");
        fs::write(&corrupt, b"not-json").unwrap();
        assert!(matches!(
            load_settings(&corrupt),
            SettingsLoad::Recovered(_)
        ));
        fs::remove_file(corrupt).unwrap();

        let valid = test_path("valid");
        fs::write(
            &valid,
            serde_json::to_vec(&LocalSettings::default()).unwrap(),
        )
        .unwrap();
        assert!(matches!(load_settings(&valid), SettingsLoad::Loaded(_)));
        fs::remove_file(valid).unwrap();
    }

    #[test]
    fn settings_save_creates_parent_and_round_trips() {
        let root = test_path("save");
        let path = root.join("nested/settings.json");
        let settings = LocalSettings {
            diagnostics_visible: true,
            selected_device: Some("device-1".to_owned()),
            window_maximized: true,
            ..LocalSettings::default()
        };
        save_settings(&path, &settings).unwrap();
        assert_eq!(load_settings(&path).settings(), &settings);

        let unsupported = LocalSettings {
            schema_version: SETTINGS_SCHEMA_VERSION + 1,
            ..LocalSettings::default()
        };
        assert!(save_settings(&path, &unsupported).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn settings_migrate_older_schemas_and_bound_recent_sources() {
        let path = test_path("migrate");
        fs::write(
            &path,
            br#"{"schema_version":1,"diagnostics_visible":true,"selected_device":null}"#,
        )
        .unwrap();
        let loaded = load_settings(&path);
        assert!(matches!(loaded, SettingsLoad::Migrated(_)));
        assert_eq!(loaded.settings().schema_version, SETTINGS_SCHEMA_VERSION);
        assert!(loaded.settings().recent_sources_enabled);

        let settings = (0..MAX_RECENT_SOURCES + 2)
            .fold(LocalSettings::default(), |value, index| {
                value.with_recent_source(&format!("C:\\music\\{index}.stem.m4a"))
            });
        assert_eq!(settings.recent_sources.len(), MAX_RECENT_SOURCES);
        assert_eq!(settings.recent_sources[0], "C:\\music\\9.stem.m4a");
        assert!(
            settings
                .with_recent_sources_enabled(false)
                .recent_sources
                .is_empty()
        );
        assert_eq!(
            settings.without_recent_sources().recent_sources,
            Vec::<String>::new()
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn settings_reject_unsafe_window_and_cache_values() {
        for settings in [
            LocalSettings {
                window_width: Some(100),
                window_height: Some(100),
                ..LocalSettings::default()
            },
            LocalSettings {
                waveform_cache_limit_mb: 0,
                ..LocalSettings::default()
            },
            LocalSettings {
                theme: 3,
                ..LocalSettings::default()
            },
            LocalSettings {
                window_x: Some(i32::MAX),
                window_y: Some(0),
                ..LocalSettings::default()
            },
        ] {
            assert!(!valid_settings(&settings));
        }
    }

    #[test]
    fn dropped_group_is_explicit_and_single_folder() {
        let root = test_path("drop");
        fs::create_dir(&root).unwrap();
        let manifest = root.join("manifest.json");
        let drums = root.join("drums.wav");
        fs::write(&manifest, b"{}").unwrap();
        fs::write(&drums, b"RIFF").unwrap();
        assert!(select_local_paths(&[manifest.clone(), drums]).is_ok());

        let other_root = test_path("other-drop");
        fs::create_dir(&other_root).unwrap();
        let other = other_root.join("bass.wav");
        fs::write(&other, b"RIFF").unwrap();
        assert!(select_local_paths(&[manifest, other]).is_err());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(other_root).unwrap();
    }

    #[test]
    fn arbitrary_json_and_role_suffixed_wav_are_selectable() {
        let root = test_path("local-pattern");
        fs::create_dir(&root).unwrap();
        let manifest = root.join("song.json");
        fs::write(&manifest, b"{}").unwrap();
        assert!(
            select_local_source(&manifest)
                .unwrap()
                .manifest_reader()
                .unwrap()
                .is_some()
        );

        fs::remove_file(&manifest).unwrap();
        let bass = root.join("song.bass.wav");
        fs::write(&bass, b"RIFF").unwrap();
        let group = select_local_source(&bass).unwrap();
        assert_eq!(group.display_name(), "song");
        assert!(group.manifest_reader().unwrap().is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn application_name_is_validated() {
        assert!(AppPaths::discover("").is_err());
        assert!(AppPaths::discover("../private").is_err());
        assert!(AppPaths::discover("stemplayer-native").is_ok());
    }
    #[test]
    fn playlist_settings_migrate_round_trip_and_preserve_previous_on_oversize() {
        let migration = test_path("playlist-migrate");
        fs::write(&migration, br#"{"schema_version":3}"#).unwrap();
        let migrated = load_settings(&migration);
        assert!(matches!(migrated, SettingsLoad::Migrated(_)));
        assert!(migrated.settings().playlist_sources.is_empty());
        fs::remove_file(migration).unwrap();

        let path = test_path("playlist-save");
        let original = LocalSettings::default().with_playlist(
            vec![
                "C:\\music\\first.mp3".to_owned(),
                "C:\\music\\second.flac".to_owned(),
            ],
            Some("C:\\music\\second.flac".to_owned()),
        );
        save_settings(&path, &original).unwrap();
        assert_eq!(load_settings(&path).settings(), &original);

        let oversized = original.with_playlist(
            (0..32)
                .map(|index| format!("C:\\music\\{index}-{}.mp3", "x".repeat(4_000)))
                .collect(),
            None,
        );
        assert!(save_settings(&path, &oversized).is_err());
        assert_eq!(load_settings(&path).settings(), &original);
        fs::remove_file(path).unwrap();
    }
}
