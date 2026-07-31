# Phase 12 — Windows Qualification, Optional Platform Expansion, and Release Gate

## Goal

Prove that the native player is correct, stable, secure, installable, accessible, and materially resource-efficient on Windows. Convert all prior phase evidence into a Windows release decision with explicit known limitations and rollback readiness. macOS and native Linux qualification is an optional platform-expansion work package in this phase.

## Entry criteria

- Every release-scoped implementation phase has passed its local exit gate.
- The Windows release artifact from Phase 11 exists. Optional macOS/Linux artifacts are required only when their platform expansion is selected.
- Performance budgets and reference machines are final.
- Public synthetic, generated, and private listening fixtures are available.
- No unresolved critical security or correctness defect remains.

## Work package 12.1 — Release candidate freeze

Create a release-candidate tag or immutable commit. Record:

- Source revision.
- Rust toolchain.
- Dependency lock hash.
- Feature set.
- Platform package hashes.
- Signing identities and verification results.
- Fixture versions.
- Reference machine inventory.

Only release-blocking fixes are accepted after freeze, and each fix resets affected qualification evidence.

## Work package 12.2 — Functional conformance matrix

Run the complete matrix for:

- Studio WAV bundles.
- AAC-LC stem containers.
- ALAC stem containers.
- HLS if included.
- Master present and absent.
- 44.1 and 48 kHz source rates.
- Supported device rates and formats.
- All transport states and legal transitions.
- Gain, mute, solo, multi-solo, reset, automatic/manual A/B.
- Waveform miss/hit/clear/evict.
- Output-device selection and loss.
- Settings migration and corruption.
- File associations and second-instance open.
- Suspend/resume and media controls.

Each case records pass/fail, artifact, platform, and linked defect.

## Work package 12.3 — Sample-accuracy proof

For synthetic and private sources, prove:

- Identical common first frame.
- Identical common exclusive end frame.
- Zero unexplained gaps and overlaps.
- Zero inter-stem drift over 30-minute accelerated and complete real-time runs.
- Exact seek targets after decoder preroll.
- Exact end after resampling.
- Correct channel routing.
- Correct gain and crossfade sample shapes.
- HLS segment continuity and preroll trim where applicable.

Store machine-readable reports and small legal reference hashes, not private audio.

## Work package 12.4 — Soak and stress

Required runs:

- Eight-hour continuous local playback on Windows and on each optionally selected platform.
- Complete-track compressed playback.
- Repeated track open/close loop.
- 500 randomized seeks.
- Rapid mixer automation.
- Playback during waveform analysis.
- Playback under controlled CPU and disk pressure.
- Device disconnect/reconnect loop.
- Suspend/resume loop.
- HLS network shaping and temporary failure when included.
- Window minimize/restore and long paused state.

Release-blocking normal-load criteria:

- Zero underruns.
- Zero ring overflows.
- Zero deadlocks.
- Zero crashes.
- No unbounded queue or cache growth.
- No source-frame mismatch.

Stress runs may permit controlled buffering, but never drift, corruption, or uncontrolled resource growth.

## Work package 12.5 — Performance qualification

Measure every budget from the Phase 0 decision record using release builds:

- Idle private memory.
- Playback memory by source type.
- CPU p50/p95/max.
- Wakeups/timers.
- READY latency.
- First-audio latency.
- Seek median/p95.
- Mixer latency.
- Callback deadline distribution.
- Buffer occupancy.
- Analysis and cached reopen.
- HLS latency/cache if included.
- Installed size and startup time.

Separate UI, decoder, waveform, network, and output costs where possible. Record deviations with one of:

- Fixed before release.
- Budget formally revised with product approval and evidence.
- Platform removed from supported matrix.

Do not hide platform outliers in an average.

## Work package 12.6 — Memory and leak analysis

Use platform tools and long-run counters to inspect:

- Heap growth.
- Thread count.
- File and device handles.
- Decoder objects.
- Ring and analysis buffers.
- GPU resources.
- Cache maps.
- Network connections.
- UI objects after source replacement.

Warm-up allocations are separated from linear growth. Any sustained source-open or seek leak is release blocking.

## Work package 12.7 — Security qualification

Perform:

- Dependency audit.
- License audit.
- SBOM validation.
- Manifest/path/URL fuzzing.
- WAV and MP4 parser fuzzing.
- Playlist and segment-reference fuzzing.
- Integer overflow and allocation-limit tests.
- Malformed local IPC tests.
- Cache corruption tests.
- Signature/update verification.
- Artifact secret/private-path scan.
- Offline behavior verification.

Network tests confirm no ambient credentials, no unexpected hosts, and no network access during local-only use.

## Work package 12.8 — Accessibility and UX qualification

