use super::lifecycle_tests::{engine, harness, open_command};
use super::*;

fn replacement() -> SourceLocator {
    SourceLocator::LocalPath(stemplayer_domain::BoundedText::new("synthetic-recovery.wav").unwrap())
}

fn source_evidence(
    controller: &StudioController,
    state: TransportState,
    transport_revision: u64,
) -> SourceSnapshot {
    let subscription = controller.subscribe_source_snapshots();
    {
        let _admission = controller.shared.command_admission.lock().unwrap();
        publish(
            &controller.shared,
            PlayerSnapshot::empty(state),
            transport_revision,
        );
    }
    subscription.recv_timeout(Duration::ZERO).unwrap()
}

#[test]
fn delayed_failure_cannot_replace_or_unload_a_newly_admitted_source() {
    let (controller, receiver) = harness();
    let subscription = controller.subscribe_source_snapshots();
    controller.submit(open_command("synthetic-a.wav")).unwrap();
    let a = receiver.recv().unwrap();
    let mut state = WorkerState::new(None, false);
    state.source_epoch = a.epoch;
    state.transport_revision = a.transport_revision;
    state.source_cancelled = a.cancelled;
    publish_error(
        &controller.shared,
        &mut state,
        PlayerError::new(ErrorCode::DecodeFailed, "Synthetic failure"),
    );
    controller.submit(open_command("synthetic-b.wav")).unwrap();
    let evidence = subscription.recv_timeout(Duration::ZERO).unwrap();
    assert_eq!(evidence.snapshot().transport_state(), TransportState::Error);
    assert_eq!(
        evidence.snapshot().generation(),
        controller.latest_snapshot().generation()
    );
    let b_epoch = controller.shared.epoch.load(Ordering::Acquire);
    let b_revision = controller.shared.transport_revision.load(Ordering::Acquire);
    let b_cancelled = Arc::clone(&controller.shared.cancel.lock().unwrap());
    for source in [Some(replacement()), None] {
        assert!(
            !controller
                .submit_source_recovery(&evidence, source, true)
                .unwrap()
        );
        assert_eq!(controller.shared.epoch.load(Ordering::Acquire), b_epoch);
        assert_eq!(
            controller.shared.transport_revision.load(Ordering::Acquire),
            b_revision
        );
        assert!(Arc::ptr_eq(
            &b_cancelled,
            &controller.shared.cancel.lock().unwrap()
        ));
        assert!(!b_cancelled.load(Ordering::Acquire));
        assert!(!controller.play_requested());
    }
    let queued = receiver.try_iter().collect::<Vec<_>>();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].epoch, b_epoch);
    assert!(
        matches!(&queued[0].command, WorkerCommand::Player(command) if *command == open_command("synthetic-b.wav"))
    );
}

#[test]
fn current_failed_source_recovery_admits_open_and_play_as_one_operation() {
    let (controller, receiver) = harness();
    let subscription = controller.subscribe_source_snapshots();
    controller.submit(open_command("synthetic-a.wav")).unwrap();
    let a = receiver.recv().unwrap();
    let mut state = WorkerState::new(None, false);
    state.source_epoch = a.epoch;
    state.transport_revision = a.transport_revision;
    state.source_cancelled = Arc::clone(&a.cancelled);
    publish_error(
        &controller.shared,
        &mut state,
        PlayerError::new(ErrorCode::DecodeFailed, "Synthetic failure"),
    );
    assert!(a.cancelled.load(Ordering::Acquire));
    let evidence = subscription.recv_timeout(Duration::ZERO).unwrap();
    assert!(
        controller
            .submit_source_recovery(&evidence, Some(replacement()), true)
            .unwrap()
    );
    assert!(controller.play_requested());
    let recovery = receiver.recv().unwrap();
    assert_eq!(recovery.epoch, a.epoch + 1);
    assert!(!Arc::ptr_eq(&a.cancelled, &recovery.cancelled));
    assert!(!recovery.cancelled.load(Ordering::Acquire));
    assert!(
        matches!(&recovery.command, WorkerCommand::SourceRecovery { source: Some(source), play: true } if *source == replacement())
    );
    assert!(receiver.try_recv().is_err());
    assert!(
        !controller
            .submit_source_recovery(&evidence, None, false)
            .unwrap()
    );
    assert_eq!(
        controller.shared.epoch.load(Ordering::Acquire),
        recovery.epoch
    );
    assert!(receiver.try_recv().is_err());
    controller.submit(open_command("synthetic-b.wav")).unwrap();
    assert!(!should_process_command(&recovery, &controller.shared));
    assert!(recovery.cancelled.load(Ordering::Acquire));
    let b = receiver.recv().unwrap();
    assert!(should_process_command(&b, &controller.shared));
    assert!(!b.cancelled.load(Ordering::Acquire));
    assert!(!controller.play_requested());
    assert!(receiver.try_recv().is_err());
}

