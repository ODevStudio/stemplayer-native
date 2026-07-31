# Phase 0 â€” Boundary, Reference Audit, and Decision Gates

## Progress

- Status: exit gate passed.
- Started: 2026-07-21.
- Completed: 2026-07-22.
- Read-only webplayer revision: `08dfe03cc4ada7b8cac84d6191198e25e59aeb1e`.
- Native workspace: ``, isolated from the existing Node workspace.
- Qualification hosts: Windows 11 x86-64 and Ubuntu 24.04 WSL2 x86-64.
- Native macOS and Linux desktop qualification is an optional Phase 12 platform-expansion step and does not block Phase 1.

## Goal

Convert the existing product knowledge into an explicit native-player specification before writing production code. Establish the standalone project boundary, baseline behavior, target platforms, resource budgets, dependency policy, and decision records needed by later phases.

## Boundary

The webplayer is read-only during this phase. No source, configuration, test, package, or documentation change in the webplayer is required. The native player is planned as a separate repository named `native player`; if organizational constraints require a monorepo temporarily, the Cargo workspace must remain independently buildable and must not modify or invoke the webplayer.

The stem converter is not inspected as a runtime dependency. Only published output-format examples may be used as fixtures.

## Entry criteria

- Access to the current webplayer source for read-only inspection.
- At least one legal public synthetic fixture plan.
- Availability of a representative Windows machine and Ubuntu WSL2 for Linux compilation/headless tests.
- Product and release/security ownership must be named before a public release decision in Phase 12.

## Work package 0.1 â€” Reference inventory

Create `docs/reference/webplayer-behavior-map.md` in the native project. For each relevant webplayer concern, record:

- Reference file path and revision identifier.
- Observed behavior.
- Native requirement derived from it.
- Whether the behavior is mandatory parity, a browser-specific implementation detail, or intentionally deferred.
- Native test that will eventually prove the requirement.

At minimum inspect:

- Backend capability surface.
- Transport state transitions.
- Buffer watermarks and bounded-ring behavior.
- Stem role resolution.
- MP4 edit-list mapping.
- Mixer state derivation and automation durations.
- Studio WAV validation.
- Waveform levels and cache privacy.
- HLS manifest, playlist, segment, and preroll handling.
- Error and diagnostic codes.
- Existing performance reports and documented limitations.

Do not copy browser-only mechanisms such as AudioWorklet, SharedArrayBuffer, IndexedDB, WebCodecs, browser workers, or DOM events into the native requirements. Capture their purpose instead.

## Work package 0.2 â€” Native capability specification

Create versioned native specifications:

- `docs/formats/studio-wav-v1.md`
- `docs/formats/stem-mp4-supported-profile.md`
- `docs/formats/stem-hls-v1-client-profile.md`
- `docs/architecture/transport-semantics.md`
- `docs/architecture/mixer-semantics.md`
- `docs/architecture/timeline-semantics.md`
- `docs/architecture/cache-privacy.md`

Every specification distinguishes:

- Input syntax and size limits.
- Required versus optional fields.
- Normalization rules.
- Rejection conditions.
- Exact numeric behavior.
- Error codes.
- Compatibility assumptions.
- Security constraints.

## Work package 0.3 â€” Platform support decision

Define the first supported matrix:

- Windows architecture and minimum OS release.
- macOS architecture and minimum OS release.
- Linux distributions or runtime baselines.
- Linux audio priority: PipeWire, PulseAudio, ALSA fallback, and whether JACK is supported.
- GPU requirements or software-rendering fallback.
- Whether Intel macOS is release-blocking.
- Whether ARM64 Windows is release-blocking or post-GA.

Record the result in a decision document. Avoid a vague â€œall desktopsâ€ promise.

## Work package 0.4 â€” Native UI gate

Evaluate Slint as the default native UI toolkit with a minimal proof application containing:

