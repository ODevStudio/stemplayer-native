use super::*;

const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct DrainResume {
    required: bool,
    remaining: Option<Duration>,
    not_before: Option<Instant>,
    resume_position: Option<(u64, u64)>,
}

impl DrainResume {
    pub(super) fn pause(&mut self, remaining: Option<Duration>, now: Instant) {
        self.remaining = if self.required {
            match (self.remaining, remaining) {
                (Some(previous), Some(observed)) => Some(
                    observed.max(
                        self.not_before
                            .map_or(previous, |deadline| deadline.saturating_duration_since(now)),
                    ),
                ),
                _ => None,
            }
        } else {
            remaining
        };
        self.required = true;
        self.not_before = None;
        self.resume_position = None;
    }

    pub(super) fn resume(&mut self, now: Instant, position: Option<(u64, u64)>) {
        if self.required {
            self.not_before = self
                .remaining
                .and_then(|remaining| now.checked_add(remaining));
            self.resume_position = position;
        }
    }

    fn presentation_ready(
        &self,
        observed: bool,
        estimate: stemplayer_output::PresentationEstimate,
        now: Instant,
        elapsed: Duration,
    ) -> Result<bool, PlayerError> {
        let fresh = self.resume_position.is_some_and(|(generation, consumed)| {
            estimate.generation == generation
                && estimate.submitted_device_frames > consumed
                && estimate.submitted_device_frames == estimate.produced_device_frames
                && estimate.remaining == Some(Duration::ZERO)
        });
        if !self.required || fresh || self.not_before.is_some_and(|deadline| now >= deadline) {
            return Ok(observed);
        }
        if elapsed >= OUTPUT_DRAIN_TIMEOUT {
            return Err(PlayerError::new(
                ErrorCode::PlaybackNotReady,
                "Resumed output presentation estimate is unavailable",
            ));
        }
        Ok(false)
    }
}

impl WorkerState {
    pub(super) fn ensure_output(&mut self) -> Result<(), PlayerError> {
        if self.output.is_none() {
            let engine = self.engine.as_ref().ok_or_else(not_ready)?;
            let source_rate = engine.sample_rate().get();
            let remaining = engine.remaining_source_frames();
            let output =
                DeviceOutput::open(self.selected_device.as_deref(), OUTPUT_CAPACITY_FRAMES)?;
            let output_rate = output.negotiated().sample_rate;
            let input_chunk = resampler_input_frames(source_rate, output_rate);
            let resampler =
                PostMixResampler::new(source_rate, output_rate, input_chunk, remaining)?;
            self.active_tick =
                ACTIVE_TICK.min(output.refill_interval(resampler.maximum_output_frames())?);
            self.output_clock = Some(AudioClock::new(source_rate, output_rate)?);
            self.output_origin = engine.position();
            self.paused_position = None;
            self.output_drain_started = None;
            self.drain_resume = DrainResume::default();
            self.resampler = Some(resampler);
            self.output = Some(output);
            self.last_output_underflows = 0;
        }
        Ok(())
    }

    pub(super) fn clear_output(&mut self) -> Result<(), PlayerError> {
        self.paused_position = self.engine.as_ref().map(PlaybackEngine::position);
        if let Some(output) = self.output.as_mut() {
            output.clear(CLEAR_TIMEOUT)?;
        }
        self.last_output_underflows = 0;
        self.paused_position = None;
        self.output_drain_started = None;
        self.drain_resume = DrainResume::default();
        Ok(())
    }

    pub(super) fn close_output(&mut self) -> Result<(), PlayerError> {
        self.output_reopen_at = None;
        self.output_lifecycle.stop();
        self.resampler = None;
        self.output_clock = None;
        self.paused_position = None;
        self.output_drain_started = None;
        self.drain_resume = DrainResume::default();
        self.last_output_underflows = 0;
        if let Some(mut output) = self.output.take() {
            output.pause()?;
        }
        Ok(())
    }

