use super::*;
use std::sync::mpsc;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(10);

#[test]
fn blocked_disk_writes_do_not_block_updates_and_pending_state_is_coalesced() {
    let (started, start_received) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let saved = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&saved);
    let writer = SettingsWriter::spawn(
        LocalSettings::default(),
        move |settings| {
            let first = captured.lock().unwrap().is_empty();
            if first {
                started.send(()).unwrap();
                released.recv_timeout(WAIT).unwrap();
            }
            captured.lock().unwrap().push(settings.clone());
            Ok(())
        },
        || panic!("Synthetic writes must succeed"),
    )
    .unwrap();
    let store = writer.store();
    store
        .update(|current| LocalSettings {
            theme: 1,
            ..current.clone()
        })
        .unwrap();
    start_received.recv_timeout(WAIT).unwrap();
    for index in 0..1_000 {
        store
            .update(|current| current.with_playlist(vec![format!("track-{index}.mp3")], None))
            .unwrap();
    }
    store
        .close(|current| LocalSettings {
            diagnostics_visible: true,
            window_width: Some(780),
            window_height: Some(350),
            ..current.clone()
        })
        .unwrap();
    assert!(store.update(|_| LocalSettings::default()).is_none());
    release.send(()).unwrap();
    writer.finish(Clone::clone).unwrap();

    let saved = saved.lock().unwrap();
    assert_eq!(saved.len(), 2);
    let final_settings = saved.last().unwrap();
    assert_eq!(final_settings.playlist_sources, ["track-999.mp3"]);
    assert_eq!(final_settings.theme, 1);
    assert!(final_settings.diagnostics_visible);
    assert_eq!(final_settings.window_height, Some(350));
}

#[test]
fn invalid_updates_preserve_bounded_state_and_report_failure() {
    let (failed, failure_received) = mpsc::sync_channel(1);
    let writer = SettingsWriter::spawn(
        LocalSettings::default(),
        |_| Ok(()),
        move || failed.send(()).unwrap(),
    )
    .unwrap();
    let store = writer.store();
    assert!(
        store
            .update(|current| { current.with_playlist(vec!["x".repeat(4_000); 32], None) })
            .is_none()
    );
    assert_eq!(store.snapshot(), LocalSettings::default());
    failure_received.recv_timeout(WAIT).unwrap();
    writer.finish(Clone::clone).unwrap();
}

#[test]
fn failed_writes_are_reported_and_final_flush_retries_the_latest_state() {
    let (failed, failure_received) = mpsc::sync_channel(1);
    let saved = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&saved);
    let mut first = true;
    let writer = SettingsWriter::spawn(
        LocalSettings::default(),
        move |settings| {
            if std::mem::take(&mut first) {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            captured.lock().unwrap().push(settings.clone());
            Ok(())
        },
        move || failed.send(()).unwrap(),
    )
    .unwrap();
    writer
        .store()
        .update(|current| LocalSettings {
            theme: 2,
            ..current.clone()
        })
        .unwrap();
    failure_received.recv_timeout(WAIT).unwrap();
    writer
        .finish(|current| current.with_recent_source("track.mp3"))
        .unwrap();
    let saved = saved.lock().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].theme, 2);
    assert_eq!(saved[0].recent_sources, ["track.mp3"]);
}

#[test]
fn final_write_failure_is_returned_to_the_caller() {
    let writer = SettingsWriter::spawn(
        LocalSettings::default(),
        |_| Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        || {},
    )
    .unwrap();
    assert_eq!(
        writer.finish(Clone::clone).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}
