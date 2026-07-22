# Phase 5 — Sample-Synchronous Playback Engine and Transport

## Goal

Integrate the Studio WAV source path with the native output core to produce the first audible product vertical slice: synchronized four-stem playback with optional master, bounded buffering, exact transport state, frame-based seek, pause, stop, end detection, and deterministic cancellation.

## Entry criteria

- Phase 2 native shell and mock controller available.
- Phase 3 output core qualified.
- Phase 4 Studio WAV validation and bounded decode qualified.
- Phase 1 transport and command contracts stable.

## Work package 5.1 — Session construction

A successful Studio inspection produces a native playback session containing:

- Source identity and redacted display name.
- Sample rate.
- Start frame zero.
- Common exclusive end frame.
- Duration derived with rational arithmetic.
- Roles and master presence.
- Decoder/source handles.
- Initial generation identifier.
- Bounded buffer configuration.
- File and consistency diagnostics.

Session construction is transactional: failure leaves no partially active stream, worker, file handle, or cache entry.

## Work package 5.2 — Synchronized source ring

Implement one common frame domain with either:

- A single interleaved 8/10-channel ring, or
- A block ring where each item contains all routed channels for one common frame range.

Required properties:

- One read cursor and one write cursor.
- Blocks published only when all required tracks cover the same frame range.
- No independent per-stem playback clocks.
- Fixed capacity independent of duration.
- Explicit wraparound and generation metadata.
- Reset protocol safe against stale producers.
- No partial-frame publication.

Start with conservative watermarks based on existing evidence, then tune using native measurements. Any change to 1.5-second start, 3-second low, 6-second target, or 12-second maximum policy must be evidence-based and documented.

## Work package 5.3 — Decode scheduler

Create a bounded scheduler that:

- Decodes only requested frame ranges.
- Maintains the target watermark without decoding the whole track.
- Uses a small fixed worker count.
- Coordinates all track readers by absolute frame.
- Stops work when the ring is full.
- Prioritizes playback refill over waveform analysis.
- Cancels promptly when generation changes.
- Reports minimum/maximum buffered frames, pending blocks, bytes read, and worker queue depth.

No task queue may grow in proportion to track duration.

## Work package 5.4 — Generation and cancellation model

Every load, stop reset, and seek that invalidates pending PCM creates a new generation. All source blocks, decode requests, render operations, and ring state are tagged or guarded by that generation.

Prove:

- Stale work cannot write into the current ring.
- Stale completion events cannot move the transport state.
- Seek storms do not retain old buffers.
- Shutdown cancels all generations.
- Generation wraparound is either impossible in practice with a wide counter or explicitly handled.

## Work package 5.5 — Transport operations

### Load

- `EMPTY -> INSPECTING -> READY` or `ERROR`.
- No audio stream required before Play.

### Play

- `READY/ENDED -> PRIMING -> PLAYING`.
- Decode to start watermark.
- Start or resume the device stream only when sufficient data exists.
- Record time to first enabled output.

### Pause

- Stop consuming source progression without losing the current frame.
- Avoid repeated decode work while paused once bounded targets are satisfied.
- Preserve session and mixer state.

### Stop

- Cancel active generation.
- Reset to start frame.
- Return to a ready-equivalent loaded state according to the domain specification.
- Silence output without unloading.

### Seek

- Clamp to valid frame range.
- Silence or ramp output safely.
- Create a new generation.
- Reset rings and decoder range.
- Re-prime from target.
- Resume only if playback was desired before seek.
- Report actual target and latency.

### End

- Reach exact exclusive end frame.
- Drain only valid prepared output.
- Enter `ENDED` once.
- Never wrap or replay stale frames.

## Work package 5.6 — Source-to-output render bridge

For this phase, use a neutral fixed route sufficient to prove transport:

- If master exists, use master reference in neutral state.
- Otherwise sum stems at exact unity.

Full user mixer automation arrives in Phase 6. The bridge must already preserve all channels in the source ring so Phase 6 does not require an engine redesign.

## Work package 5.7 — Position snapshots

Publish bounded snapshots containing:

- Transport state.
- Authoritative source frame.
- Duration and sample rate.
- Buffered source frames.
- Device queue estimate.
- Audio stream state.
- Underrun/overflow counters.
- Generation-safe error state.

Snapshot publication is coalesced. The UI interpolates between snapshots and does not receive per-callback messages.

## Work package 5.8 — Underrun policy

On source or output underrun:

- Device callback writes silence.
- Counters increment.
- Engine enters buffering or recoverable error according to duration and policy.
- Resume requires sufficient source data.
- Repeated underruns may increase adaptive buffering within a hard cap.
- No decoder work occurs in the callback.

Normal local SSD playback must produce zero underruns in qualification.

## Work package 5.9 — Integration with native shell

Replace the mock controller behind an internal feature with the real Studio controller:

- Open supported bundle.
- Show inspecting/ready/buffering/error.
- Play, pause, stop, seek.
- Update time and buffer diagnostics.
- Close safely.

Mixer controls may be visible but disabled or limited until Phase 6.

## Deliverables

- Transactional playback sessions.
- Synchronized 8/10-channel source ring.
- Bounded decode scheduler.
- Generation-safe cancellation.
- Complete transport implementation for Studio WAV.
- Position and buffering snapshots.
- Underrun recovery policy.
- First audible native application vertical slice.

## Tests and evidence

- Exact first and last frame.
- 30-minute synthetic no-drift test at 44.1 and 48 kHz.
- Complete-track real-time playback.
- Pause/resume at randomized positions.
- 500 rapid randomized seeks.
- Seek while priming, paused, and near end.
- Stop during load, prime, playback, and seek.
- New load while old generation is active.
- Ring wraparound and capacity tests.
- No gaps, overlaps, or inserted silence in source assembly.
- Controlled underrun recovery.
- UI shutdown during every transport state.

## Resource checks

- Source-ring memory fixed and reported.
- Peak pending PCM fixed by configuration.
- Decode queue bounded.
- No full-duration PCM retention.
- Playback CPU and memory measured against WAV budgets.
- Paused state reaches a stable low-wakeup condition.

## Risks and mitigations

- **Independent stem workers drift:** publish only common synchronized blocks.
- **Seek races corrupt state:** generation-tag all work and state transitions.
- **Large ring hides defects and wastes memory:** preserve diagnostics and tune from evidence.
- **UI polling becomes expensive:** use coalesced snapshots and local interpolation.

## Exit gate

Phase 5 is complete when Studio WAV bundles play audibly and sample-synchronously from start to exact end, transport and seek behave deterministically, memory is duration-independent, normal-load playback has zero underruns, and stale work cannot affect the active session.
