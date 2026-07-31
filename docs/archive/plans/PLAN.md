# Stemplayer Native Rust Plan

## 1. Purpose

This plan defines a new, fully native, cross-platform stem player written in Rust for Windows, Linux, and macOS. It is a standalone application. It may use the existing `ODevStudio/stemplayer` webplayer as a read-only behavioral and format reference, but it must not modify, wrap, embed, import, patch, or become a build dependency of that webplayer.

The native player is also independent from the stem converter. It consumes supported files and URLs; it does not run separation, model installation, Python, PyTorch, converter export jobs, or converter-specific IPC.

The deliverable is a stable desktop player with the capabilities users expect from the webplayerÃ¢â‚¬â€sample-synchronous stem playback, four-stem mixing, optional master reference, frame-accurate transport, waveform navigation, local container playback, Studio WAV bundle playback, and eventually `stem-hls-v1` playbackÃ¢â‚¬â€implemented with a native Rust UI and a native Rust audio engine.

## 2. Non-negotiable project boundary

The following constraints apply to every phase:

1. **The webplayer remains unchanged.** Implementation work occurs in a separate repository or a completely independent native workspace. No existing webplayer source file, test, package manifest, Vite configuration, build output, or runtime contract is changed to make the native player work.
2. **No web technology in the native runtime.** No browser engine, WebView, Electron, Tauri, Web Audio, AudioWorklet, WebCodecs, JavaScript runtime, HTML, or CSS is required by the native player.
3. **No converter runtime dependency.** The player does not launch or bundle Python, PyTorch, model management, separation, or transactional export code.
4. **Reference, do not couple.** The webplayer may be inspected to extract expected behavior, schemas, error conditions, timing semantics, fixtures, and qualification cases. Those expectations are re-expressed in native documentation and native tests.
5. **Standalone installation.** A user can install and run the native player without installing Node.js, pnpm, Python, FFmpeg, the converter, or the webplayer.
6. **Independent release lifecycle.** The native player has its own versioning, CI, artifacts, signing, release notes, and rollback process.

If the native project is temporarily hosted in the same Git repository, it must still be an isolated Cargo workspace with independent CI paths and no changes to the webplayerÃ¢â‚¬â„¢s source or build files. A separate repository is preferred because it makes the boundary mechanically enforceable.

## 3. Definition of Ã¢â‚¬Å“fully nativeÃ¢â‚¬Â

The target implementation is:

- Rust application and engine.
- Native window, event loop, rendering, dialogs, drag-and-drop, and accessibility.
- Native audio devices through CPAL or an equivalent low-level Rust audio abstraction.
- Native demuxing, decoding, buffering, mixing, waveform analysis, caching, and networking.
- No hidden browser process.
- Event-driven UI rendering with no permanent animation loop while idle.

The plan is written with **Slint** as the default UI toolkit because it provides a native desktop UI model and separates UI presentation from the Rust engine. Phase 0 contains a Windows licensing, accessibility, renderer, binary-size, idle-memory, and Ubuntu WSL2 compilation gate. Native macOS and Linux desktop qualification is an optional Phase 12 platform expansion. If Slint fails a shipped-platform gate, the UI layer may be replaced by another native Rust toolkit without changing the engine, domain, format, or output crates.

## 4. Target architecture

```text
Native UI
  Ã¢â€â€Ã¢â€â‚¬Ã¢â€â‚¬ application/controller layer
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ commands
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ immutable snapshots
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ settings
        Ã¢â€â€Ã¢â€â‚¬Ã¢â€â‚¬ lifecycle
              Ã¢â€â€š
              Ã¢â€“Â¼
        Native playback engine
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ source inspection
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ timeline mapping
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ bounded decode workers
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ synchronized 8/10-channel source ring
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ transport state machine
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ four-stem mixer
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ optional master-reference bus
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ stereo output resampler
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ small device-output ring
        Ã¢â€â€Ã¢â€â‚¬Ã¢â€â‚¬ diagnostics
              Ã¢â€â€š
              Ã¢â€“Â¼
        Native audio output
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ WASAPI on Windows
        Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ CoreAudio on macOS
        Ã¢â€â€Ã¢â€â‚¬Ã¢â€â‚¬ PipeWire/PulseAudio/ALSA policy on Linux

Independent services
  Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ waveform analyzer and cache
  Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ optional HLS client
  Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ platform media controls
  Ã¢â€â€Ã¢â€â‚¬Ã¢â€â‚¬ release/update service
```

