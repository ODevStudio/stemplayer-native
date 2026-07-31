# Phase 3 â€” Native Audio Device and Real-Time Output Core

## Progress

- Status: exit gate passed (2026-07-22).
- Phase 0 audio decisions, Phase 1 contracts, and the Phase 2 probe handoff were reviewed before implementation.
- Scope remains limited to `native player`; formats, mixer behavior, the webplayer, and the converter remain untouched.

## Goal

Implement and qualify the platform audio-output layer independently of file formats and the product mixer. The result is a bounded, real-time-safe path from prepared stereo frames to the default or selected audio device.

## Entry criteria

- Phase 1 domain and error contracts are available.
- Phase 0 audio backend decision passed.
- Reference hardware and output devices are identified.
- A headless probe can report machine-readable results.

Phase 3 can proceed in parallel with Phases 2 and 4 after Phase 1.

## Work package 3.1 â€” Output-device discovery

Implement:

- Enumerate output devices.
- Identify the current default output device.
- Produce stable session-local device identifiers without persisting sensitive hardware details unnecessarily.
- Query supported sample rates, channel counts, buffer ranges, and sample formats.
- Select stereo output configurations by documented preference.
- Reject devices that cannot provide a safe stereo route.

Device names are user-visible but must not be included in crash reports or telemetry because no telemetry exists.

## Work package 3.2 â€” Configuration policy

Define deterministic negotiation:

1. Prefer the selected device; otherwise use the current default.
2. Prefer a device sample rate matching the source when available.
3. Otherwise prefer the platformâ€™s normal/default rate.
4. Prefer Float32 output, then common integer formats.
5. Use a conservative default buffer size initially.
6. Expose an advanced latency setting only after qualification data exists.

The policy must not silently switch devices during ordinary playback unless the active device disappears or the user changes it.

## Work package 3.3 â€” SPSC output ring

Create a fixed-capacity stereo ring between the render thread and device callback:

- Preallocated storage.
- Single producer and single consumer.
- Monotonic frame counters.
- Explicit underflow and overflow counters.
- No overwrite of unread frames.
- Reset protocol with generation protection.
- Capacity expressed in frames, not bytes.
- Tests for wraparound near integer boundaries using reduced counter widths in simulation.

The ring stores the final device-rate stereo representation or a canonical Float32 stereo representation followed by callback-local format conversion.

## Work package 3.4 â€” Device callback

The callback may perform only:

- Pop available stereo frames.
- Convert Float32 to the negotiated device sample format with explicit saturation rules required by integer output.
- Duplicate or map stereo channels only when the output policy explicitly permits it.
- Write silence for missing frames.
- Update lock-free counters.

Prohibited:

- Heap allocation.
- Normal mutex or condition-variable locking.
- File, network, or console I/O.
- Logging formatting.
- Decoder calls.
- Resampler calls that can allocate or perform unbounded work.
- UI notifications.

Add a test-only allocator hook or equivalent instrumentation to prove zero callback allocations after stream start.

## Work package 3.5 â€” Render worker

Implement a render worker that fills the output ring from a synthetic source:

- Sine wave and impulse modes.
- Configurable source frame count.
- Start, pause, resume, stop.
- Bounded wait strategy when the ring is full.
- Immediate cancellation on generation change.
- Accurate produced and consumed device-frame counters.

The worker is the future location for post-mix resampling, not the callback.

## Work package 3.6 â€” Audio clock

Define authoritative output timing:

- Distinguish source frames, rendered device frames, queued frames, and consumed frames.
- Estimate audible position from consumed frames plus platform latency information where reliable.
- Never use wall-clock accumulation as playback truth.
- Handle callback chunk-size variation.
- Reset clock atomically on seek or stream recreation.

Record uncertainty when a backend cannot expose reliable device latency rather than inventing precision.

## Work package 3.7 â€” Device loss and lifecycle

Implement:

