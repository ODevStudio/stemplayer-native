use crate::MixerState;
use stemplayer_domain::{
    AutomationReason, GainDb, MixerSnapshot, OutputMode, OutputPreference, PlayerError, StemRole,
    StemSnapshot,
};

const GAIN_RAMP_MILLISECONDS: u32 = 10;
const CROSSFADE_MILLISECONDS: u32 = 20;

#[derive(Clone, Copy, Debug)]
struct LinearRamp {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
}

impl LinearRamp {
    const fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
            step: 0.0,
            remaining: 0,
        }
    }

    fn retarget(&mut self, target: f32, frames: u32) {
        self.target = target;
        self.remaining = frames;
        self.step = if frames == 0 {
            self.current = target;
            0.0
        } else {
            (target - self.current) / frames as f32
        };
    }

    fn next(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            self.current = if self.remaining == 0 {
                self.target
            } else {
                self.current + self.step
            };
        }
        self.current
    }

    fn prime(&mut self) {
        self.current = self.target;
        self.step = 0.0;
        self.remaining = 0;
    }
}

pub struct MixerRenderer {
    state: MixerState,
    has_master: bool,
    gain_ramp_frames: u32,
    crossfade_frames: u32,
    gains: [LinearRamp; 4],
    master_gain: LinearRamp,
    master_volume: LinearRamp,
    automation_count: u64,
    crossfade_count: u64,
    last_automation_reason: Option<AutomationReason>,
    source_peak: [f32; 2],
    output_peak: [f32; 2],
    non_finite_samples: u64,
}

impl MixerRenderer {
    pub fn new(sample_rate: u32, has_master: bool) -> Self {
        let state = MixerState::default();
        let gains = state.effective_linear_gains().map(LinearRamp::new);
        let master =
            f32::from(state.effective_output(has_master).ok() == Some(OutputMode::MasterReference));
        let master_volume = state.coherent_gain().map(GainDb::linear).unwrap_or(1.0);
        Self {
            state,
            has_master,
            gain_ramp_frames: duration_frames(sample_rate, GAIN_RAMP_MILLISECONDS),
            crossfade_frames: duration_frames(sample_rate, CROSSFADE_MILLISECONDS),
            gains,
            master_gain: LinearRamp::new(master),
            master_volume: LinearRamp::new(master_volume),
            automation_count: 0,
            crossfade_count: 0,
            last_automation_reason: None,
            source_peak: [0.0; 2],
            output_peak: [0.0; 2],
            non_finite_samples: 0,
        }
    }

    pub fn set_gain(&mut self, role: StemRole, gain_db: GainDb) {
        self.apply(self.state.with_gain(role, gain_db), AutomationReason::Gain);
    }

    pub fn set_mute(&mut self, role: StemRole, muted: bool) {
        self.apply(self.state.with_mute(role, muted), AutomationReason::Mute);
    }

    pub fn set_solo(&mut self, role: StemRole, solo: bool) {
        self.apply(self.state.with_solo(role, solo), AutomationReason::Solo);
    }

    pub fn reset(&mut self) {
        self.apply(self.state.reset(), AutomationReason::Reset);
    }

    pub fn set_output_preference(
        &mut self,
        preference: OutputPreference,
    ) -> Result<(), PlayerError> {
        let state = self.state.with_preference(preference, self.has_master)?;
        self.apply(state, AutomationReason::OutputPreference);
        Ok(())
    }

    pub fn begin_block(&mut self) {
        self.source_peak = [0.0; 2];
        self.output_peak = [0.0; 2];
    }