The audio device callback is the strict real-time boundary. It only copies prepared stereo samples, performs unavoidable sample-format conversion, advances counters, and writes silence on underrun. It does not decode, allocate, lock a normal mutex, log, perform I/O, parse JSON, update the UI, or execute network operations.

## 5. Behavioral reference map

The following webplayer files are read-only references. Paths are informative; the native player must not import them at build or runtime.

| Native concern | Webplayer reference |
| --- | --- |
| Backend capability contract | `src/audio/backend/StemPlaybackBackend.ts` |
| Transport states and legal transitions | `src/audio/transport/TransportState.ts` |
| Buffering defaults and session fields | `src/audio/transport/PlaybackSession.ts` |
| Mixer roles, gain range, mute/solo, master behavior | `src/audio/mixer/StemMixerState.ts` |
| Local playback lifecycle and generation handling | `src/audio/backend/LocalContainerPlaybackBackend.ts` |
| Stem MP4 descriptors and role resolution | `src/container/StemContainer.ts` |
| Studio bundle validation and PCM behavior | `docs/architecture/studio-wav-bundle.md` |
| HLS source model and diagnostics | `src/hls/HlsStemset.ts` |
| HLS URL and hosting contract | `docs/hls/static-hosting.md` |
| Existing performance evidence | `docs/performance-results.md` |
| Product overview and qualification commands | `README.md` |

Reference knowledge must be captured in native specifications and tests before implementation relies on it. Native code reviews should not require the webplayer checkout to understand correctness.

## 6. Product behavior invariants

These invariants apply across all phases and source types:

### 6.1 Roles and channel layout

- Required stem roles are `drums`, `bass`, `other`, and `vocals`.
- Each routed track is stereo.
- An optional stereo `master` is a reference route, not a fifth user-controlled stem.
- The synchronized source layout is eight Float32 channels without master and ten Float32 channels with master.
- Every source in one session uses one absolute output-frame timeline.

### 6.2 Mixer semantics

- Neutral stem state is 0 dB, unmuted, and not soloed for all four stems.
- Gain range is -60 dB through +6 dB.
- -60 dB is exact silence; 0 dB is exact unity.
- Mute overrides solo.
- Multiple simultaneous solos are allowed.
- Stored user gain survives mute and solo changes.
- When a master exists, automatic neutral playback uses the master reference.
- Any stem modification in automatic mode selects the stem mix.
- Reset restores neutral controls and returns automatic mode to the master reference.
- Stem gains use 10 ms linear ramps.
- Master/stem switching uses complementary 20 ms linear crossfades.
- No normalization, limiter, compressor, residual correction, hidden gain compensation, or automatic clipping prevention is applied.

### 6.3 Transport semantics

The native state machine retains the conceptual states:

`EMPTY`, `INSPECTING`, `READY`, `PRIMING`, `PLAYING`, `PAUSED`, `SEEKING`, `ENDED`, `ERROR`, and `CLOSED`.

- Illegal transitions fail deterministically.
- Seek targets are expressed on the absolute output-frame timeline.
- A seek or new load invalidates stale decoder work with a generation identifier or equivalent cancellation token.
- End-of-stream is reached at the exact common exclusive end frame.
- Pause does not discard the loaded session.
- Stop returns to the session start without unloading.
- Duration-independent memory use is mandatory.

### 6.4 Source fidelity