- One main window.
- Four mixer rows.
- A waveform placeholder.
- Keyboard focus and accessible labels.
- Drag-and-drop event capture.
- Event-driven redraw.
- Minimize/restore.
- Light and dark system theme handling.

Measure:

- Clean binary size.
- Idle private memory.
- CPU and wakeups while idle and minimized.
- High-DPI behavior.
- Windows UI Automation exposure and automated NVDA focus announcements; a blind-user usability study remains optional final qualification work.
- License obligations and release suitability.
- Renderer compatibility on common GPUs and virtual machines.

Decision gate:

- Adopt Slint and document renderer policy, or
- Select another fully native Rust UI toolkit and update only the UI-specific portions of subsequent plans.

A WebView-based fallback is not allowed because it would violate the product definition.

## Work package 0.5 â€” Audio and codec dependency gate

Prototype and assess:

- CPAL or equivalent for native output.
- Symphonia or equivalent for qualified AAC-LC, ALAC, WAV, and MP4 support.
- Rubato or equivalent for post-mix stereo sample-rate conversion.
- SPSC/ring-buffer crate versus a small internal implementation.
- SHA-256 implementation.
- HTTP/TLS stack for the later HLS phase.

For each candidate record:

- Supported platforms and backends.
- License.
- Maintenance state.
- Transitive dependency cost.
- Unsafe-code surface.
- Known format limitations.
- Whether the dependency can allocate or block in the real-time path.
- Whether a small adapter can isolate replacement later.

Do not commit to a decoder based only on â€œfile opens.â€ The gate requires exact frame-count, seek, edit-list, and bounded-memory feasibility.

## Work package 0.6 â€” Measurement methodology

Create reproducible scripts and report templates for:

- Private and resident memory.
- Per-process and normalized CPU.
- Wakeups or timer activity.
- Time to READY.
- Time to first audio.
- Seek median and p95.
- Mixer control latency.
- Ring high-water marks.
- Underruns and overflows.
- Long-run memory growth.
- Exact end frame.

Document tool differences by platform. Raw measurements must be saved as machine-readable artifacts with hardware, OS, build profile, source fixture, sample rate, output device, and timestamp.

## Work package 0.7 â€” Fixture and provenance policy

Define three fixture classes:

1. **Public synthetic fixtures** committed to the repository.
2. **Generated qualification fixtures** created deterministically during tests.
3. **Private listening fixtures** stored outside version control.

Public fixtures must cover malformed manifests, role ambiguity, Unicode collisions, WAV bit depths, sample-rate mismatch, frame-count mismatch, truncated chunks, MP4 edit cases, AAC/ALAC packet boundaries, HLS traversal attempts, duplicated preroll, and corrupted hashes.

No fixture may contain unlicensed commercial audio.

## Work package 0.8 â€” Initial error taxonomy

Create stable categories:

- Transport and lifecycle.
- Audio device and stream.
- Manifest and path validation.
- WAV structure and PCM format.
- Container and role resolution.
- Codec configuration and decode.
- Timeline gap, overlap, priming, padding, and end mismatch.
- Buffer underrun, overflow, and generation mismatch.
- Cache corruption and version mismatch.
- Network, TLS, manifest, playlist, and segment failure.
- Platform integration and update failure.

Map known webplayer error meanings where useful, but use native-specific codes for native failures rather than pretending a Web Audio error occurred.

## Deliverables

- Native repository or isolated workspace location decision.
- Read-only reference map.
- Native capability specifications.
- Supported-platform decision.
- UI toolkit decision.
- Audio/codec/resampler dependency decisions.
- Measurement scripts and report schema.
- Fixture policy and initial synthetic fixture list.
- Error taxonomy.
- Initial risk register.
- Initial release budgets in `PLAN.md` either accepted or amended with rationale.

## Tests and evidence