    pub fn render(&mut self, source: &[f32]) -> [f32; 2] {
        let mut stem = [0.0; 2];
        for role in StemRole::ALL {
            let index = role.index() * 2;
            let gain = self.gains[role.index()].next();
            let left = self.finite(source[index]);
            let right = self.finite(source[index + 1]);
            self.source_peak[0] = self.source_peak[0].max(left.abs());
            self.source_peak[1] = self.source_peak[1].max(right.abs());
            stem[0] += left * gain;
            stem[1] += right * gain;
        }
        let master_volume = self.master_volume.next();
        let master = if self.has_master {
            let left = self.finite(source[8]);
            let right = self.finite(source[9]);
            self.source_peak[0] = self.source_peak[0].max(left.abs());
            self.source_peak[1] = self.source_peak[1].max(right.abs());
            [left * master_volume, right * master_volume]
        } else {
            [0.0; 2]
        };
        let master_gain = self.master_gain.next();
        let stem_gain = 1.0 - master_gain;
        let output = if master_gain == 0.0 {
            stem
        } else if master_gain == 1.0 {
            master
        } else {
            [
                stem[0] * stem_gain + master[0] * master_gain,
                stem[1] * stem_gain + master[1] * master_gain,
            ]
        };
        let output = output.map(|sample| self.finite(sample));
        self.output_peak[0] = self.output_peak[0].max(output[0].abs());
        self.output_peak[1] = self.output_peak[1].max(output[1].abs());
        output
    }

    pub fn prime(&mut self) {
        for gain in &mut self.gains {
            gain.prime();
        }
        self.master_gain.prime();
        self.master_volume.prime();
    }

    pub fn snapshot(&self) -> MixerSnapshot {
        let stems = StemRole::ALL.map(|role| {
            let control = self.state.control(role);
            StemSnapshot {
                role,
                gain_db: control.gain_db,
                muted: control.muted,
                solo: control.solo,
            }
        });
        MixerSnapshot {
            stems,
            output_preference: self.state.preference(),
            output_mode: self
                .state
                .effective_output(self.has_master)
                .unwrap_or(OutputMode::StemMix),
            automation_count: self.automation_count,
            crossfade_count: self.crossfade_count,
            current_linear_gains: self.gains.map(|gain| gain.current),
            target_linear_gains: self.gains.map(|gain| gain.target),
            remaining_gain_frames: self.gains.map(|gain| gain.remaining),
            current_master_gain: self.master_gain.current,
            target_master_gain: self.master_gain.target,
            remaining_crossfade_frames: self.master_gain.remaining,
            last_automation_reason: self.last_automation_reason,
            source_peak: self.source_peak,
            output_peak: self.output_peak,
            non_finite_samples: self.non_finite_samples,
        }
    }

    fn apply(&mut self, state: MixerState, reason: AutomationReason) {
        if state == self.state {
            return;
        }
        let old_mode = self.state.effective_output(self.has_master).ok();
        let new_mode = state.effective_output(self.has_master).ok();
        self.state = state;
        for (ramp, target) in self.gains.iter_mut().zip(state.effective_linear_gains()) {
            if ramp.target != target {
                ramp.retarget(target, self.gain_ramp_frames);
            }
        }
        if let Some(target) = state.coherent_gain().map(GainDb::linear)
            && self.master_volume.target != target
        {
            self.master_volume.retarget(target, self.gain_ramp_frames);
        }
        if old_mode != new_mode {
            self.master_gain.retarget(
                f32::from(new_mode == Some(OutputMode::MasterReference)),
                self.crossfade_frames,
            );
            self.crossfade_count = self.crossfade_count.saturating_add(1);
        }
        self.automation_count = self.automation_count.saturating_add(1);
        self.last_automation_reason = Some(reason);
    }

    fn finite(&mut self, sample: f32) -> f32 {
        if sample.is_finite() {
            sample
        } else {
            self.non_finite_samples = self.non_finite_samples.saturating_add(1);
            0.0
        }
    }
}