- Float32 values are preserved without implicit clipping or normalization.
- Input mismatch is rejected unless a behavior is explicitly part of the format contract.
- Source-rate-to-device-rate conversion occurs only after the final stereo mix.
- Inter-stem resampling is forbidden.
- Full-track PCM caching is forbidden.

## 7. Supported source roadmap

The phases build support in this order:

1. Synthetic native audio for output qualification.
2. Studio WAV bundles with `manifest.json`, four required WAV stems, and optional master.
3. Local `.stem.m4a` and `.stem.mp4` containers with qualified AAC-LC and ALAC profiles.
4. `stem-hls-v1` static HLS VOD sources.

The native player does not need to support arbitrary media files. It should fail closed with structured diagnostics for unsupported, ambiguous, malformed, or unsafe inputs.

## 8. Proposed Rust workspace

```text

Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ Cargo.toml
Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ rust-toolchain.toml
Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ crates/
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer-domain/       roles, states, errors, commands, snapshots
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer-format/       manifest, WAV, MP4, HLS validation
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer-timeline/     rational time and absolute-frame mapping
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer-decode/       PCM, AAC-LC, ALAC adapters
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer-engine/       session, transport, workers, synchronized ring
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer-mixer/        gains, mute/solo, A/B, ramps
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer-output/       CPAL, device policy, resampling, output ring
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer-waveform/     peak pyramids and cache
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer-network/      optional HLS client
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer-platform/     dialogs, media controls, power, paths
Ã¢â€â€š   Ã¢â€â€Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer-ui/           native UI adapter and presentation model
Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ apps/
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer/              production desktop binary
Ã¢â€â€š   Ã¢â€â€Ã¢â€â‚¬Ã¢â€â‚¬ stemplayer-probe/        headless inspection/qualification binary
Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ fixtures/
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ public/
Ã¢â€â€š   Ã¢â€â€Ã¢â€â‚¬Ã¢â€â‚¬ README.md
Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ tests/
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ conformance/
Ã¢â€â€š   Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ integration/
Ã¢â€â€š   Ã¢â€â€Ã¢â€â‚¬Ã¢â€â‚¬ soak/
Ã¢â€â€Ã¢â€â‚¬Ã¢â€â‚¬ docs/
    Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ architecture/
    Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ decisions/
    Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ formats/
    Ã¢â€Å“Ã¢â€â‚¬Ã¢â€â‚¬ performance/
    Ã¢â€â€Ã¢â€â‚¬Ã¢â€â‚¬ release/
```

Crates must have narrow dependency direction. UI and platform crates depend on domain and controller APIs; the engine never depends on the UI. The real-time output crate must not depend on networking, UI, serialization frameworks, or filesystem traversal.

## 9. Cross-phase engineering requirements

### 9.1 Real-time safety

- No heap allocation in the device callback after stream initialization.
- No blocking locks in the device callback.
- No unbounded queues anywhere in playback.
- Every producer/consumer queue has explicit capacity and overflow behavior.
- Decoder, waveform, and network work run outside the callback.
- The render thread has a documented scheduling and priority policy.
- All callback-visible data is preallocated or lock-free/SPSC.

### 9.2 Error model

- Every user-visible failure has a stable code, actionable message, and optional structured details.
- Internal library errors are mapped at crate boundaries.
- Paths, URLs, codec internals, and operating-system details are redacted when not required by the user.
- Panic is never a normal error path.
- Release builds install a top-level panic handler that preserves a safe shutdown path and writes a local redacted crash record only when enabled.

### 9.3 Security and privacy

- Local media remains local.
- No telemetry, analytics, account, upload, or remote logging is introduced.
- HLS performs only explicit user-requested network access.
- Manifest and URL references are validated before opening.
- Path traversal, network-path injection, unsafe schemes, encoded separators, duplicate normalized names, and ambiguous role resolution fail closed.
- Persistent caches do not store full media paths, PCM, encoded audio, or private metadata.
- Updates and release artifacts are signed before automatic update functionality can be enabled.

