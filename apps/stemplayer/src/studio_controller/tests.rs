use super::*;

#[test]
fn shutdown_bypasses_a_full_command_queue() {
    let controller = StudioController::new();
    for _ in 0..COMMAND_CAPACITY {
        let _ = controller.submit(PlayerCommand::Cache(stemplayer_domain::CacheCommand::Clear));
    }
    assert!(controller.submit(PlayerCommand::Shutdown).is_ok());
    controller.shutdown_and_join();
    assert_eq!(
        controller.latest_snapshot().transport_state(),
        TransportState::Closed
    );
}

#[test]
fn controller_loads_seeks_and_stops_without_opening_audio() {
    let fixture = Fixture::new();
    let controller = StudioController::new();
    let path = stemplayer_domain::BoundedText::new(fixture.path.to_string_lossy()).unwrap();
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(path)))
        .unwrap();
    wait_for(&controller, TransportState::Ready);
    assert_eq!(
        controller
            .latest_snapshot()
            .source()
            .unwrap()
            .end_frame_exclusive,
        SourceFrame::new(8)
    );

    controller
        .submit(PlayerCommand::SeekFrame(SourceFrame::new(4)))
        .unwrap();
    wait_for(&controller, TransportState::Paused);
    assert_eq!(
        controller
            .latest_snapshot()
            .position()
            .unwrap()
            .absolute_frame(),
        SourceFrame::new(4)
    );

    controller.submit(PlayerCommand::Stop).unwrap();
    wait_for(&controller, TransportState::Ready);
    assert_eq!(
        controller
            .latest_snapshot()
            .position()
            .unwrap()
            .absolute_frame(),
        SourceFrame::new(0)
    );
    controller.shutdown_and_join();
}

#[test]
fn controller_applies_and_resets_mixer_commands_without_opening_audio() {
    let fixture = Fixture::new();
    let controller = StudioController::new();
    let path = stemplayer_domain::BoundedText::new(fixture.path.to_string_lossy()).unwrap();
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(path)))
        .unwrap();
    wait_for(&controller, TransportState::Ready);
    controller
        .submit(PlayerCommand::SetGain {
            role: stemplayer_domain::StemRole::Vocals,
            gain_db: stemplayer_domain::GainDb::new(-3.0).unwrap(),
        })
        .unwrap();
    controller
        .submit(PlayerCommand::SetMute {
            role: stemplayer_domain::StemRole::Bass,
            muted: true,
        })
        .unwrap();
    controller
        .submit(PlayerCommand::SetSolo {
            role: stemplayer_domain::StemRole::Drums,
            solo: true,
        })
        .unwrap();
    controller
        .submit(PlayerCommand::SetOutputPreference(
            stemplayer_domain::OutputPreference::StemMix,
        ))
        .unwrap();
    wait_until(&controller, |snapshot| {
        snapshot.details().mixer.automation_count == 4
    });
    let snapshot = controller.latest_snapshot();
    assert_eq!(snapshot.details().mixer.stems[3].gain_db.get(), -3.0);
    assert!(snapshot.details().mixer.stems[1].muted);
    assert!(snapshot.details().mixer.stems[0].solo);
    assert_eq!(
        snapshot.details().mixer.output_preference,
        stemplayer_domain::OutputPreference::StemMix
    );

    controller.submit(PlayerCommand::ResetMix).unwrap();
    wait_until(&controller, |snapshot| {
        snapshot.details().mixer.automation_count == 5
    });
    let mixer = controller.latest_snapshot().details().mixer;
    assert_eq!(
        mixer.output_preference,
        stemplayer_domain::OutputPreference::Automatic
    );
    assert!(mixer.stems.iter().all(|stem| {
        stem.gain_db == stemplayer_domain::GainDb::UNITY && !stem.muted && !stem.solo
    }));
    controller.shutdown_and_join();
}

#[test]
#[ignore = "requires two native output devices with different default rates"]
fn active_device_rate_change_recreates_and_reprimes_output() {
    let alternate = stemplayer_output::enumerate_devices()
        .unwrap()
        .into_iter()
        .find(|device| {
            !device.is_default
                && device
                    .default_configuration
                    .as_ref()
                    .is_some_and(|config| config.minimum_sample_rate != 44_100)
        })
        .expect("a non-default device with a different rate is required");
    let expected_rate = alternate.default_configuration.unwrap().minimum_sample_rate;
    let fixture = Fixture::with_frames(88_200);
    let controller = StudioController::new();
    let path = stemplayer_domain::BoundedText::new(fixture.path.to_string_lossy()).unwrap();
    controller
        .submit(PlayerCommand::Open(SourceLocator::LocalPath(path)))
        .unwrap();
    wait_for(&controller, TransportState::Ready);
    controller.submit(PlayerCommand::Play).unwrap();
    controller
        .submit(PlayerCommand::SelectDevice(Some(
            stemplayer_domain::BoundedText::new(alternate.id).unwrap(),
        )))
        .unwrap();
    wait_until(&controller, |snapshot| {
        snapshot
            .details()
            .resampler
            .is_some_and(|resampler| resampler.discontinuity_count == 1)
    });
    let snapshot = controller.latest_snapshot();
    let resampler = snapshot.details().resampler.unwrap();
    assert_eq!(resampler.output_rate, expected_rate);
    assert_eq!(resampler.reset_count, 1);
    assert_eq!(snapshot.details().underrun_count, 0);
    assert_eq!(snapshot.details().overflow_count, 0);
    assert_eq!(snapshot.transport_state(), TransportState::Playing);
    controller.shutdown_and_join();
}

struct Fixture {
    path: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::with_frames(8)
    }

    fn with_frames(frames: usize) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stemplayer-controller-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        let roles = ["drums", "bass", "other", "vocals"];
        for (index, role) in roles.iter().enumerate() {
            std::fs::write(
                path.join(format!("{role}.wav")),
                wave(&vec![1000_i16 + index as i16; frames]),
            )
            .unwrap();
        }
        let outputs = roles
            .iter()
            .map(|role| {
                format!(
                    r#"{{"logical_type":"{role}","filename":"{role}.wav","sample_rate":44100,"channels":2,"frames":{frames}}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(
            path.join("manifest.json"),
            format!(r#"{{"schema_version":1,"outputs":[{outputs}]}}"#),
        )
        .unwrap();
        Self { path }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn wait_for(controller: &StudioController, state: TransportState) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while controller.latest_snapshot().transport_state() != state {
        assert!(
            Instant::now() < deadline,
            "controller did not reach {state:?}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn wait_until(controller: &StudioController, predicate: impl Fn(&PlayerSnapshot) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !predicate(&controller.latest_snapshot()) {
        assert!(Instant::now() < deadline, "controller condition timed out");
        thread::sleep(Duration::from_millis(5));
    }
}

fn wave(samples: &[i16]) -> Vec<u8> {
    let data_size = (samples.len() * 4) as u32;
    let mut bytes = Vec::with_capacity(44 + data_size as usize);
    bytes.extend(b"RIFF");
    bytes.extend((36 + data_size).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(44_100_u32.to_le_bytes());
    bytes.extend((44_100_u32 * 4).to_le_bytes());
    bytes.extend(4_u16.to_le_bytes());
    bytes.extend(16_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data_size.to_le_bytes());
    for sample in samples {
        bytes.extend(sample.to_le_bytes());
        bytes.extend(sample.to_le_bytes());
    }
    bytes
}
