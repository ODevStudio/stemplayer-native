#![deny(unsafe_code)]

mod render;

use serde::Serialize;
use stemplayer_domain::{ErrorCode, GainDb, OutputMode, OutputPreference, PlayerError, StemRole};

pub use render::MixerRenderer;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct StemControl {
    pub gain_db: GainDb,
    pub muted: bool,
    pub solo: bool,
}

impl Default for StemControl {
    fn default() -> Self {
        Self {
            gain_db: GainDb::UNITY,
            muted: false,
            solo: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MixerState {
    controls: [StemControl; 4],
    preference: OutputPreference,
}

impl Default for MixerState {
    fn default() -> Self {
        Self {
            controls: [StemControl::default(); 4],
            preference: OutputPreference::Automatic,
        }
    }
}

impl MixerState {
    pub fn control(self, role: StemRole) -> StemControl {
        self.controls[role.index()]
    }

    pub const fn preference(self) -> OutputPreference {
        self.preference
    }

    pub fn with_gain(mut self, role: StemRole, gain_db: GainDb) -> Self {
        self.controls[role.index()].gain_db = gain_db;
        self
    }

    pub fn with_mute(mut self, role: StemRole, muted: bool) -> Self {
        self.controls[role.index()].muted = muted;
        self
    }

    pub fn with_solo(mut self, role: StemRole, solo: bool) -> Self {
        self.controls[role.index()].solo = solo;
        self
    }

    pub fn with_preference(
        mut self,
        preference: OutputPreference,
        has_master: bool,
    ) -> Result<Self, PlayerError> {
        if preference == OutputPreference::MasterReference && !has_master {
            return Err(master_unavailable());
        }
        self.preference = preference;
        Ok(self)
    }

    pub fn reset(self) -> Self {
        Self::default()
    }

    pub fn is_neutral(self) -> bool {
        self.controls
            .iter()
            .all(|control| *control == StemControl::default())
    }

    pub fn coherent_gain(self) -> Option<GainDb> {
        let gain = self.controls[0].gain_db;
        self.controls
            .iter()
            .all(|control| !control.muted && !control.solo && control.gain_db == gain)
            .then_some(gain)
    }

    pub fn effective_output(self, has_master: bool) -> Result<OutputMode, PlayerError> {
        match self.preference {
            OutputPreference::Automatic if has_master && self.coherent_gain().is_some() => {
                Ok(OutputMode::MasterReference)
            }
            OutputPreference::Automatic | OutputPreference::StemMix => Ok(OutputMode::StemMix),
            OutputPreference::MasterReference if has_master => Ok(OutputMode::MasterReference),
            OutputPreference::MasterReference => Err(master_unavailable()),
        }
    }

    pub fn effective_linear_gains(self) -> [f32; 4] {
        let any_solo = self.controls.iter().any(|control| control.solo);
        self.controls.map(|control| {
            let audible = !control.muted && (!any_solo || control.solo);
            if audible {
                control.gain_db.linear()
            } else {
                0.0
            }
        })
    }
}

fn master_unavailable() -> PlayerError {
    PlayerError::new(
        ErrorCode::MixerMasterUnavailable,
        "Master reference is not available for this source",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_mute_and_solo_combinations_follow_the_contract() {
        for muted_mask in 0_u8..16 {
            for solo_mask in 0_u8..16 {
                let mut state = MixerState::default();
                for role in StemRole::ALL {
                    let bit = 1 << role.index();
                    state = state
                        .with_mute(role, muted_mask & bit != 0)
                        .with_solo(role, solo_mask & bit != 0);
                }
                let gains = state.effective_linear_gains();
                for role in StemRole::ALL {
                    let bit = 1 << role.index();
                    let muted = muted_mask & bit != 0;
                    let solo = solo_mask & bit != 0;
                    let expected = if muted || (solo_mask != 0 && !solo) {
                        0.0
                    } else {
                        1.0
                    };
                    assert_eq!(gains[role.index()], expected);
                }
            }
        }
    }

    #[test]
    fn automatic_output_tracks_neutrality_and_master_presence() {
        let neutral = MixerState::default();
        assert_eq!(
            neutral.effective_output(true).unwrap(),
            OutputMode::MasterReference
        );
        assert_eq!(
            neutral.effective_output(false).unwrap(),
            OutputMode::StemMix
        );
        let modified = neutral.with_gain(StemRole::Vocals, GainDb::new(-3.0).unwrap());
        assert_eq!(
            modified.effective_output(true).unwrap(),
            OutputMode::StemMix
        );
        let coherent = StemRole::ALL.into_iter().fold(neutral, |state, role| {
            state.with_gain(role, GainDb::new(-3.0).unwrap())
        });
        assert_eq!(
            coherent.effective_output(true).unwrap(),
            OutputMode::MasterReference
        );
        assert_eq!(
            modified.reset().effective_output(true).unwrap(),
            OutputMode::MasterReference
        );
    }

    #[test]
    fn output_preferences_validate_master_availability() {
        let state = MixerState::default();
        assert!(
            state
                .with_preference(OutputPreference::MasterReference, false)
                .is_err()
        );
        let explicit = state
            .with_preference(OutputPreference::MasterReference, true)
            .unwrap();
        assert_eq!(
            explicit.effective_output(true).unwrap(),
            OutputMode::MasterReference
        );
        assert_eq!(
            state
                .with_preference(OutputPreference::StemMix, true)
                .unwrap()
                .effective_output(true)
                .unwrap(),
            OutputMode::StemMix
        );
    }

    #[test]
    fn gain_endpoints_are_exact_and_state_is_immutable() {
        let original = MixerState::default();
        let silent = original.with_gain(StemRole::Drums, GainDb::SILENCE);
        assert_eq!(silent.effective_linear_gains()[0], 0.0);
        assert_eq!(original.effective_linear_gains()[0], 1.0);
        let maximum = original.with_gain(StemRole::Bass, GainDb::new(GainDb::MAX).unwrap());
        assert!(maximum.effective_linear_gains()[1] > 1.0);
    }
    #[test]
    fn output_selection_covers_all_controls_preferences_and_master_states() {
        for muted_mask in 0_u8..16 {
            for solo_mask in 0_u8..16 {
                for gain in [
                    GainDb::SILENCE,
                    GainDb::UNITY,
                    GainDb::new(GainDb::MAX).unwrap(),
                ] {
                    let mut state = MixerState::default().with_gain(StemRole::Drums, gain);
                    for role in StemRole::ALL {
                        let bit = 1 << role.index();
                        state = state
                            .with_mute(role, muted_mask & bit != 0)
                            .with_solo(role, solo_mask & bit != 0);
                    }
                    for has_master in [false, true] {
                        for preference in [
                            OutputPreference::Automatic,
                            OutputPreference::MasterReference,
                            OutputPreference::StemMix,
                        ] {
                            let selected = state.with_preference(preference, has_master);
                            if preference == OutputPreference::MasterReference && !has_master {
                                assert_eq!(
                                    selected.unwrap_err().code,
                                    ErrorCode::MixerMasterUnavailable
                                );
                                continue;
                            }
                            let selected = selected.unwrap();
                            let expected = match preference {
                                OutputPreference::Automatic
                                    if has_master && state.coherent_gain().is_some() =>
                                {
                                    OutputMode::MasterReference
                                }
                                OutputPreference::MasterReference => OutputMode::MasterReference,
                                OutputPreference::Automatic | OutputPreference::StemMix => {
                                    OutputMode::StemMix
                                }
                            };
                            assert_eq!(selected.effective_output(has_master).unwrap(), expected);
                        }
                    }
                }
            }
        }
    }
}