### 9.4 Determinism and reproducibility

- Format parsers use explicit limits for bytes, nodes, depth, tracks, samples, playlists, segments, and metadata.
- Persistent binary formats are versioned.
- Test fixtures have documented provenance and expected hashes.
- Builds pin the Rust toolchain and lock dependencies.
- Release artifacts include checksums, dependency licenses, and a software bill of materials.

### 9.5 UI resource discipline

- No continuous repaint loop while paused, stopped, hidden, or minimized.
- Playhead interpolation occurs locally in the presentation layer from an authoritative engine snapshot.
- Engine snapshots are rate-limited and coalesced; the UI never receives per-audio-buffer events.
- Waveform rendering is level-of-detail based and viewport bounded.
- Diagnostics update more slowly than the playhead.
- Expensive panels and analyses are lazy.

### 9.6 Accessibility and input

- All transport and mixer controls are keyboard accessible.
- Accessible names, roles, values, and state changes are exposed.
- Focus order is deterministic.
- High-DPI scaling and text scaling are tested.
- Reduced motion disables nonessential transitions.
- Media keys and screen-reader behavior are platform-qualified before release.

## 10. Performance budgets

Phase 0 records exact measurement methodology and reference machines. The following are initial product budgets, not assumed achievements:

| Metric | Initial budget |
| --- | ---: |
| Idle steady-state private memory | <= 50 MiB per application process set |
| WAV playback private memory | <= 75 MiB |
| AAC/ALAC playback private memory | <= 100 MiB |
| Memory growth over 8-hour soak | <= 5 MiB after warm-up |
| WAV playback CPU | <= 5% of one logical core, p95 |
| AAC/ALAC playback CPU | <= 10% of one logical core, p95 |
| Idle application timers | none except explicitly enabled services |
| WAV open to READY on local SSD | <= 100 ms p95 |
| WAV Play to audible output | <= 250 ms p95 |
| Container Play to audible output | <= 500 ms p95 |
| Local seek completion | <= 250 ms p95 |
| Mixer command to rendered effect | <= 30 ms p95 |
| Inter-stem drift | 0 frames |
| Normal-load playback underruns | 0 |
| Full-duration end-frame error | 0 frames |

Measurements must distinguish private memory, resident memory, shared libraries, filesystem cache, and GPU allocations where the platform exposes them. A phase cannot claim a budget without a reproducible measurement command and raw result file.

## 11. Global test matrix

Every applicable phase contributes to the required development matrix:

- Windows 11 x86-64 using default WASAPI output.
- Ubuntu 24.04 WSL2 x86-64 for Linux compilation and headless logic tests.
- 44.1 kHz and 48 kHz sources.
- Device rates of 44.1, 48, 96 kHz where hardware permits.
- Stereo Float32 and common integer device sample formats.
- Sources with and without master.
- Beginning, middle, near-end, repeated, and randomized seeks.
- Device disconnect, default-device change, suspend/resume, and application minimize/restore.
- Long filenames, Unicode normalization, case collisions, malformed paths, and corrupted manifests.
- Playback while waveform analysis is active.
- Playback under controlled CPU, disk, and network stress.
- Complete-track and eight-hour soak runs.

Public synthetic fixtures must cover all structural edge cases. Private listening fixtures remain outside version control and are referenced through environment variables or local configuration.

Native macOS and Linux desktop/audio/renderer/accessibility/package testing is an optional Phase 12 platform-expansion matrix. A skipped or failed optional matrix does not block Windows release, but that platform remains unsupported and no package for it is published.

## 12. Phase index

