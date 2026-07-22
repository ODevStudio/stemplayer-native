# Phase 8 — Complete Native Player UI and Interaction Model

## Goal

Turn the technical local-playback foundation into a complete, efficient desktop player experience. Integrate Studio WAV and local stem-container playback with native file workflows, transport, mixer, output devices, errors, settings, and accessibility while retaining event-driven resource discipline.

## Entry criteria

- Phase 2 native shell and presentation model are stable.
- Phases 5–7 provide real local playback, mixer, and container capabilities.
- UI toolkit and renderer remain within Phase 0 budgets.

## Work package 8.1 — Source opening workflows

Implement clear entry points:

- Open stem container.
- Open Studio bundle manifest.
- Open Studio bundle folder.
- Drag and drop supported file/folder.
- Reopen recent source using a privacy-conscious local list.

The UI passes explicit selections to the controller. It does not traverse arbitrary parent directories or infer converter project structures.

Recent entries store only what is necessary for a local convenience feature and can be disabled/cleared. Missing or moved sources fail gracefully.

## Work package 8.2 — Transport UI

Provide:

- Play/pause.
- Stop.
- Current time and duration.
- Seek bar or waveform seek.
- Beginning/end handling.
- Buffering indication.
- Disabled states based on controller capabilities.
- Keyboard shortcuts.
- Media-control command mapping prepared for Phase 11.

During seek dragging, show preview position without issuing continuous decoder seeks. Commit on release and optionally at a low bounded interval for scrubbing if later qualified.

## Work package 8.3 — Mixer UI

For drums, bass, other, vocals:

- Gain fader with -60 to +6 dB range.
- Exact 0 dB reset affordance.
- Mute.
- Solo.
- Accessible value and state text.
- Keyboard fine and coarse adjustment.

Provide:

- Reset all.
- Current automatic/manual output mode.
- Master/stem selection only when master exists.
- Clear explanation that master is a reference path.

The UI never computes effective mute/solo or output mode independently; it displays engine-derived state.

## Work package 8.4 — Output-device settings

Add:

- Current device.
- Default device option.
- Explicit device list.
- Source and device sample rates.
- Recoverable device error guidance.
- Optional latency mode only if qualified.

Device changes are controller commands and display a controlled transition. Avoid showing unstable internal identifiers.

## Work package 8.5 — Application state and settings

Version and persist only user preferences:

- Window placement with safe screen-bound restoration.
- Theme preference.
- Last volume/fader policy only if product-approved; default should avoid surprising persistent mix changes.
- Output-device preference.
- Waveform cache limit and enabled state.
- Diagnostics visibility.
- Recent source behavior.

Do not persist loaded PCM, source metadata not needed by the user, credentials, or HLS cookies.

## Work package 8.6 — Error and recovery UX

Errors must answer:

- What failed.
- Whether playback stopped or source remains loaded.
- What the user can do.
- Technical code for support.
- Expandable redacted details.

Support recovery actions such as:

- Retry device.
- Choose another device.
- Reopen source.
- Reset player.
- Clear corrupt cache entry.
- Copy redacted diagnostics.

Never expose panic traces as the primary user message.

## Work package 8.7 — Responsive layout and scaling

Design for:

- Compact laptop window.
- Normal desktop window.
- High-DPI monitors.
- Large text.
- Long translated strings.
- Four visible stem controls without excessive scrolling.
- Diagnostics hidden by default.

The player need not mimic the web layout exactly. It should preserve tasks and semantics while using native conventions.

## Work package 8.8 — Accessibility completion

Qualify:

- Screen-reader names, roles, values, and state changes.
- Full keyboard use.
- Focus restoration after dialogs.
- Contrast.
- Large text.
- Reduced motion.
- Error announcements.
- Time and gain value pronunciation.
- Drag-and-drop alternatives.

Accessibility failures in core playback and mixer tasks are release blocking.

## Work package 8.9 — Event and redraw policy

Enforce:

- Maximum authoritative snapshot rate while playing.
- Lower diagnostic rate.
- No source-frame event per callback.
- No redraw while hidden/minimized except required OS events.
- Waveform redraw only when viewport, peaks, cursor, or control state changes.
- Coalesced gain and seek input.
- Lazy diagnostics formatting.

Instrument UI repaint count and time per frame in diagnostics builds.

## Work package 8.10 — Native UI testing

Create automation around stable accessibility identifiers or toolkit-native test hooks. Cover:

- Open success and failure.
- Transport state transitions.
- Seek preview/commit.
- All mixer combinations.
- Master absent/present.
- Device loss and recovery.
- Settings migration.
- Cache controls placeholder.
- Keyboard and screen reader flows.
- Closing during activity.

Use fake and real engine backends. Fake tests provide determinism; a smaller real suite proves wiring.

## Deliverables

- Complete native local-player UI.
- File/folder open and drag-and-drop workflows.
- Transport and mixer controls.
- Output-device selection.
- Versioned settings.
- Error recovery UX.
- Responsive and accessible layout.
- UI automation suite.
- Redraw and idle-resource report.

## Tests and evidence

- Every command enabled/disabled state.
- Keyboard-only task completion.
- Screen-reader task completion.
- High-DPI and large-text screenshots.
- Long-name and Unicode display.
- Settings corruption and migration.
- Device change during play.
- Close during load/seek/analysis.
- Minimized playback and minimized pause resource tests.
- UI command flood and snapshot flood resilience.

## Resource checks

- Idle memory and CPU remain within budgets.
- Playing UI overhead measured separately from engine CPU.
- Repaint frequency measured while playing, paused, hidden, and minimized.
- Recent list and diagnostics memory bounded.
- No WebView, JavaScript runtime, or browser process appears in the process tree.

## Risks and mitigations

- **UI parity becomes pixel-copy work:** prioritize native tasks and semantics, not browser appearance.
- **Settings persist unsafe surprises:** keep defaults conservative and schema explicit.
- **Diagnostics formatting causes jitter:** compute off the real-time path and rate-limit.
- **Accessibility support varies by toolkit:** keep Windows tests active; native macOS/Linux task qualification is optional Phase 12 platform-expansion work.

## Exit gate

Phase 8 is complete when Windows users can perform every local playback and mixer task through a fully native, accessible, event-driven UI; errors are recoverable; settings are safe; and the UI stays within idle and playback overhead budgets. Native macOS/Linux UI qualification is optional Phase 12 work.
