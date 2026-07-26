#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use stemplayer_domain::{DeviceId, ErrorCode, PlayerError};
use stemplayer_format::{SelectedFile, SourceGroup};

pub const SETTINGS_SCHEMA_VERSION: u32 = 2;
pub const MAX_RECENT_SOURCES: usize = 8;
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
    pub theme: u8,
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,
    pub window_x: Option<i32>,
    pub window_y: Option<i32>,
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
            theme: 0,
            window_width: None,
            window_height: None,
            window_x: None,
            window_y: None,
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
        Ok(settings) if settings.schema_version == 1 && valid_settings(&settings) => {
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
    fs::write(path, bytes)
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
    let root = if selection.is_dir() {
        selection
    } else {
        let metadata = fs::symlink_metadata(selection)
            .map_err(|error| selection_io_error("Selected manifest is unavailable", error))?;
        let name = selection.file_name().and_then(|name| name.to_str());
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || !name.is_some_and(|name| name.eq_ignore_ascii_case("manifest.json"))
        {
            return Err(selection_error(
                ErrorCode::ManifestPathUnsafe,
                "Selection must be a folder or manifest.json",
            ));
        }
        selection.parent().unwrap_or_else(|| Path::new("."))
    };
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
        files.push(SelectedFile::new(basename, entry.path()));
    }
    SourceGroup::new(source_display_name(root), files)
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
    SourceGroup::new(source_display_name(root), files)
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
    fn settings_migrate_v1_and_bound_recent_sources() {
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
    fn application_name_is_validated() {
        assert!(AppPaths::discover("").is_err());
        assert!(AppPaths::discover("../private").is_err());
        assert!(AppPaths::discover("stemplayer-native").is_ok());
    }
}
