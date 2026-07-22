# Mixer Semantics v1

## Roles And State

The controllable roles are `drums`, `bass`, `other`, and `vocals`. Each stores immutable `gain_db`, `muted`, and `solo` values. An optional stereo master is a reference bus, not a fifth stem control.

Neutral is every gain at 0 dB with no mute or solo. Reset restores neutral controls and automatic output preference.

## Gain

Accepted gain is finite and within -60 through +6 dB inclusive. -60 dB maps to exact zero. 0 dB maps to exact one. Other values map to `10^(dB/20)`. Invalid values return `E_MIXER_GAIN_INVALID` and do not alter state.

Mute overrides solo. If any role is soloed, only soloed and unmuted roles are audible. Multiple roles may be soloed. Mute and solo never overwrite stored user gain.

## Output Selection

Preferences are automatic, master reference, or stem mix. Master preference without a master returns `E_MIXER_MASTER_UNAVAILABLE`.

- With master and a neutral mix, automatic selects master reference.
- Any gain, mute, or solo edit in automatic selects stem mix.
- Reset returns automatic and therefore selects master for a neutral source with master.
- Without master, effective output is always stem mix.
- Manual selection persists across ordinary transport commands until reset or a new load.

## Automation

Stem effective gains change with 10 ms linear ramps in amplitude. Master/stem switching uses complementary 20 ms linear crossfades: for normalized progress `t`, outgoing gain is `1 - t` and incoming gain is `t`. Automation begins from current instantaneous values and replaces conflicting scheduled automation deterministically.

Command-to-rendered-effect p95 budget is 30 ms. The UI may update immediately, but diagnostics distinguish command acceptance from first rendered frame.

## Numeric Contract

Four stereo stems sum in Float32 without normalization, limiter, compressor, residual correction, hidden compensation, or implicit clipping. Float32 source values outside `[-1, 1]` remain valid. Device-format clipping or quantization occurs only at final output conversion and is diagnosed separately.

The master and stem buses are mutually exclusive outside their crossfade. A/B never changes transport position, decoder generation, source buffers, or stored stem controls.

## Proof

Native conformance vectors cover every mute/solo combination, boundary gains, multiple solos, master absent/present, automatic/manual/reset behavior, 10 ms ramps, complementary 20 ms crossfades, rapid command replacement, and exact unity/silence endpoints.