fn duration_frames(sample_rate: u32, milliseconds: u32) -> u32 {
    ((u64::from(sample_rate) * u64::from(milliseconds) + 500) / 1_000)
        .max(1)
        .min(u64::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(stems: [f32; 4], master: f32) -> [f32; 10] {
        [
            stems[0], stems[0], stems[1], stems[1], stems[2], stems[2], stems[3], stems[3], master,
            master,
        ]
    }

    #[test]
    fn every_stem_and_channel_routes_once_in_order() {
        let mut mixer = MixerRenderer::new(48_000, false);
        for role in StemRole::ALL {
            let mut frame = [0.0; 8];
            frame[role.index() * 2] = 1.0;
            frame[role.index() * 2 + 1] = 2.0;
            assert_eq!(mixer.render(&frame), [1.0, 2.0]);
        }
    }

    #[test]
    fn gain_ramp_reaches_the_target_in_exact_source_frames() {
        let mut mixer = MixerRenderer::new(1_000, false);
        mixer.set_gain(StemRole::Drums, GainDb::SILENCE);
        let input = source([1.0, 0.0, 0.0, 0.0], 0.0);
        let output = (0..10).map(|_| mixer.render(&input)[0]).collect::<Vec<_>>();
        assert!((output[0] - 0.9).abs() < 0.000_001);
        assert_eq!(output[9], 0.0);
        assert_eq!(mixer.snapshot().remaining_gain_frames[0], 0);
    }

    #[test]
    fn mute_overrides_multiple_solos_without_losing_gain() {
        let mut mixer = MixerRenderer::new(1_000, false);
        mixer.set_gain(StemRole::Bass, GainDb::new(-6.0).unwrap());
        mixer.prime();
        mixer.set_solo(StemRole::Drums, true);
        mixer.set_solo(StemRole::Bass, true);
        mixer.set_mute(StemRole::Drums, true);
        mixer.prime();
        let output = mixer.render(&source([1.0; 4], 0.0));
        assert!((output[0] - GainDb::new(-6.0).unwrap().linear()).abs() < 0.000_001);
        assert_eq!(
            mixer.snapshot().stems[StemRole::Bass.index()].gain_db,
            GainDb::new(-6.0).unwrap()
        );
    }

    #[test]
    fn crossfade_is_complementary_and_retargets_from_the_current_value() {
        let mut mixer = MixerRenderer::new(1_000, true);
        let input = source([0.0; 4], 1.0);
        mixer
            .set_output_preference(OutputPreference::StemMix)
            .unwrap();
        for frame in 1..=10 {
            let output = mixer.render(&input)[0];
            assert!((output - (1.0 - frame as f32 / 20.0)).abs() < 0.000_001);
        }
        let before = mixer.snapshot().current_master_gain;
        mixer
            .set_output_preference(OutputPreference::MasterReference)
            .unwrap();
        let after = mixer.render(&input)[0];
        assert!((after - (before + (1.0 - before) / 20.0)).abs() < 0.000_001);
        assert_eq!(mixer.snapshot().crossfade_count, 2);
    }

    #[test]
    fn coherent_stem_gains_scale_the_master_reference() {
        let mut mixer = MixerRenderer::new(1_000, true);
        let gain = GainDb::new(-6.0).unwrap();
        for role in StemRole::ALL {
            mixer.set_gain(role, gain);
        }
        mixer.prime();
        let output = mixer.render(&source([0.0; 4], 1.0));
        assert!((output[0] - gain.linear()).abs() < 0.000_001);
        assert_eq!(mixer.snapshot().output_mode, OutputMode::MasterReference);
    }

    #[test]
    fn complementary_crossfade_keeps_equal_buses_at_unity() {
        let mut mixer = MixerRenderer::new(1_000, true);
        let input = source([0.25; 4], 1.0);
        mixer
            .set_output_preference(OutputPreference::StemMix)
            .unwrap();
        for _ in 0..20 {
            assert!((mixer.render(&input)[0] - 1.0).abs() < 0.000_001);
        }
    }

    #[test]
    fn master_reference_ignores_overflowing_stems_at_exact_endpoints() {
        let mut mixer = MixerRenderer::new(1_000, true);
        let input = source([f32::MAX, f32::MAX, 0.0, 0.0], -2.5);
        assert_eq!(mixer.render(&input), [-2.5; 2]);
        assert_eq!(mixer.snapshot().non_finite_samples, 0);
        mixer
            .set_output_preference(OutputPreference::StemMix)
            .unwrap();
        mixer.prime();
        mixer.set_mute(StemRole::Vocals, true);
        mixer
            .set_output_preference(OutputPreference::MasterReference)
            .unwrap();
        for _ in 0..19 {
            assert_eq!(mixer.render(&input), [0.0; 2]);
        }
        assert_eq!(mixer.render(&input), [-2.5; 2]);
        let snapshot = mixer.snapshot();
        assert_eq!(snapshot.current_master_gain, 1.0);
        assert_eq!(snapshot.remaining_crossfade_frames, 0);
        assert_eq!(snapshot.current_linear_gains[3], 0.0);
        assert_eq!(snapshot.remaining_gain_frames[3], 0);
        assert_eq!(snapshot.non_finite_samples, 38);
    }

    #[test]
    fn stem_mix_ignores_overflowing_master_at_the_crossfade_endpoint() {
        let mut mixer = MixerRenderer::new(1_000, true);
        let gain = GainDb::new(GainDb::MAX).unwrap();
        for role in StemRole::ALL {
            mixer.set_gain(role, gain);
        }
        mixer.prime();
        mixer
            .set_output_preference(OutputPreference::StemMix)
            .unwrap();
        let input = source([2.5, 0.0, 0.0, 0.0], f32::MAX);
        for _ in 0..19 {
            assert_eq!(mixer.render(&input), [0.0; 2]);
        }
        assert_eq!(mixer.render(&input), [2.5 * gain.linear(); 2]);
        let snapshot = mixer.snapshot();
        assert_eq!(snapshot.current_master_gain, 0.0);
        assert_eq!(snapshot.remaining_crossfade_frames, 0);
        assert_eq!(snapshot.non_finite_samples, 38);
    }

    #[test]
    fn arithmetic_overflow_is_silenced_without_clipping_finite_output() {
        let mut mixer = MixerRenderer::new(1_000, false);
        let overflowing = [f32::MAX, -3.0, f32::MAX, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert_eq!(mixer.render(&overflowing), [0.0, -3.0]);
        assert_eq!(mixer.snapshot().output_peak, [0.0, 3.0]);
        assert_eq!(mixer.snapshot().non_finite_samples, 1);
        mixer.begin_block();
        let finite = [f32::MAX, -2.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert_eq!(mixer.render(&finite), [f32::MAX, -2.5]);
        assert_eq!(mixer.snapshot().output_peak, [f32::MAX, 2.5]);
        assert_eq!(mixer.snapshot().non_finite_samples, 1);
    }

    #[test]
    fn rapid_retargeting_remains_finite_and_bounded() {
        let mut mixer = MixerRenderer::new(48_000, true);
        let input = source([0.25; 4], 1.0);
        for index in 0..10_000 {
            mixer.set_mute(StemRole::Vocals, index % 2 == 0);
            mixer
                .set_output_preference(if index % 2 == 0 {
                    OutputPreference::StemMix
                } else {
                    OutputPreference::MasterReference
                })
                .unwrap();
            let output = mixer.render(&input);
            assert!(output[0].is_finite());
            assert!(output[1].is_finite());
        }
        let snapshot = mixer.snapshot();
        assert_eq!(snapshot.automation_count, 20_000);
        assert_eq!(snapshot.crossfade_count, 10_000);
        assert!(snapshot.remaining_gain_frames[3] <= 480);
        assert!(snapshot.remaining_crossfade_frames <= 960);
    }

    #[test]
    fn reset_returns_automatic_master_and_non_finite_input_is_silenced() {
        let mut mixer = MixerRenderer::new(1_000, true);
        mixer.set_mute(StemRole::Vocals, true);
        mixer.prime();
        mixer.reset();
        mixer.prime();
        let mut input = source([f32::NAN, 0.0, 0.0, 0.0], 1.0);
        input[9] = f32::INFINITY;
        let output = mixer.render(&input);
        assert_eq!(output, [1.0, 0.0]);
        assert_eq!(
            mixer.snapshot().output_preference,
            OutputPreference::Automatic
        );
        assert_eq!(mixer.snapshot().non_finite_samples, 3);
    }
}
