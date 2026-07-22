# Phase 3 — Native Audio Device and Real-Time Output Core

## Goal

Implement and qualify the platform audio-output layer independently of file formats and the product mixer. The result is a bounded, real-time-safe path from prepared stereo frames to the default or selected audio device.

## Entry criteria

- Phase 1 domain and error contracts are available.
- Phase 0 audio backend decision passed.
- Reference hardware and output devices are identified.
- A headless probe can report machine-readable results.

Phase 3 can proceed in parallel with Phases 2 and 4 after Phase 1.

## Work package 3.1 — Output-device discovery

Implement:

- Enumerate output devices.
- Identify the current default output device.
- Produce stable session-local device identifiers without persisting sensitive hardware details unnecessarily.
- Query supported sample rates, channel counts, buffer ranges, and sample formats.
- Select stereo output configurations by documented preference.
- Reject devices that cannot provide a safe stereo route.

Device names are user-visible but must not be included in crash reports or telemetry because no telemetry exists.

## Work package 3.2 — Configuration policy

Define deterministic negotiation:

1. Prefer the selected device; otherwise use the current default.
2. Prefer a device sample rate matching the source when available.
3. Otherwise prefer the platform’s normal/default rate.
4. Prefer Float32 output, then common integer formats.
5. Use a conservative default buffer size initially.
6. Expose an advanced latency setting only after qualification data exists.

The policy must not silently switch devices during ordinary playback unless the active device disappears or the user changes it.

## Work package 3.3 — SPSC output ring

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

## Work package 3.4 — Device callback

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

## Work package 3.5 — Render worker

Implement a render worker that fills the output ring from a synthetic source:

- Sine wave and impulse modes.
- Configurable source frame count.
- Start, pause, resume, stop.
- Bounded wait strategy when the ring is full.
- Immediate cancellation on generation change.
- Accurate produced and consumed device-frame counters.

The worker is the future location for post-mix resampling, not the callback.

## Work package 3.6 — Audio clock

Define authoritative output timing:

- Distinguish source frames, rendered device frames, queued frames, and consumed frames.
- Estimate audible position from consumed frames plus platform latency information where reliable.
- Never use wall-clock accumulation as playback truth.
- Handle callback chunk-size variation.
- Reset clock atomically on seek or stream recreation.

Record uncertainty when a backend cannot expose reliable device latency rather than inventing precision.

## Work package 3.7 — Device loss and lifecycle

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

## Work package 3.8 — Platform backend policy

### Windows

- Qualify WASAPI shared mode first.
- Document exclusive mode as out of scope unless a later requirement justifies it.
- Test common Bluetooth and USB devices.

### macOS

- Keep CoreAudio support behind the common backend contract.
- Native default/selected-device, aggregate-device, and channel-layout qualification is optional Phase 12 platform-expansion work.

### Linux

- Keep PipeWire/PulseAudio/ALSA support behind the common backend contract.
- Detect and explain “device busy” conditions.
- Native hot-plug and server-restart qualification is optional Phase 12 platform-expansion work.

## Work package 3.9 — Probe commands

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
