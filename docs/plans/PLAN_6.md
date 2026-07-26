# Phase 6 — Stem Mixer, Master Reference, and Output Resampling

## Goal

Implement the complete native mixing behavior and make playback robust across source and device sample-rate differences. Preserve exact stem semantics while minimizing CPU by resampling only the final stereo bus.

## Entry criteria

- Phase 5 provides stable synchronized source blocks and transport.
- Phase 3 output clock and ring are qualified.
- Phase 1 pure mixer state contract is complete.
- Mixer behavior specification is approved.

## Work package 6.1 — Render graph

Implement a fixed render graph:

```text
8 stem channels ──> four stereo stem gains ──> stem sum ─┐
                                                        ├─> complementary A/B ─> stereo output
optional master stereo ─────────────────────────────────┘
```

The graph is data-oriented and preallocated. It must handle source blocks without master without branching into a separate timing model.

## Work package 6.2 — Gain and mute/solo application

For each role:

- Convert validated dB to linear gain.
- Treat -60 dB as exact zero.
- Treat 0 dB as exact one.
- Apply effective gain after mute/solo derivation.
- Preserve values above unity through +6 dB.
- Do not clamp Float32 stem samples.
- Use a 10 ms linear ramp from the currently rendered gain to the new target.
- Replace or retarget an active ramp without discontinuity.

Commands update control state outside the callback. The render thread consumes compact automation events from a bounded queue or atomic mailbox.

## Work package 6.3 — Master/stem A/B

Implement:

- Automatic mode.
- Explicit master-reference mode when master exists.
- Explicit stem-mix mode.
- Automatic neutral master selection.
- Automatic stem selection after any stem control change.
- Reset to neutral and automatic mode.
- Complementary 20 ms linear crossfade.
- Retargeting during an active crossfade without clicks or gain discontinuity.

A/B changes must not seek, reset decoders, reset source rings, or move the source cursor.

## Work package 6.4 — Summation and numeric behavior

- Accumulate stems in Float32 or Float64 according to measured quality/performance; record the decision.
- Preserve deterministic channel order.
- Do not normalize by stem count.
- Do not limit or compress.
- Report optional peak diagnostics without changing audio.
- Define NaN/Infinity source behavior before render; never allow non-finite values to reach an integer device conversion silently.

A diagnostic residual between master and stem sum may be measured for Studio bundles, but no correction is applied.

## Work package 6.5 — Post-mix resampling

Add one stereo resampler between the A/B output and device ring:

- Source rate remains the session rate.
- Device rate is negotiated independently.
- Resampler is initialized outside the callback.
- Input/output buffers are preallocated.
- Render in bounded chunks.
- Track fractional phase and delay explicitly.
- Reset or prime deterministically on seek and device recreation.
- Account for resampler latency in the audible-position estimate.
- Preserve left/right synchronization.

Do not create one resampler per stem or per track.

## Work package 6.6 — Device-rate changes

When the active output device changes:

- Pause or enter controlled buffering.
- Create a new output stream and resampler state.
- Keep the source frame as the authoritative position.
- Re-prime enough output to prevent an immediate underrun.
- Resume according to user intent and product policy.
- Record the discontinuity and latency in diagnostics.

Do not rewrite the loaded source or alter stem alignment.

## Work package 6.7 — Automation timing

Define automation time in source frames, not UI clock milliseconds. Convert 10 ms and 20 ms durations to frame counts with deterministic rounding. For resampled output, ensure the audible automation duration remains within tolerance.

Tests cover commands issued:

- Exactly at block boundaries.
- Mid-block.
- During active ramps.
- During seek/prime.
- Immediately before end.
- In rapid alternating sequences.

## Work package 6.8 — Mixer diagnostics

Expose bounded values:

- Automation count.
- Crossfade count.
- Current and target gains.
- Remaining ramp frames.
- Effective output mode.
- Last automation reason.
- Optional source and output peaks.
- Resampler input/output rates, delay, and buffer occupancy.

Diagnostic reads must not lock or disturb the render path.

## Work package 6.9 — UI enablement

Enable full native controls:

- Gain faders with fine control near zero.
- Mute and solo.
- Multiple solos.
- Master/stem output preference where master exists.
- Reset.
- Clear display of automatic versus manual output mode.
- Keyboard adjustment with accessible value announcements.

UI values are control state; effective values and mode come from engine snapshots.

## Deliverables

- Native four-stem render graph.
- Click-free 10 ms gain ramps.
- Complementary 20 ms A/B crossfades.
- Exact mixer state behavior.
- One stereo post-mix resampler.
- Device-rate-change handling.
- Mixer and resampler diagnostics.
- Fully enabled mixer UI.

## Tests and evidence

- Golden state tests matching the approved mixer specification.
- Impulse tests for every role and channel.
- Mute-over-solo and multi-solo tests.
- Gain boundary tests at -60, 0, and +6 dB.
- Ramp sample-shape tests.
- Rapid automation stress tests.
- Master/stem crossfade complementarity tests.
- Proof that A/B never resets transport or source ring.
- 44.1-to-48, 48-to-44.1, and 48-to-96 kHz resampling tests.
- Long resampling drift test with exact source end.
- Listening qualification for clicks, channel swaps, and discontinuities.

## Resource checks