| Phase | Detailed plan | Primary outcome | Depends on |
| ---: | --- | --- | --- |
| 0 | [PLAN_0.md](PLAN_0.md) | Scope, evidence baseline, decisions, budgets, read-only reference map | None |
| 1 | [PLAN_1.md](PLAN_1.md) | Independent Cargo workspace and domain contracts | 0 |
| 2 | [PLAN_2.md](PLAN_2.md) | Native UI shell, controller boundary, mock engine, headless harness | 1 |
| 3 | [PLAN_3.md](PLAN_3.md) | Qualified native audio-device and real-time output core | 1 |
| 4 | [PLAN_4.md](PLAN_4.md) | Secure Studio WAV bundle inspection and bounded decoding | 1 |
| 5 | [PLAN_5.md](PLAN_5.md) | Sample-synchronous WAV playback, transport, seek, buffering | 2, 3, 4 |
| 6 | [PLAN_6.md](PLAN_6.md) | Mixer parity, master A/B, device-rate resampling | 5 |
| 7 | [PLAN_7.md](PLAN_7.md) | Native `.stem.m4a`/`.stem.mp4` AAC-LC and ALAC playback | 5, 6 |
| 8 | [PLAN_8.md](PLAN_8.md) | Complete native player UI and interaction model | 5, 6, 7 |
| 9 | [PLAN_9.md](PLAN_9.md) | Waveforms, bounded cache, diagnostics, playback-priority policy | 5, 8 |
| 10 | [PLAN_10.md](PLAN_10.md) | Native `stem-hls-v1` playback and grouped network buffering | 7, 9 |
| 11 | [PLAN_11.md](PLAN_11.md) | OS integration, secure packaging, signing, installation | 8, 9; 10 for HLS parity release |
| 12 | [PLAN_12.md](PLAN_12.md) | Windows qualification, performance proof, release gate, optional platform expansion | All release-scoped phases |

## 13. Release milestones

### Milestone A Ã¢â‚¬â€ Headless foundation

Completed after Phases 0Ã¢â‚¬â€œ4. The project can inspect and decode bounded Studio WAV blocks and produce deterministic diagnostics, but it is not yet a user player.

### Milestone B Ã¢â‚¬â€ Native local WAV player

Completed after Phases 5Ã¢â‚¬â€œ6. The application provides synchronized Studio WAV playback, transport, four-stem mixing, optional master A/B, and output resampling.

### Milestone C Ã¢â‚¬â€ Native local container beta

Completed after Phases 7Ã¢â‚¬â€œ9. Local `.stem.m4a`, `.stem.mp4`, and Studio WAV sources have native playback, UI, waveforms, cache, and diagnostics.

### Milestone D Ã¢â‚¬â€ Feature-parity release candidate

Completed after Phases 10Ã¢â‚¬â€œ11. HLS, platform integration, installers, signing, and release packaging are present.

### Milestone E Ã¢â‚¬â€ General availability

Completed when Phase 12 passes for Windows and all Windows release-blocking budgets have evidence. macOS or Linux joins the supported matrix only after its optional Phase 12 expansion passes.

## 14. Execution rules

- Implement one phase at a time unless the plan explicitly marks work as parallelizable.
- Keep each phase shippable behind disabled or internal feature flags where appropriate.
- Each phase ends with a written evidence report, not only passing CI.
- New dependencies require a recorded rationale, license check, maintenance assessment, and footprint measurement.
- Unsafe Rust is denied by default. Any exception is isolated, justified, tested, and reviewed for soundness.
- Performance regressions are treated as correctness regressions for real-time and bounded-memory code.
- No phase may solve a native requirement by changing the webplayer.
- Discoveries from the webplayer are copied into native specifications or test vectors before they become implementation assumptions.

## 15. Completion definition

The project is complete when a user can install the qualified Windows package; open a supported local source or explicit HLS stemset; play, pause, stop, seek, mute, solo, adjust gain, switch master/stem output, reset the mix, inspect waveforms and diagnostics; and close or resume the application without drift, unbounded memory, hidden browser processes, converter dependencies, or modifications to the existing webplayer. macOS and Linux packages are optional and may be called supported only after their Phase 12 expansion passes.