#[test]
fn recovery_open_and_play_fit_atomically_in_the_last_queue_slot() {
    let (controller, receiver) = harness();
    let evidence = source_evidence(&controller, TransportState::Ended, 0);
    for _ in 0..COMMAND_CAPACITY - 1 {
        controller.submit(PlayerCommand::ResetMix).unwrap();
    }
    assert!(
        controller
            .submit_source_recovery(&evidence, Some(replacement()), true)
            .unwrap()
    );
    for _ in 0..COMMAND_CAPACITY - 1 {
        assert!(matches!(
            receiver.recv().unwrap().command,
            WorkerCommand::Player(PlayerCommand::ResetMix)
        ));
    }
    assert!(matches!(
        receiver.recv().unwrap().command,
        WorkerCommand::SourceRecovery {
            source: Some(_),
            play: true
        }
    ));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn full_recovery_admission_preserves_queue_epoch_cancellation_and_play_intent() {
    let (controller, receiver) = harness();
    controller.submit(PlayerCommand::Play).unwrap();
    let processed = receiver.recv().unwrap();
    let evidence = source_evidence(
        &controller,
        TransportState::Ended,
        processed.transport_revision,
    );
    let cancelled = Arc::clone(&controller.shared.cancel.lock().unwrap());
    let epoch = controller.shared.epoch.load(Ordering::Acquire);
    for _ in 0..COMMAND_CAPACITY {
        controller.submit(PlayerCommand::ResetMix).unwrap();
    }
    assert_eq!(
        controller.submit(PlayerCommand::Pause).unwrap_err().code,
        ErrorCode::CommandQueueFull
    );
    for source in [Some(replacement()), None] {
        let error = controller
            .submit_source_recovery(&evidence, source, false)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::CommandQueueFull);
        assert_eq!(controller.shared.epoch.load(Ordering::Acquire), epoch);
        assert_eq!(
            controller.shared.transport_revision.load(Ordering::Acquire),
            processed.transport_revision
        );
        assert!(Arc::ptr_eq(
            &cancelled,
            &controller.shared.cancel.lock().unwrap()
        ));
        assert!(!cancelled.load(Ordering::Acquire));
        assert!(controller.play_requested());
    }
    let queued = receiver.try_iter().collect::<Vec<_>>();
    assert_eq!(queued.len(), COMMAND_CAPACITY);
    for queued in &queued {
        assert_eq!(queued.epoch, epoch);
        assert_eq!(queued.transport_revision, processed.transport_revision);
        assert!(Arc::ptr_eq(&queued.cancelled, &cancelled));
        assert!(matches!(
            queued.command,
            WorkerCommand::Player(PlayerCommand::ResetMix)
        ));
    }
}