- Start stream on Play, not application launch, unless platform constraints require otherwise.
- Pause behavior that avoids unnecessary CPU while retaining a fast resume path.
- Clean stream teardown.
- Default-device change detection.
- Active-device disconnect handling.
- Reopen policy with bounded retries outside the callback.
- User-visible recoverable errors.
- Suspend/resume hooks in cooperation with the platform crate.

Do not automatically resume audible playback after a disruptive device switch unless product policy explicitly approves it.

## Work package 3.8 â€” Platform backend policy

### Windows

- Qualify WASAPI shared mode first.
- Document exclusive mode as out of scope unless a later requirement justifies it.
- Test common Bluetooth and USB devices.

### macOS

- Keep CoreAudio support behind the common backend contract.
- Native default/selected-device, aggregate-device, and channel-layout qualification is optional Phase 12 platform-expansion work.

### Linux

- Keep PipeWire/PulseAudio/ALSA support behind the common backend contract.
- Detect and explain â€œdevice busyâ€ conditions.
- Native hot-plug and server-restart qualification is optional Phase 12 platform-expansion work.

## Work package 3.9 â€” Probe commands

Implement:

```text
stemplayer-probe devices
stemplayer-probe output-test --device <id> --rate <hz> --seconds <n>
stemplayer-probe callback-audit
stemplayer-probe device-reopen-test
```

Reports include negotiated configuration, callback sizes, underruns, overflows, callback maximum duration, output-ring occupancy, and shutdown time.

## Deliverables

- Native output crate.
- Device enumeration and selection policy.
- Preallocated SPSC output ring.
- Real-time-safe callback.
- Synthetic render worker.
- Audio clock model.
- Device-loss recovery state machine.
- Platform qualification report.

## Tests and evidence

- Ring property tests and wraparound tests.
- Allocation-free callback evidence.
- Callback deadline distribution under normal and stressed load.
- Pause/resume/stop cycles.
- 10,000 stream open/close operations in a fake backend and a practical hardware loop count.
- Device disconnect and default-device change tests.
- Suspend/resume tests.
- One-hour Windows synthetic output soak with zero normal-load underruns. Native macOS/Linux soaks are optional Phase 12 platform-expansion evidence.

## Resource checks

- Output ring memory is fixed and documented.
- Render worker sleeps efficiently when paused or full.
- No stream exists while the application has never played, unless required and documented.
- CPU and wakeups are measured for playing, paused, stopped, hidden, and device-lost states.

## Risks and mitigations

- **Backend callback behavior differs by platform:** isolate backend configuration and keep a common callback contract.
- **Device format requires costly conversion:** perform simple bounded conversion only and prefer Float32.
- **Linux server conflict:** implement explicit backend order and diagnostic guidance.
- **Audio clock overclaims latency accuracy:** expose confidence and raw counters.

## Exit gate

Phase 3 is complete when Windows can play synthetic stereo audio through the selected WASAPI device with a proven allocation-free callback, bounded rings, stable lifecycle, recoverable device loss, and reproducible performance evidence. macOS and Linux remain unsupported until their optional Phase 12 native audio qualification passes.

## Phase 3 execution record - 2026-07-22

Status: exit gate passed.

### Implemented checkpoint

- Reused the Phase 0 decisions for CPAL 0.18.1 and rtrb 0.3.4; no additional dependency was introduced.
- Added session-local output-device enumeration, default-device identification, capability reporting, stereo rejection, and deterministic Float32-first configuration negotiation.
- Added a fixed-capacity stereo SPSC ring with generation-tagged reset, no overwrite, monotonic counters, occupancy tracking, explicit underflow/overflow counters, and reduced-width wraparound simulation.
- Added callback-local Float32, Float64, signed-integer, and unsigned-integer conversion with explicit saturation and silence behavior.
- Added the synthetic render worker, prefill, bounded full-ring sleep, immediate stop signal, callback-size/deadline counters, output clock model, and bounded device-loss lifecycle state.
- Added `devices`, `output-test`, `callback-audit`, `device-reopen-test`, and `device-recovery-test` to `stemplayer-probe`.
- Added a dedicated allocation-audit binary, practical WASAPI lifecycle qualification, CPU stress, memory sampling, context-switch sampling, and retained machine-readable artifacts.

