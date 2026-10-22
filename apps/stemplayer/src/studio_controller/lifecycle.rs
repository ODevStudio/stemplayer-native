use super::*;

impl WorkerState {
    pub(super) fn cache_result(
        &mut self,
        result: Result<(), PlayerError>,
        shared: &Shared,
    ) -> Result<(), PlayerError> {
        if let Err(error) = result {
            {
                let _admission = shared
                    .command_admission
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if !self.source_is_current(shared) {
                    return Ok(());
                }
                self.waveform_error = Some(waveform_warning(error));
            }
            self.publish(shared)?;
        }
        Ok(())
    }

    pub(super) fn source_is_current(&self, shared: &Shared) -> bool {
        !shared.shutdown.load(Ordering::Acquire)
            && self.source_epoch == shared.epoch.load(Ordering::Acquire)
    }

    pub(super) fn begin_inspection(
        &mut self,
        path: Option<&Path>,
        shared: &Shared,
    ) -> Result<(), PlayerError> {
        self.engine = None;
        self.analysis = None;
        self.waveform_error = None;
        self.output_error = None;
        self.fingerprint = None;
        self.hls = None;
        self.current_source = path.map(Path::to_path_buf);
        self.generation = self.generation.next()?;
        self.close_output()?;
        let _admission = shared
            .command_admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.source_is_current(shared) {
            return Err(source_cancelled());
        }
        *shared
            .current_source
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = self.current_source.clone();
        *shared
            .waveform
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        publish(
            shared,
            PlayerSnapshot::new(
                TransportState::Inspecting,
                self.generation,
                None,
                None,
                vec![],
            )?,
            self.transport_revision,
        );
        Ok(())
    }

    pub(super) fn commit_source(
        &mut self,
        engine: PlaybackEngine,
        waveform: Option<WaveformSnapshot>,
        shared: &Shared,
    ) -> Result<(), PlayerError> {
        {
            let _admission = shared
                .command_admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !self.source_is_current(shared) || self.source_cancelled.load(Ordering::Acquire) {
                self.analysis = None;
                return Err(source_cancelled());
            }
            self.engine = Some(engine);
            *shared
                .waveform
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = waveform;
        }
        self.publish(shared)
    }
}

fn source_cancelled() -> PlayerError {
    PlayerError::new(ErrorCode::BundleAborted, "Source operation was cancelled")
}

