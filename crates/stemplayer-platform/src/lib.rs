#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use stemplayer_domain::{DeviceId, PlayerError};

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

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
pub struct LocalSettings {
    pub schema_version: u32,
    pub diagnostics_visible: bool,
    pub selected_device: Option<String>,
}

impl Default for LocalSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            diagnostics_visible: false,
            selected_device: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsLoad {
    Loaded(LocalSettings),
    Missing(LocalSettings),
    Recovered(LocalSettings),
}

impl SettingsLoad {
    pub fn settings(&self) -> &LocalSettings {
        match self {
            Self::Loaded(value) | Self::Missing(value) | Self::Recovered(value) => value,
        }
    }
}

pub fn load_settings(path: &Path) -> SettingsLoad {
    let Ok(bytes) = fs::read(path) else {
        return SettingsLoad::Missing(LocalSettings::default());
    };
    match serde_json::from_slice::<LocalSettings>(&bytes) {
        Ok(settings) if settings.schema_version == SETTINGS_SCHEMA_VERSION => {
            SettingsLoad::Loaded(settings)
        }
        Ok(_) | Err(_) => SettingsLoad::Recovered(LocalSettings::default()),
    }
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
    fn application_name_is_validated() {
        assert!(AppPaths::discover("").is_err());
        assert!(AppPaths::discover("../private").is_err());
        assert!(AppPaths::discover("stemplayer-native").is_ok());
    }
}