#[test]
fn closed_recovery_admission_does_not_add_commands_or_replace_the_lifetime() {
    let (controller, receiver) = harness();
    let evidence = source_evidence(&controller, TransportState::Ended, 0);
    controller.signal_shutdown();
    let epoch = controller.shared.epoch.load(Ordering::Acquire);
    let cancelled = Arc::clone(&controller.shared.cancel.lock().unwrap());
    let error = controller
        .submit_source_recovery(&evidence, Some(replacement()), true)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PlaybackSessionClosed);
    assert_eq!(controller.shared.epoch.load(Ordering::Acquire), epoch);
    assert_eq!(
        controller.shared.transport_revision.load(Ordering::Acquire),
        0
    );
    assert!(Arc::ptr_eq(
        &cancelled,
        &controller.shared.cancel.lock().unwrap()
    ));
    assert!(cancelled.load(Ordering::Acquire));
    assert!(!controller.play_requested());
    let queued = receiver.try_iter().collect::<Vec<_>>();
    assert_eq!(queued.len(), 1);
    assert!(matches!(
        queued[0].command,
        WorkerCommand::Player(PlayerCommand::Shutdown)
    ));
}

#[test]
fn disconnected_recovery_admission_preserves_epoch_and_lifetime() {
    let (controller, receiver) = harness();
    let evidence = source_evidence(&controller, TransportState::Ended, 0);
    let cancelled = Arc::clone(&controller.shared.cancel.lock().unwrap());
    let epoch = controller.shared.epoch.load(Ordering::Acquire);
    drop(receiver);
    let error = controller
        .submit_source_recovery(&evidence, None, false)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PlaybackSessionClosed);
    assert_eq!(controller.shared.epoch.load(Ordering::Acquire), epoch);
    assert_eq!(
        controller.shared.transport_revision.load(Ordering::Acquire),
        0
    );
    assert!(Arc::ptr_eq(
        &cancelled,
        &controller.shared.cancel.lock().unwrap()
    ));
    assert!(!cancelled.load(Ordering::Acquire));
    assert!(!controller.play_requested());
}

#[test]
fn source_evidence_cannot_authorize_recovery_on_another_controller() {
    let (a, _a_receiver) = harness();
    let evidence = source_evidence(&a, TransportState::Ended, 0);
    let (b, b_receiver) = harness();
    assert_eq!(
        a.shared.epoch.load(Ordering::Acquire),
        b.shared.epoch.load(Ordering::Acquire)
    );
    assert!(
        !b.submit_source_recovery(&evidence, Some(replacement()), true)
            .unwrap()
    );
    assert_eq!(b.shared.epoch.load(Ordering::Acquire), 0);
    assert!(!b.shared.cancel.lock().unwrap().load(Ordering::Acquire));
    assert!(!b.play_requested());
    assert!(b_receiver.try_recv().is_err());
}

#[test]
fn seek_generation_does_not_change_the_source_operation_identity() {
    let (controller, receiver) = harness();
    controller.submit(open_command("synthetic-a.wav")).unwrap();
    let queued = receiver.recv().unwrap();
    let mut state = WorkerState::new(None, false);
    state.source_epoch = queued.epoch;
    state.transport_revision = queued.transport_revision;
    state.source_cancelled = queued.cancelled;
    state
        .commit_source(
            engine(Arc::clone(&state.source_cancelled)),
            None,
            &controller.shared,
        )
        .unwrap();
    let evidence = source_evidence(
        &controller,
        TransportState::Ready,
        queued.transport_revision,
    );
    let subscription = controller.subscribe_source_snapshots();
    controller
        .submit(PlayerCommand::SeekFrame(SourceFrame::new(1)))
        .unwrap();
    let seek = receiver.recv().unwrap();
    assert_eq!(seek.epoch, state.source_epoch);
    assert!(Arc::ptr_eq(&seek.cancelled, &state.source_cancelled));
    state.handle(seek, &controller.shared).unwrap();
    assert_ne!(
        controller.latest_snapshot().generation(),
        evidence.snapshot().generation()
    );
    assert!(
        !controller
            .submit_source_recovery(&evidence, None, true)
            .unwrap()
    );
    let current = subscription.recv_timeout(Duration::ZERO).unwrap();
    assert!(
        controller
            .submit_source_recovery(&current, None, true)
            .unwrap()
    );
    let recovery = receiver.recv().unwrap();
    assert!(matches!(
        recovery.command,
        WorkerCommand::SourceRecovery {
            source: None,
            play: false
        }
    ));
    state.handle(recovery, &controller.shared).unwrap();
    assert_eq!(
        controller.latest_snapshot().transport_state(),
        TransportState::Empty
    );
    assert!(state.engine.is_none());
    assert!(!controller.play_requested());
}