    pub(super) fn maintain_output(&mut self, shared: &Shared) -> Result<(), PlayerError> {
        if self
            .output
            .as_ref()
            .is_some_and(|output| output.stream_errors() > 0)
        {
            let audible = self.audible_position();
            self.pause_for_output_fault()?;
            if let Some(frame) = audible {
                self.seek_before_source_end(frame)?;
            }
            self.output = None;
            self.resampler = None;
            self.output_clock = None;
            self.paused_position = None;
            self.output_drain_started = None;
            self.drain_resume = DrainResume::default();
            self.output_lifecycle.device_lost();
            self.schedule_output_reopen()?;
            return self.publish(shared);
        }
        if self
            .output_reopen_at
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.output_reopen_at = None;
            match self.ensure_output() {
                Ok(()) => {
                    self.output_lifecycle.reopened();
                    self.publish(shared)?;
                }
                Err(_) => self.schedule_output_reopen()?,
            }
        }
        Ok(())
    }

    pub(super) fn schedule_output_reopen(&mut self) -> Result<(), PlayerError> {
        match self.output_lifecycle.retry_reopen() {
            LifecycleAction::ReopenAfter(delay) => {
                self.output_reopen_at = Some(Instant::now() + delay);
                Ok(())
            }
            LifecycleAction::ReportRecoverableError => Err(PlayerError::new(
                ErrorCode::PlaybackNotReady,
                "Audio output device could not be recovered",
            )
            .with_user_action("Choose an available audio output device")),
            _ => Ok(()),
        }
    }

    pub(super) fn pause_for_output_fault(&mut self) -> Result<(), PlayerError> {
        if let Some(engine) = self.engine.as_mut()
            && matches!(
                engine.state(),
                TransportState::Playing | TransportState::Priming
            )
        {
            engine.pause()?;
        }
        Ok(())
    }

    pub(super) fn output_failed(&mut self, error: PlayerError) -> PlayerError {
        let frame = self.audible_position();
        let pause_error = self.pause_for_output_fault().err();
        let seek_error = if let Some(frame) = frame {
            self.seek_before_source_end(frame).err()
        } else {
            None
        };
        if seek_error.is_some() {
            if let Some(engine) = self.engine.as_ref() {
                self.generation = engine.generation();
            }
            self.source_cancelled.store(true, Ordering::Release);
            self.engine = None;
            self.analysis = None;
            self.hls = None;
        }
        let close_error = self.close_output().err();
        let error = close_error.or(pause_error).or(seek_error).unwrap_or(error);
        self.output_error = Some(error.clone());
        error
    }

    pub(super) fn seek_before_source_end(&mut self, frame: SourceFrame) -> Result<(), PlayerError> {
        let engine = self.engine_mut()?;
        let end = engine
            .position()
            .get()
            .saturating_add(i64::try_from(engine.remaining_source_frames()).unwrap_or(i64::MAX));
        engine.seek(SourceFrame::new(frame.get().min(end.saturating_sub(1))))?;
        Ok(())
    }

    pub(super) fn engine_mut(&mut self) -> Result<&mut PlaybackEngine, PlayerError> {
        self.engine.as_mut().ok_or_else(not_ready)
    }

    pub(super) fn pump(&mut self) -> Result<(), PlayerError> {
        if self.source_cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        let Some(engine) = self.engine.as_mut() else {
            return Ok(());
        };
        engine.refill()?;
        if engine.state() != TransportState::Playing {
            return Ok(());
        }
        let Some(output) = self.output.as_mut() else {
            return Ok(());
        };
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(());
        };
        let starting = !output.is_active();
        let underflows = output.metrics().underflows;
        let recovering = underflows > self.last_output_underflows && !engine.source_exhausted();
        self.last_output_underflows = underflows;
        if recovering {
            output.pause()?;
        }
        let mut rendered = [StereoFrame::default(); RENDER_FRAMES];
        while output.available_frames() >= resampler.maximum_output_frames()
            && !resampler.finished()
        {
            if self.source_cancelled.load(Ordering::Acquire) {
                return Ok(());
            }
            let needed = resampler.input_frames_next();
            if needed > rendered.len() {
                return Err(PlayerError::new(
                    ErrorCode::FeatureUnsupported,
                    "The device sample-rate ratio exceeds the bounded render chunk",
                ));
            }
            let produced = if engine.source_exhausted() {
                0
            } else {
                engine.render(&mut rendered[..needed])?
            };
            if produced == 0 && !engine.source_exhausted() {
                break;
            }
            if produced < needed && !engine.source_exhausted() {
                return Err(PlayerError::new(
                    ErrorCode::Internal,
                    "Playback produced an incomplete non-final render window",
                ));
            }
            let converted = resampler.process(&rendered[..produced], engine.source_exhausted())?;
            let converted_len = converted.len();
            if output.push(converted) != converted_len {
                return Err(PlayerError::new(
                    ErrorCode::Internal,
                    "Output queue accepted a partial render block",
                ));
            }
            if produced == 0 && converted_len == 0 && !resampler.finished() {
                return Err(PlayerError::new(
                    ErrorCode::Internal,
                    "Resampler made no progress while draining",
                ));
            }
        }
        if resampler.finished() {
            output.begin_drain();
        }
        let drained = if resampler.finished() {
            let started = self.output_drain_started.get_or_insert_with(Instant::now);
            let elapsed = started.elapsed();
            let observed = output.estimated_drain_complete(elapsed, OUTPUT_DRAIN_TIMEOUT)?;
            self.drain_resume.presentation_ready(
                observed,
                output.presentation_estimate(),
                Instant::now(),
                elapsed,
            )?
        } else {
            false
        };
        if drained && engine.finish_if_output_drained(output.queued_frames())? {
            output.pause()?;
        } else if recovering || starting {
            output.play()?;
        }
        Ok(())
    }

    pub(super) fn reset_resampler(&mut self, discontinuity: bool) -> Result<(), PlayerError> {
        let remaining = self
            .engine
            .as_ref()
            .ok_or_else(not_ready)?
            .remaining_source_frames();
        if let Some(resampler) = self.resampler.as_mut() {
            resampler.reset(remaining);
            if discontinuity {
                resampler.record_discontinuity();
            }
        }
        if let (Some(engine), Some(output)) = (self.engine.as_ref(), self.output.as_ref()) {
            self.output_clock = Some(AudioClock::new(
                engine.sample_rate().get(),
                output.negotiated().sample_rate,
            )?);
            self.output_origin = engine.position();
        }
        self.paused_position = None;
        self.output_drain_started = None;
        self.drain_resume = DrainResume::default();
        Ok(())
    }

    pub(super) fn select_device(
        &mut self,
        device: Option<String>,
        shared: &Shared,
    ) -> Result<(), PlayerError> {
        let resume = self
            .engine
            .as_ref()
            .is_some_and(PlaybackEngine::playback_requested);
        let audible = self.audible_position();
        self.selected_device = device;
        if !self.source_is_current(shared) {
            self.engine = None;
            self.analysis = None;
            self.hls = None;
            return Ok(());
        }
        if let Some(frame) = audible {
            if self
                .engine
                .as_ref()
                .is_some_and(|engine| engine.state() == TransportState::Ended)
            {
                self.engine_mut()?.seek(frame)?;
            } else {
                self.seek_before_source_end(frame)?;
            }
        }
        self.close_output()?;
        if !self.source_is_current(shared) {
            self.engine = None;
            self.analysis = None;
            self.hls = None;
            return Ok(());
        }
        if self.engine.is_some() {
            self.ensure_output()?;
            self.reset_resampler(true)?;
            if resume {
                self.pump()?;
            }
        }
        self.output_error = None;
        self.publish(shared)
    }

    pub(super) fn audible_position(&self) -> Option<SourceFrame> {
        let engine = self.engine.as_ref()?;
        if let Some(position) = self.paused_position {
            return Some(position);
        }
        let output = self.output.as_ref()?;
        let mut clock = self.output_clock.clone()?;
        clock.synchronize_output(&output.metrics());
        let offset = i64::try_from(clock.snapshot().source_frames).unwrap_or(i64::MAX);
        let end = engine
            .position()
            .get()
            .saturating_add(i64::try_from(engine.remaining_source_frames()).unwrap_or(i64::MAX));
        Some(SourceFrame::new(
            self.output_origin.get().saturating_add(offset).min(end),
        ))
    }
}