On Windows and on each optional platform being considered for publication:

- Complete open/play/seek/mix/reset tasks by keyboard.
- Complete core tasks with a screen reader.
- Verify focus restoration after errors and dialogs.
- Verify large text and high DPI.
- Verify contrast and reduced motion.
- Verify clear device and format error guidance.
- Conduct human listening checks for clicks, routing, channel balance, and end behavior.

Accessibility failures in core tasks block release.

## Work package 12.9 — Installer and upgrade qualification

Use clean virtual or physical systems to verify:

- Fresh install.
- Launch without development tools.
- File association.
- Upgrade from previous test version.
- Settings/cache preservation policy.
- Uninstall.
- Signature/notarization.
- Standard-user operation.
- No orphan helper process or service.
- No browser, Python, Node.js, FFmpeg, or converter dependency.

## Work package 12.10 — Documentation and support package

Publish:

- Supported formats and limits.
- Supported OS and architectures.
- Install/uninstall instructions.
- Keyboard shortcuts.
- Mixer/master behavior explanation.
- Cache/privacy explanation.
- Network behavior for HLS and updates.
- Troubleshooting for audio devices.
- Diagnostic export instructions.
- Known limitations.
- Third-party licenses.
- Release notes.

Documentation must not tell users to modify or install the webplayer.

## Work package 12.11 — Release decision

Hold a formal release review with:

- Phase exit-gate status.
- Open defect list by severity.
- Performance budget table.
- Security and license status.
- Accessibility status.
- Package/signing status.
- Rollback plan.
- Support owner.
- Known limitations.

Possible outcomes:

- Approve GA.
- Approve limited beta with explicit platform/feature restrictions.
- Reject and return to named phase(s).

## Work package 12.12 — Post-release monitoring without telemetry

Because the product has no automatic telemetry:

- Provide explicit local diagnostic export.
- Define issue template fields.
- Preserve build identifiers.
- Publish checksum and known-issue page.
- Create a reproducible support benchmark command.
- Define urgent security update procedure.
- Track opt-in user reports without collecting media.

## Work package 12.13 - Optional macOS and native Linux platform expansion

This work package is optional and does not block a Windows release. Run it only when a macOS or Linux package is intended for publication.

For macOS:

- Build and run on macOS 14 or newer on Apple silicon.
- Qualify CoreAudio devices, loss/recovery, suspend/resume, and one-hour/eight-hour soaks.
- Qualify Slint rendering, software fallback, minimize/restore, system light/dark themes, Retina scaling, keyboard use, and VoiceOver.
- Measure every performance budget and inspect GPU/heap/handle growth.
- Install, sign, notarize, upgrade, uninstall, and verify standalone operation.

For native Linux:

- Build and run on Ubuntu 24.04 and the selected Fedora baseline.
- Qualify PipeWire first, PulseAudio second, and ALSA fallback on real desktop sessions.
- Qualify X11 and Wayland only after the selected backend graph passes the advisory audit.
- Qualify software/GPU rendering, minimize/restore, light/dark themes, high DPI, keyboard use, and Orca.
- Measure every performance budget and inspect heap/file/device/GPU resource growth.
- Install, upgrade, uninstall, and verify the selected package formats and desktop integration.

Skipping or failing this work package leaves the affected platform unsupported and unpublished. Passing it adds that platform to the supported matrix without changing the Windows release decision.

## Deliverables

- Immutable release-candidate record.
- Functional conformance report.
- Sample-accuracy reports.
- Soak and stress reports.
- Performance budget report.
- Memory/leak report.
- Security/fuzzing report.
- Accessibility report.
- Installer/upgrade report.
- Optional macOS/Linux platform-expansion report when selected.
- User and support documentation.
- Signed release decision.
- Rollback and urgent-update procedure.

## Release-blocking conditions

- Any alteration or runtime dependency on the webplayer.
- Any bundled converter, Python, model, Node.js, browser, or WebView runtime.
- Inter-stem drift or unexplained frame mismatch.
- Real-time callback allocation or blocking lock.
- Unbounded playback memory or queue.
- Normal-load underrun or overflow in required soak runs.
- Critical parser, path, URL, IPC, update, or signing vulnerability.
- Core keyboard or screen-reader task failure on a package intended for publication.
- Unsigned package where signing is required.
- Missing license obligations or SBOM.
- Unexplained sustained memory/handle growth.

## Exit gate

Phase 12 is complete when the Windows package passes functional, sample-accuracy, stability, performance, security, accessibility, and installation gates; remaining limitations are explicit and accepted; rollback is ready; and the release is demonstrably a standalone fully native Rust player that leaves the webplayer untouched. Optional macOS/Linux expansion does not block Windows completion, but an optional platform is unsupported unless its full work package passes.