- Build and run the UI proof on Windows.
- Produce Windows visible, minimized, restored, DPI, accessibility, CPU, context-switch, renderer, and memory reports.
- Decode deterministic AAC-LC and ALAC fixtures with exact presentation-frame, repeat, seek, edit-window, and duration-independent-memory checks.
- Open and close a native WASAPI output stream.
- Compile and run headless logic tests on Ubuntu 24.04 WSL2.
- Verify the native project can be built with the webplayer unavailable.
- Verify no native build step invokes Node.js, pnpm, Python, FFmpeg, or converter commands.
- Defer native macOS and Linux desktop/audio/accessibility/package qualification to the optional Phase 12 platform-expansion work package.

## Risks and mitigations

- **Codec library lacks exact timeline behavior:** keep timeline mapping in a native crate and treat decoder output as packet PCM only.
- **UI toolkit footprint exceeds budget:** use the measured gate before building product UI.
- **Linux audio fragmentation:** decide an explicit backend policy and maintain fallback tests.
- **Private fixtures hide defects:** require synthetic structural coverage before private listening gates.
- **Reference drift:** pin the inspected webplayer revision in documentation; later changes are new inputs, not silent requirements.

## Exit gate

Phase 0 is complete when Windows UI/audio and codec feasibility evidence passes, Ubuntu WSL2 compiles and passes headless tests, dependency advisory/license inventories are recorded, major dependency and platform decisions have written outcomes, measurement commands are reproducible, and the standalone boundary has been proven mechanically. Native macOS and Linux desktop qualification is optional Phase 12 evidence; those platforms cannot be advertised as supported unless that work package passes.

## Phase 0 execution record - 2026-07-21

### Deliverables created

The isolated workspace is ``. It is mechanically independent and may carry Phase 1; moving it to a separate production repository remains a release-boundary task.

- Reference map: `docs/reference/webplayer-behavior-map.md`.
- Native source profiles: `docs/formats/`.
- Transport, mixer, timeline, and cache contracts: `docs/architecture/`.
- Repository, platform, UI, dependency, security, and budget decisions: `docs/decisions/phase0-decisions.md`.
- Fixture/provenance policy: `docs/fixtures/policy.md`.
- Error taxonomy: `docs/errors/taxonomy.md`.
- Risk register: `docs/risks.md`.
- Measurement method/schema/script: `docs/measurement/` and `scripts/measure-windows.ps1`.
- Native proof: `phase0-probe/`.

### Decisions

- Production location: separate `native player` repository; the in-tree workspace is temporary and mechanically independent.
- Release matrix: Windows 11 24H2 x86-64 is the qualified required platform. macOS 14+ Apple silicon and native Ubuntu 24.04/Fedora 44 x86-64 are optional Phase 12 expansions and remain unsupported until qualified.
- UI: Slint 1.17.1 with winit/software rendering is adopted for Windows and X11 compilation. FemtoVG is rejected because it misses the idle private-memory budget. Wayland is disabled until its vulnerable build-time dependency path is patched upstream.
- Dependencies: CPAL, Rubato, rtrb, sha2, and Symphonia are accepted for Phase 1 boundaries. Symphonia passed Phase 0 AAC-LC/ALAC feasibility; full adversarial codec qualification remains Phase 7 work. ureq/rustls remains provisional for Phase 10.
- Budgets: accepted unchanged. No budget was lowered to pass a candidate.

### Windows evidence

Host: Windows 11 x86-64, Rust 1.97.1, local default WASAPI output.