pub(super) fn worker_loop(
    receiver: Receiver<QueuedCommand>,
    shared: Arc<Shared>,
    cache: Option<Arc<WaveformCache>>,
    cache_enabled: bool,
) {
    let mut state = WorkerState::new(cache, cache_enabled);
    while !shared.shutdown.load(Ordering::Acquire) {
        let playing = state.engine.as_ref().is_some_and(|engine| {
            matches!(
                engine.state(),
                TransportState::Priming | TransportState::Playing
            )
        });
        let timeout = worker_timeout(
            playing,
            state.analysis.is_some(),
            state.cache_write_pending() || state.preloader.pending(),
        );
        let timeout = if playing {
            timeout.min(state.active_tick)
        } else {
            timeout
        };
        let timeout = state.output_reopen_at.map_or(timeout, |deadline| {
            timeout.min(deadline.saturating_duration_since(Instant::now()))
        });
        match receiver.recv_timeout(timeout) {
            Ok(queued) => {
                if !should_process_command(&queued, &shared) {
                    continue;
                }
                if let Err(error) = state.handle(queued, &shared)
                    && should_publish_error(&error, state.source_cancelled.load(Ordering::Acquire))
                {
                    let error = if matches!(
                        error.code,
                        ErrorCode::Internal | ErrorCode::FeatureUnsupported
                    ) {
                        state.output_failed(error)
                    } else {
                        error
                    };
                    publish_error(&shared, &mut state, error);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if !state.source_is_current(&shared) {
            continue;
        }
        let previous_state = state.engine.as_ref().map(PlaybackEngine::state);
        state.preloader.poll();
        let tick = state
            .poll_cache_writer(&shared)
            .and_then(|()| state.maintain_output(&shared))
            .and_then(|()| state.pump())
            .and_then(|()| state.analyze(&shared))
            .and_then(|()| state.publish_if_due(&shared));
        if let Err(error) = tick {
            if should_publish_error(&error, state.source_cancelled.load(Ordering::Acquire)) {
                let error = if matches!(
                    error.category,
                    ErrorCategory::Source | ErrorCategory::Timeline | ErrorCategory::Network
                ) {
                    error
                } else {
                    state.output_failed(error)
                };
                publish_error(&shared, &mut state, error);
            }
        } else if previous_state != state.engine.as_ref().map(PlaybackEngine::state)
            && let Err(error) = state.publish(&shared)
        {
            publish_error(&shared, &mut state, error);
        }
    }
    state.source_cancelled.store(true, Ordering::Release);
    state.engine = None;
    state.analysis = None;
    state.hls = None;
    state.preloader.shutdown();
    let _ = state.close_output();
    let cache_error = state.shutdown_cache_writer().err();
    let mut snapshot =
        PlayerSnapshot::new(TransportState::Closed, state.generation, None, None, vec![])
            .unwrap_or_else(|_| PlayerSnapshot::empty(TransportState::Closed));
    if let Some(error) = cache_error {
        let mut details = snapshot.details().clone();
        details.error = Some(error);
        snapshot = snapshot.with_details(details);
    }
    publish(&shared, snapshot, state.transport_revision);
}

pub(super) fn should_process_command(queued: &QueuedCommand, shared: &Shared) -> bool {
    let _admission = shared
        .command_admission
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let current_epoch = shared.epoch.load(Ordering::Acquire);
    !shared.shutdown.load(Ordering::Acquire)
        && (queued.epoch == current_epoch
            || matches!(
                &queued.command,
                WorkerCommand::Player(PlayerCommand::SelectDevice(_) | PlayerCommand::Cache(_))
            ))
}

pub(super) fn worker_timeout(
    playing: bool,
    immediate_work_pending: bool,
    cache_write_pending: bool,
) -> Duration {
    match (playing, immediate_work_pending, cache_write_pending) {
        (true, _, _) | (false, false, true) => ACTIVE_TICK,
        (false, true, _) => Duration::ZERO,
        (false, false, false) => IDLE_TICK,
    }
}

pub(super) fn publish_error(shared: &Shared, state: &mut WorkerState, error: PlayerError) {
    let current = state
        .engine
        .as_ref()
        .and_then(|engine| engine.snapshot().ok());
    let output_error = if matches!(
        error.category,
        ErrorCategory::Source | ErrorCategory::Timeline | ErrorCategory::Network
    ) {
        state.preloader.cancel();
        state.source_cancelled.store(true, Ordering::Release);
        state.engine = None;
        state.analysis = None;
        state.hls = None;
        state.generation = current
            .as_ref()
            .map_or(state.generation, PlayerSnapshot::generation);
        state.output_error = state.close_output().err();
        state.output_error.clone()
    } else {
        None
    };
    let _admission = shared
        .command_admission
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !state.source_is_current(shared) {
        return;
    }
    let snapshot = PlayerSnapshot::new(
        TransportState::Error,
        current
            .as_ref()
            .map_or(state.generation, PlayerSnapshot::generation),
        current.as_ref().and_then(PlayerSnapshot::position),
        current.as_ref().and_then(PlayerSnapshot::source).cloned(),
        vec![],
    )
    .map(|snapshot| {
        let mut details = current
            .as_ref()
            .map_or_else(Default::default, |value| value.details().clone());
        details.error = Some(with_teardown_error(error, output_error.as_ref()));
        snapshot.with_details(details)
    })
    .unwrap_or_else(|_| PlayerSnapshot::empty(TransportState::Error));
    publish(shared, snapshot, state.transport_revision);
}

pub(super) fn with_teardown_error(
    error: PlayerError,
    teardown: Option<&PlayerError>,
) -> PlayerError {
    let Some(teardown) = teardown else {
        return error;
    };
    let detail = stemplayer_domain::ErrorDetail {
        key: stemplayer_domain::BoundedText::truncated("output_teardown_error"),
        value: stemplayer_domain::BoundedText::truncated(teardown.code.as_str()),
    };
    let details = error
        .details
        .iter()
        .cloned()
        .chain(std::iter::once(detail))
        .collect();
    error.clone().with_details(details).unwrap_or(error)
}