#[test]
fn source_snapshot_delivery_coalesces_with_the_matching_operation() {
    let (controller, receiver) = harness();
    let subscription = controller.subscribe_source_snapshots();
    controller.submit(open_command("synthetic-a.wav")).unwrap();
    let a = receiver.recv().unwrap();
    let a_evidence = source_evidence(&controller, TransportState::Ready, a.transport_revision);
    controller.submit(open_command("synthetic-b.wav")).unwrap();
    let b = receiver.recv().unwrap();
    let b_evidence = source_evidence(&controller, TransportState::Ended, b.transport_revision);
    let delivered = subscription.recv_timeout(Duration::ZERO).unwrap();
    assert_eq!(
        delivered.snapshot().transport_state(),
        TransportState::Ended
    );
    assert!(!a_evidence.matches_admission(b.epoch, b.transport_revision, &b.cancelled));
    assert!(b_evidence.matches_admission(b.epoch, b.transport_revision, &b.cancelled));
    assert!(delivered.matches_admission(b.epoch, b.transport_revision, &b.cancelled));
    assert!(!delivered.matches_admission(a.epoch, a.transport_revision, &a.cancelled));
    assert!(subscription.recv_timeout(Duration::ZERO).is_none());
    assert!(
        controller
            .submit_source_recovery(&delivered, None, false)
            .unwrap()
    );
}

#[test]
fn newer_transport_admission_rejects_delivered_end_or_error_without_other_effects() {
    for command in [
        PlayerCommand::Pause,
        PlayerCommand::Stop,
        PlayerCommand::Play,
        PlayerCommand::SeekFrame(SourceFrame::new(1)),
        PlayerCommand::SeekTime(stemplayer_domain::Microseconds::new(1)),
    ] {
        for snapshot_state in [TransportState::Ended, TransportState::Error] {
            let (controller, receiver) = harness();
            controller.submit(open_command("synthetic-a.wav")).unwrap();
            let a = receiver.recv().unwrap();
            let evidence = source_evidence(&controller, snapshot_state, a.transport_revision);
            controller.submit(command.clone()).unwrap();
            let revision = controller.shared.transport_revision.load(Ordering::Acquire);
            assert_eq!(revision, a.transport_revision + 1);
            assert_eq!(
                controller.latest_snapshot().transport_state(),
                snapshot_state
            );
            assert_eq!(
                controller.latest_snapshot().generation(),
                evidence.snapshot().generation()
            );
            for source in [Some(replacement()), None] {
                assert!(
                    !controller
                        .submit_source_recovery(&evidence, source, true)
                        .unwrap()
                );
                assert_eq!(controller.shared.epoch.load(Ordering::Acquire), a.epoch);
                assert_eq!(
                    controller.shared.transport_revision.load(Ordering::Acquire),
                    revision
                );
                assert!(Arc::ptr_eq(
                    &a.cancelled,
                    &controller.shared.cancel.lock().unwrap()
                ));
                assert!(!a.cancelled.load(Ordering::Acquire));
                assert_eq!(controller.play_requested(), command == PlayerCommand::Play);
            }
            let queued = receiver.try_iter().collect::<Vec<_>>();
            assert_eq!(queued.len(), 1);
            assert_eq!(queued[0].epoch, a.epoch);
            assert_eq!(queued[0].transport_revision, revision);
            assert!(
                matches!(&queued[0].command, WorkerCommand::Player(queued) if *queued == command)
            );
        }
    }
}