| Evidence | Result |
| --- | --- |
| `cargo check --locked` | PASS |
| `cargo fmt --all -- --check` | PASS after rustfmt |
| `cargo clippy --locked --all-targets -- -D warnings` | PASS |
| `cargo test --locked` | PASS, 1 test |
| Release build | PASS, software-only binary 12,858,368 bytes |
| SHA-256 known vector | PASS |
| rtrb bounded/order proof | PASS |
| Rubato 44.1 to 48 kHz stereo construction | PASS |
| Generated 48 kHz Float32 WAV | PASS, 4 decoded frames |
| Private five-track AAC container | PASS for inspect plus first 1,024 decoded frames; this is not exact-frame codec qualification |
| Default output stream | PASS, 44.1 kHz stereo Float32, 23 callbacks, 0 errors |
| Software-only UI, 20 seconds | PASS execution; 11,218,944 private bytes, 27,738,112 resident bytes, normalized CPU average 0.0189% |
| FemtoVG UI, 20 seconds | FAIL budget; 106,668,032 to 108,503,040 private bytes |
| Standalone copy test | PASS: copied without `target`/artifacts to `C:\tmp\native player-phase0-boundary`, then `cargo test --locked --offline` |
| Build-step dependency scan | PASS: no Node.js, pnpm, Python, FFmpeg, webplayer path, or converter dependency in native build/run configuration |
| Windows UI Automation | PASS: 15 keyboard-focusable elements, all required accessible names exposed |
| NVDA 2026.1.1 | PASS: announced all four gain sliders plus Play and Stop through actual NVDA speech events |
| DPI and theme | PASS on connected display: 216 DPI / 225% scale, active dark system theme |
| Visible/minimized/restored idle states | PASS: zero sampled normalized CPU and context switches per second; restore completed |
| GPU memory counters, software renderer | PASS: available, zero dedicated/shared allocation reported |
| AAC-LC exact feasibility | PASS at 441,000 and 2,646,000 presentation frames; repeat and mid-stream exact seek passed |
| ALAC exact feasibility | PASS at 441,000 and 2,646,000 presentation frames; repeat and mid-stream exact seek passed |
| Codec duration-memory comparison | PASS: AAC +2,174,976 bytes; ALAC +12,288 bytes from 10 to 60 seconds |

Raw reports are `artifacts/phase0/windows-software-selected/`, `artifacts/phase0/windows-femtovg/ui-idle.json`, `artifacts/phase0/windows-ui/ui-qualification.json`, `artifacts/phase0/windows-nvda/nvda-qualification.json`, and `artifacts/phase0/windows-codecs/codec-qualification.json`.

### WSL evidence

Ubuntu 24.04.4 LTS under WSL2 x86-64 used Rust 1.97.1 and an isolated `/tmp` Cargo target directory. `cargo test --locked` compiled the Linux target and passed the headless primitive test; `cargo metadata --locked --no-deps --format-version 1` also passed. This is compilation/headless evidence only, not native Linux desktop, renderer, audio, or accessibility qualification. Raw evidence is `artifacts/phase0/wsl-ubuntu/headless.json`.

### Dependency evidence

The final locked graph contains 602 packages across all target-specific branches after selecting Slint's X11-only Linux backend. `cargo audit` found zero vulnerabilities with no ignored advisories. The earlier Wayland graph exposed two `quick-xml 0.39.4` denial-of-service advisories through `wayland-scanner`; removing Wayland removed that vulnerable build path. The license inventory found zero unlicensed crates and records every detected license. Four Slint-transitive crates are reported unmaintained and remain tracked for Phase 12 dependency refresh. Raw evidence is `artifacts/phase0/dependencies/audit.json`.

### Deferred optional Phase 12 evidence

- Native macOS CoreAudio, UI, renderer, VoiceOver, performance, package, and notarization qualification.
- Native Linux PipeWire/PulseAudio/ALSA, Wayland/X11, UI, renderer, Orca, performance, and package qualification.
- Blind-user usability study, light-theme coverage, additional monitor scales, and hardware GPU renderer coverage.
- Final dependency refresh, third-party notices, SBOM, and formal release/security ownership.
- Separate production repository, independent CI, signing, and rollback evidence.

These items do not block Phase 1. macOS and Linux remain unsupported unless the optional Phase 12 platform-expansion work passes. Phase 0 passed its revised exit gate; Phase 1 is active but not started. No existing webplayer or converter source, package, configuration, or test file was changed.