### Evidence at this checkpoint

| Check | Command | Result |
| --- | --- | --- |
| Focused tests | `cargo test --offline -p stemplayer-output -p stemplayer-probe` | PASS, 24 tests |
| Workspace format | `cargo fmt --all -- --check` | PASS |
| Workspace lint | `cargo clippy --offline --locked --all-targets --all-features -- -D warnings` | PASS |
| Workspace tests | `cargo test --offline --locked --workspace --all-features` | PASS, 61 tests |
| Dependency direction | `cargo tree --offline --locked -p stemplayer-output` | PASS, only CPAL, rtrb, serde, and domain at the direct boundary |
| Forbidden coupling | exact output/probe scan | PASS, zero webplayer, Node.js, Python, FFmpeg, WebView, Electron, Tauri, AudioWorklet, or mixer references |
| Callback kernel | `stemplayer-probe callback-audit` | PASS, 10,000 callbacks, 256 frames each, 0 underflows, 0 overflows, maximum observed callback 75,700 ns |
| Fake lifecycle | output lifecycle tests | PASS, 10,000 open/close cycles, 0 failures |
| Allocation proof | `stemplayer-callback-audit` | PASS, 100,000 callbacks, 256 frames each, 0 allocations |
| Hardware reopen | `stemplayer-probe device-reopen-test` | PASS, 10 WASAPI open/play/close cycles, 0 failures, underruns, overflows, or stream errors |
| Pause/resume | `stemplayer-probe output-test --seconds 5 --pause-cycles 20` | PASS, 20 WASAPI pause/resume cycles, 0 underruns, overflows, or stream errors |
| CPU stress | `scripts/qualify-phase3-windows.ps1` | PASS, 15 CPU workers, 0 underruns, overflows, or stream errors |
| One-hour soak | `scripts/qualify-phase3-windows.ps1 -SoakSeconds 3600` | PASS, 0 underruns, overflows, or stream errors; maximum callback 423,000 ns; private memory change -69,632 bytes |
| Wakeup proxy | `scripts/measure-phase3-wakeups.ps1 -Seconds 10 -DeviceId device-2` | PASS, 5,780 context switches, 722.5 per second, 0 underruns |
| Windows discovery | `stemplayer-probe devices` | PASS, five stereo-capable devices, one current default |
| WASAPI smoke | `stemplayer-probe output-test --device device-2 --rate 48000 --seconds 2` | PASS, Float32 stereo, 201 callbacks, 0 underflows, 0 overflows, 0 stream errors, maximum observed callback 62,500 ns, 3 ms shutdown |
| Active-device unplug recovery | `stemplayer-probe device-recovery-test --seconds 30` | PASS, iFi USB stream reported 1 error; Realtek recovery succeeded in 1 attempt with 0 stream errors; `fault_observed: true`; `recovered_paused: true` |
| Dependency audit | `scripts/audit-dependencies.ps1 -OutputDirectory artifacts/phase3/dependencies` | PASS, 616 dependencies, 0 vulnerabilities, 0 unlicensed crates |
| Qualification artifact | `artifacts/phase3/windows/qualification.json` | PASS |

### Exit-gate result

- Passed. The required Windows playback, allocation-free callback, bounded ring, stable lifecycle, recoverable device loss, and reproducible performance evidence are retained above. Suspend/resume is not part of this phase's exit gate and was not tested.

### Limitations

- Administrator policy blocked WPR CPU/audio ETW collection with error `0xc5585011`; process-thread context switches are retained as the available wakeup proxy.