pub(super) fn with_output_clipping(
    snapshot: &PlayerSnapshot,
    clipped_samples: u64,
) -> Result<PlayerSnapshot, PlayerError> {
    use stemplayer_domain::{BoundedText, DiagnosticDetail, PublicDiagnostic};

    let mut diagnostics = snapshot.diagnostics().to_vec();
    if diagnostics.len() < stemplayer_domain::MAX_SNAPSHOT_DIAGNOSTICS
        && let Some(diagnostic) = PublicDiagnostic::new(
            "D_OUTPUT_CLIPPING",
            "Integer output samples exceeded the device range",
            Severity::Warning,
            vec![DiagnosticDetail {
                key: BoundedText::new("clipped_samples")?,
                value: BoundedText::new(clipped_samples.to_string())?,
            }],
        )
    {
        diagnostics.push(diagnostic);
    }
    PlayerSnapshot::new(
        snapshot.transport_state(),
        snapshot.generation(),
        snapshot.position(),
        snapshot.source().cloned(),
        diagnostics,
    )
    .map(|value| value.with_details(snapshot.details().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presentation(submitted: u64) -> stemplayer_output::PresentationEstimate {
        stemplayer_output::PresentationEstimate {
            generation: 1,
            produced_device_frames: 10,
            submitted_device_frames: submitted,
            remaining: Some(Duration::ZERO),
            confidence: stemplayer_output::LatencyConfidence::Estimated,
        }
    }

    #[test]
    fn paused_final_presentation_time_is_rebased_on_resume() {
        let start = Instant::now();
        let mut drain = DrainResume::default();
        drain.pause(Some(Duration::from_millis(20)), start);
        let resumed = start + Duration::from_secs(60);
        drain.resume(resumed, Some((1, 10)));
        assert!(
            !drain
                .presentation_ready(true, presentation(10), resumed, Duration::ZERO)
                .unwrap()
        );
        assert!(
            drain
                .presentation_ready(
                    true,
                    presentation(10),
                    resumed + Duration::from_millis(20),
                    Duration::from_millis(20)
                )
                .unwrap()
        );
        drain.pause(Some(Duration::ZERO), resumed + Duration::from_millis(5));
        let resumed_again = resumed + Duration::from_secs(60);
        drain.resume(resumed_again, Some((1, 10)));
        assert!(
            !drain
                .presentation_ready(true, presentation(10), resumed_again, Duration::ZERO)
                .unwrap()
        );
        assert!(
            drain
                .presentation_ready(
                    true,
                    presentation(10),
                    resumed_again + Duration::from_millis(15),
                    Duration::from_millis(15)
                )
                .unwrap()
        );
    }

    #[test]
    fn unknown_paused_presentation_time_fails_closed() {
        let now = Instant::now();
        let mut drain = DrainResume::default();
        drain.pause(None, now);
        drain.resume(now, Some((1, 10)));
        assert!(
            !drain
                .presentation_ready(true, presentation(10), now, Duration::ZERO)
                .unwrap()
        );
        assert!(
            drain
                .presentation_ready(
                    true,
                    presentation(10),
                    now + OUTPUT_DRAIN_TIMEOUT,
                    OUTPUT_DRAIN_TIMEOUT
                )
                .is_err()
        );
    }

    #[test]
    fn fresh_post_resume_submission_can_complete_an_unknown_paused_tail() {
        let now = Instant::now();
        let mut drain = DrainResume::default();
        drain.pause(None, now);
        drain.resume(now, Some((1, 0)));
        assert!(
            !drain
                .presentation_ready(false, presentation(0), now, Duration::ZERO)
                .unwrap()
        );
        assert!(
            drain
                .presentation_ready(true, presentation(10), now, Duration::ZERO)
                .unwrap()
        );
        let wrong_generation = stemplayer_output::PresentationEstimate {
            generation: 2,
            ..presentation(10)
        };
        assert!(
            !drain
                .presentation_ready(true, wrong_generation, now, Duration::ZERO)
                .unwrap()
        );
    }

    #[test]
    fn integer_clipping_is_diagnostic_without_a_transport_failure() {
        let snapshot = PlayerSnapshot::empty(TransportState::Playing);
        let snapshot = with_output_clipping(&snapshot, 7).unwrap();
        assert_eq!(snapshot.transport_state(), TransportState::Playing);
        assert!(snapshot.details().error.is_none());
        let serialized = serde_json::to_value(snapshot).unwrap();
        assert_eq!(serialized["diagnostics"][0]["details"][0]["value"], "7");
    }
}