- Mixer and resampler CPU against budgets.
- No render-path allocations after initialization.
- Automation queue bounded under UI floods.
- Resampler memory fixed by maximum chunk and quality setting.
- Device callback remains unchanged and allocation-free.

## Risks and mitigations

- **Resampler adds latency:** include delay in position model and tune quality outside the callback.
- **Rapid automation clicks:** retarget from current rendered value, not old command state.
- **High positive gains overflow integer output:** define device conversion saturation while leaving internal Float32 mix unchanged.
- **Master/stem switch changes loudness by design:** do not normalize; communicate mode clearly.

## Exit gate

Phase 6 is complete when native playback reproduces all approved mixer semantics, A/B and gain changes are click-free and transport-neutral, arbitrary qualified device rates work through one post-mix stereo resampler, and render-path resource and timing budgets are met.

## Implementation record

### 2026-07-24 - Implementation resumed

Status: exit gate passed.

Implemented:

- Added the fixed four-stem Float32 graph, exact gain endpoints, mute-over-solo semantics, 10 ms retargetable source-frame ramps, optional master reference, and complementary 20 ms A/B crossfades.
- Kept A/B and gain automation transport-neutral. Seek, stop, replay, and device recreation prime the current mixer and resampler state deterministically.
- Added one preallocated stereo `rubato` FFT resampler after the A/B bus. The source rate remains authoritative while the output device negotiates independently.
- Added exact resampled end-frame accounting, delay and fractional-phase diagnostics, bounded buffers, non-finite sample rejection, peak diagnostics, and device-change discontinuity tracking.
- Enabled native gain, mute, multi-solo, reset, and automatic/master/stem preference controls. Slider floods are coalesced before the bounded controller queue.
- Recorded the resampler, dependency, and Float32 summation decision in `stemplayer-native/docs/decisions/phase6-post-mix-resampler.md`.

Verification:

| Check | Evidence | Result |
| --- | --- | --- |
| Workspace tests | `rtk cargo test --workspace` | PASS, 104 passed and 2 ignored across 30 suites |
| Lints and format | `rtk cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check` | PASS |
| Release build | `rtk cargo build --release --locked -p stemplayer -p stemplayer-probe -p stemplayer-callback-audit` | PASS |
| Mixer semantics | Golden state, every-role/channel impulse, mute/multi-solo, exact ramp, active retarget, complementary A/B, equal-bus unity, rapid automation, seek-prime, and last-frame tests | PASS |
| Resampling | 44.1 to 48, 48 to 44.1, and 48 to 96 kHz; channel isolation; deterministic reset; one-hour frame math; exact 10/20 ms scaling | PASS |
| Windows hardware | `scripts/qualify-phase6-windows.ps1`; 48 kHz source to 44.1 kHz iFi output | PASS, 1,440,000 source frames to exactly 1,323,000 device frames, 147-frame reported delay, zero underruns and overflows |
| Active device-rate change | `rtk cargo test --release -p stemplayer active_device_rate_change_recreates_and_reprimes_output -- --ignored --nocapture` | PASS, switched to a non-default device with a different rate, re-primed once, recorded one discontinuity, and retained zero underruns and overflows |
| Resource budgets | `artifacts/phase6/windows/resource-report.json` | PASS, 4.677% of one core p95 and 27,099,136 peak private bytes |
| Render allocation and latency | `stemplayer-callback-audit.exe` | PASS, zero allocations in 100,000 callbacks and 1,000 mixer/resampler blocks; command-to-render p95 100 ns against 30 ms |
| Dependency audit | `artifacts/phase6/dependencies/audit.json` | PASS, 0 vulnerabilities and 0 unlicensed crates in the 624-package locked graph |
| Ubuntu headless | `artifacts/phase6/wsl-ubuntu/headless.json` | PASS, locked tests and metadata on Ubuntu 24.04 WSL2 |

### 2026-07-24 - Native UI interaction corrections

Corrected:

- Stop now presents the authoritative zero-frame snapshot immediately instead of waiting for the next play tick.
- Automatic, master reference, and stem mix are one exclusive radio group; diagnostics remains an independent checkbox.
- Gain previews flush at a bounded 16 ms cadence while dragging and flush once more on release.
- Reset updates the visible controls immediately, flushes any pending gain command, then submits the reset in queue order.
- The output selector now enumerates native devices and submits the selected device identifier.

Verification:

| Check | Evidence | Result |
| --- | --- | --- |
| Workspace regression | `cargo test --workspace` | PASS, 104 passed and 2 ignored |
| Lints, format, and diff | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `git diff --check` | PASS |
| Release build and smoke | `rtk cargo build --release --locked -p stemplayer -p stemplayer-probe -p stemplayer-callback-audit`; `stemplayer.exe --ui-smoke 1000` | PASS |
| Active device-rate change | `rtk cargo test --release -p stemplayer active_device_rate_change_recreates_and_reprimes_output -- --ignored --nocapture` | PASS |
| Native UI regression | Stop playhead, exclusive modes, reset state, and output selector exercised in the Windows UI | PASS; operator confirmed output-device selection works |

Listening qualification:

- Operator confirmed click-free playback after exercising the native mixer controls and live output-device selection.
- No channel-order, transport, or device-change artifacts were reported.

The automated and listening gates are green. Phase 6 passed its exit gate; Phase 7 is active and not started.
