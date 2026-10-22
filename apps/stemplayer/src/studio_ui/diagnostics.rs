use super::{AppWindow, StudioController};
use slint::ComponentHandle;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use stemplayer_domain::{PlayerController, PlayerSnapshot};

pub(super) fn wire(app: &AppWindow, controller: Arc<StudioController>) {
    let weak = app.as_weak();
    app.on_export_diagnostics(move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if !begin_export(&app) {
            return;
        }
        let Some(path) = choose_destination() else {
            complete_export(&app, "Export cancelled");
            return;
        };
        let snapshot = controller.latest_snapshot();
        let completion = weak.clone();
        if std::thread::Builder::new()
            .name("diagnostics-export".into())
            .spawn(move || {
                let status = export_status(write_diagnostics(&path, &snapshot));
                let _ = completion.upgrade_in_event_loop(move |app| {
                    complete_export(&app, status);
                });
            })
            .is_err()
        {
            complete_export(&app, "Export failed");
        }
    });
}

pub(super) fn begin_export(app: &AppWindow) -> bool {
    if app.get_diagnostics_exporting() {
        return false;
    }
    app.set_diagnostics_exporting(true);
    app.set_diagnostics_export_status("Exporting...".into());
    true
}

fn complete_export(app: &AppWindow, status: &str) {
    app.set_diagnostics_export_status(status.into());
    app.set_diagnostics_exporting(false);
}

fn export_status(result: io::Result<()>) -> &'static str {
    match result {
        Ok(()) => "Exported",
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            "Export failed: choose a new file"
        }
        Err(_) => "Export failed",
    }
}

fn write_diagnostics(path: &Path, snapshot: &PlayerSnapshot) -> io::Result<()> {
    let bytes = diagnostics_json(snapshot).map_err(io::Error::other)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()
}

fn diagnostics_json(snapshot: &PlayerSnapshot) -> Result<Vec<u8>, serde_json::Error> {
    let mut value = serde_json::to_value(snapshot)?;
    if let Some(source) = value
        .get_mut("source")
        .and_then(serde_json::Value::as_object_mut)
    {
        source.remove("display_name");
    }
    if let Some(details) = value
        .get_mut("details")
        .and_then(serde_json::Value::as_object_mut)
    {
        details.remove("device_name");
    }
    serde_json::to_vec_pretty(&value)
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn choose_destination() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Export diagnostics to a new file")
        .set_file_name("stemplayer-diagnostics.json")
        .add_filter("JSON", &["json"])
        .save_file()
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn choose_destination() -> Option<PathBuf> {
    std::env::var_os("STEMPLAYER_DIAGNOSTICS_EXPORT").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stemplayer_domain::{
        ChannelCount, DisplayName, GenerationId, SampleRate, SnapshotDetails, SourceFrame,
        SourceKind, SourceMetadata, TransportState,
    };

    #[test]
    fn diagnostics_export_redacts_source_and_device_names_without_overwriting() {
        let source = SourceMetadata {
            display_name: DisplayName::truncated("synthetic-private-track.stem.m4a"),
            kind: SourceKind::LocalStemContainer,
            has_master: true,
            sample_rate: SampleRate::new(48_000).unwrap(),
            channel_count: ChannelCount::new(10).unwrap(),
            start_frame: SourceFrame::new(0),
            end_frame_exclusive: SourceFrame::new(48_000),
        };
        let snapshot = PlayerSnapshot::new(
            TransportState::Ready,
            GenerationId::INITIAL,
            None,
            Some(source),
            vec![],
        )
        .unwrap()
        .with_details(SnapshotDetails {
            device_name: Some(DisplayName::truncated("Synthetic private device")),
            ..SnapshotDetails::default()
        });
        let root = std::env::temp_dir().join(format!(
            "stemplayer-diagnostics-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("report.json");
        assert_eq!(
            export_status(write_diagnostics(&path, &snapshot)),
            "Exported"
        );
        let bytes = std::fs::read(&path).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(!text.contains("synthetic-private-track"));
        assert!(!text.contains("Synthetic private device"));
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json["source"].get("display_name").is_none());
        assert!(json["details"].get("device_name").is_none());
        assert!(json["source"].get("sample_rate").is_some());
        assert!(json["details"].get("waveform_cache").is_some());
        assert_eq!(
            export_status(write_diagnostics(&path, &snapshot)),
            "Export failed: choose a new file"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            export_status(Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "synthetic"
            ))),
            "Export failed"
        );
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