#[test]
fn worker_publication_cannot_borrow_the_revision_of_pending_transport_intent() {
    for command in [
        PlayerCommand::Pause,
        PlayerCommand::Stop,
        PlayerCommand::SeekFrame(SourceFrame::new(1)),
    ] {
        let (controller, receiver) = harness();
        controller.submit(open_command("synthetic-a.wav")).unwrap();
        let a = receiver.recv().unwrap();
        let mut state = WorkerState::new(None, false);
        state.source_epoch = a.epoch;
        state.transport_revision = a.transport_revision;
        state.source_cancelled = Arc::clone(&a.cancelled);
        state
            .commit_source(
                engine(Arc::clone(&state.source_cancelled)),
                None,
                &controller.shared,
            )
            .unwrap();
        controller.submit(command.clone()).unwrap();
        let subscription = controller.subscribe_source_snapshots();
        state.publish(&controller.shared).unwrap();
        let evidence = subscription.recv_timeout(Duration::ZERO).unwrap();
        assert!(evidence.matches_admission(a.epoch, a.transport_revision, &a.cancelled));
        assert!(
            !controller
                .submit_source_recovery(&evidence, Some(replacement()), true)
                .unwrap()
        );
        assert_eq!(
            controller.shared.transport_revision.load(Ordering::Acquire),
            a.transport_revision + 1
        );
        assert!(!a.cancelled.load(Ordering::Acquire));
        let pending = receiver.recv().unwrap();
        assert!(matches!(&pending.command, WorkerCommand::Player(pending) if *pending == command));
        assert!(receiver.try_recv().is_err());
    }
}

#[test]
fn latest_processed_transport_end_still_admits_recovery() {
    let (controller, receiver) = harness();
    controller.submit(open_command("synthetic-a.wav")).unwrap();
    let a = receiver.recv().unwrap();
    let mut state = WorkerState::new(None, false);
    state.source_epoch = a.epoch;
    state.transport_revision = a.transport_revision;
    state.source_cancelled = Arc::clone(&a.cancelled);
    state
        .commit_source(
            engine(Arc::clone(&state.source_cancelled)),
            None,
            &controller.shared,
        )
        .unwrap();
    let subscription = controller.subscribe_source_snapshots();
    controller
        .submit(PlayerCommand::SeekFrame(SourceFrame::new(96_000)))
        .unwrap();
    state
        .handle(receiver.recv().unwrap(), &controller.shared)
        .unwrap();
    let evidence = subscription.recv_timeout(Duration::ZERO).unwrap();
    assert_eq!(evidence.snapshot().transport_state(), TransportState::Ended);
    assert!(evidence.matches_admission(a.epoch, state.transport_revision, &a.cancelled));
    assert!(
        controller
            .submit_source_recovery(&evidence, Some(replacement()), true)
            .unwrap()
    );
    assert!(matches!(
        receiver.recv().unwrap().command,
        WorkerCommand::SourceRecovery {
            source: Some(_),
            play: true
        }
    ));
    assert!(controller.play_requested());
    assert!(receiver.try_recv().is_err());
}

#[test]
fn exhausted_transport_revision_rejects_admission_without_effects() {
    let (controller, receiver) = harness();
    controller
        .shared
        .transport_revision
        .store(u64::MAX, Ordering::Release);
    let evidence = source_evidence(&controller, TransportState::Ended, u64::MAX);
    let cancelled = Arc::clone(&controller.shared.cancel.lock().unwrap());
    assert_eq!(
        controller.submit(PlayerCommand::Pause).unwrap_err().code,
        ErrorCode::ValueOutOfRange
    );
    assert_eq!(
        controller
            .submit_source_recovery(&evidence, Some(replacement()), true)
            .unwrap_err()
            .code,
        ErrorCode::ValueOutOfRange
    );
    assert_eq!(controller.shared.epoch.load(Ordering::Acquire), 0);
    assert_eq!(
        controller.shared.transport_revision.load(Ordering::Acquire),
        u64::MAX
    );
    assert!(Arc::ptr_eq(
        &cancelled,
        &controller.shared.cancel.lock().unwrap()
    ));
    assert!(!cancelled.load(Ordering::Acquire));
    assert!(!controller.play_requested());
    assert!(receiver.try_recv().is_err());
}
