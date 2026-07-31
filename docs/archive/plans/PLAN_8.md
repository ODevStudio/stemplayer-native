# Phase 8 â€” Complete Native Player UI and Interaction Model

## Goal

Turn the technical local-playback foundation into a complete, efficient desktop player experience. Integrate Studio WAV and local stem-container playback with native file workflows, transport, mixer, output devices, errors, settings, and accessibility while retaining event-driven resource discipline.

## Entry criteria

- Phase 2 native shell and presentation model are stable.
- Phases 5â€“7 provide real local playback, mixer, and container capabilities.
- UI toolkit and renderer remain within Phase 0 budgets.

## Work package 8.1 â€” Source opening workflows

Implement clear entry points:

- Open stem container.
- Open Studio bundle manifest.
- Open Studio bundle folder.
- Drag and drop supported file/folder.
- Reopen recent source using a privacy-conscious local list.

The UI passes explicit selections to the controller. It does not traverse arbitrary parent directories or infer converter project structures.

Recent entries store only what is necessary for a local convenience feature and can be disabled/cleared. Missing or moved sources fail gracefully.

## Work package 8.2 â€” Transport UI

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

## Work package 8.3 â€” Mixer UI

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

## Work package 8.4 â€” Output-device settings

Add:

- Current device.
- Default device option.
- Explicit device list.
- Source and device sample rates.
- Recoverable device error guidance.
- Optional latency mode only if qualified.

Device changes are controller commands and display a controlled transition. Avoid showing unstable internal identifiers.

## Work package 8.5 â€” Application state and settings

Version and persist only user preferences:

- Window placement with safe screen-bound restoration.
- Theme preference.
- Last volume/fader policy only if product-approved; default should avoid surprising persistent mix changes.
- Output-device preference.
- Waveform cache limit and enabled state.
- Diagnostics visibility.
- Recent source behavior.

Do not persist loaded PCM, source metadata not needed by the user, credentials, or HLS cookies.

## Work package 8.6 â€” Error and recovery UX

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

## Work package 8.7 â€” Responsive layout and scaling

Design for:

- Compact laptop window.
- Normal desktop window.
- High-DPI monitors.
- Large text.
- Long translated strings.
- Four visible stem controls without excessive scrolling.
- Diagnostics hidden by default.

The player need not mimic the web layout exactly. It should preserve tasks and semantics while using native conventions.

## Work package 8.8 â€” Accessibility completion

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

## Work package 8.9 â€” Event and redraw policy

Enforce:

- Maximum authoritative snapshot rate while playing.
- Lower diagnostic rate.
- No source-frame event per callback.
- No redraw while hidden/minimized except required OS events.
- Waveform redraw only when viewport, peaks, cursor, or control state changes.
- Coalesced gain and seek input.
- Lazy diagnostics formatting.

Instrument UI repaint count and time per frame in diagnostics builds.

## Work package 8.10 â€” Native UI testing

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

## Implementation record

### 2026-07-26 - Percentage gain controls and bounded desktop layout

Status: in progress.

Implemented:

- Matched the web player's nonlinear fader curve from -60 through +6 dB while keeping the engine's validated dB contract unchanged.
- Added visible linear-gain percentage values for every stem, including exact 0%, 100%, and 200% endpoint behavior and a per-stem 100% reset affordance.
- Added bounded validation for UI fader positions and regression coverage for silence, attenuation, unity, boost, non-finite input, and out-of-range input.
- Changed the Windows application executable to the native GUI subsystem so direct launches do not create a Windows Terminal or console window.
- Capped the player content at 920 by 640 logical pixels and centered it in larger windows. Slint retains DPI-aware physical scaling while maximized windows no longer enlarge the controls without limit.

Verification:

| Check | Evidence | Result |
| --- | --- | --- |
| Workspace regression | `cargo test --workspace -q` | PASS, 116 passed and 4 environment-gated tests ignored |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Development UI build | `cargo build -p stemplayer --no-default-features --features development` | PASS |
| Production release build | `cargo build --release -p stemplayer` | PASS |
| Windows executable subsystem | Parsed the release PE optional header; subsystem value 2 (`IMAGE_SUBSYSTEM_WINDOWS_GUI`) | PASS |
| Native visual check | Normal window and maximized 1707 by 960 desktop inspection | PASS, percentage controls remain legible and the maximized content stays centered and bounded |
| Accessibility tree | Mixer sliders expose gain labels; value buttons expose 100 percent reset names | PASS |

Pending:

- Complete the remaining Phase 8 source workflows, settings, recovery UX, device transitions, accessibility qualification, UI automation, and resource evidence.
- Do not mark the Phase 8 exit gate passed until every listed work package and exit-gate requirement is complete.

### 2026-07-26 - Versioned preference persistence and device restoration

Status: in progress.

Implemented:

- Added settings writes that create the configuration directory, reject unsupported schema versions, and retain the existing corruption-recovery behavior.
- Persisted diagnostics visibility on clean application shutdown without storing source or mixer state.
- Persisted explicit output-device preference when changed.
- Restored an available saved output device at startup and fell back to the system default when the saved device is missing.
- Added focused regression coverage for settings round trips, unsupported schema rejection, and missing-device fallback.

Verification:

| Check | Evidence | Result |
| --- | --- | --- |
| Focused settings and app tests | `cargo test -p stemplayer-platform -p stemplayer` | PASS, 8 passed and 1 hardware-gated test ignored |
| Workspace regression | `cargo test --workspace -q` | PASS, 118 passed and 4 environment-gated tests ignored |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Development UI build | `cargo build -p stemplayer --no-default-features --features development` | PASS |
| Production release build | `cargo build --release -p stemplayer` | PASS |

Pending:

- Add window placement, theme, cache, and recent-source preferences only with their corresponding Phase 8 UI workflows.
- Complete source workflows, recovery UX, accessibility qualification, UI automation, and resource evidence before marking the Phase 8 exit gate passed.

### 2026-07-26 - Complete native interaction model and Windows qualification

Status: exit gate passed.

Implemented:

- Added explicit native entry points for stem containers, Studio manifests, and Studio folders, plus winit file/folder drop handling.
- Added a bounded eight-entry recent-source list that can be disabled or cleared. Disabling it also removes stored paths.
- Kept transport and all four mixer rows controller-driven, with committed seek-on-release, coalesced gain input, keyboard shortcuts, exact unity reset, mute, solo, reset-all, and master availability.
- Added native output-device selection, saved-device restoration with default fallback, and visible source/output sample rates.
- Completed schema-versioned settings for safe window size and on-screen placement, system/light/dark theme, output preference, diagnostics visibility, waveform-cache preference and limit, and recent-source behavior. Mixer state and loaded-source metadata remain transient.
- Added recoverable error presentation with playback/source status, stable support code, redacted bounded details, retry, source selection, reset, and expandable details.
- Added bounded scrolling for unusually short work areas without reducing control or text size. Windows display scaling remains authoritative; qualification ran at the configured 225% scale without an application override.
- Added deterministic development modes and Windows UI Automation for open, transport, seek, mixer controls, master present/absent, device recovery, settings persistence, compact layout, Unicode/long names, minimize/restore, and close during playback.
- Added reproducible UI resource qualification using Slint's lazy repaint metrics. Idle and minimized states stop repainting and the process tree contains no browser runtime.
- Extended the existing NVDA harness with a configurable announcement set and qualified Phase 8 source, transport, gain, mute, solo, and error-recovery controls.

Decisions:

- The waveform-cache controls persist only the Phase 8 preference and size limit. Cache storage and eviction remain Phase 9 work.
- No latency mode was exposed because it has not been qualified.
- Window position is restored only when its saved origin belongs to a current monitor; otherwise Windows chooses a safe placement.
- Compact layouts scroll instead of overriding Windows scale or shrinking controls.

Verification:

| Check | Evidence | Result |
| --- | --- | --- |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Workspace regression | `cargo test --workspace -q` | PASS, 122 passed and 4 environment-gated tests ignored |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Production release build | `cargo build --release -q -p stemplayer` | PASS |
| Windows native UI automation | `scripts/qualify-phase8-windows-ui.ps1` | PASS at 225% Windows scale |
| Screen reader, core tasks | `scripts/qualify-windows-nvda.ps1` with the Phase 8 player and core announcement set | PASS, NVDA 2026.1.1 announced all 7 required controls |
| Screen reader, recovery | `scripts/qualify-windows-nvda.ps1` with `--ui-error` | PASS, NVDA announced all 4 recovery controls |
| Resource qualification | `scripts/measure-phase8-windows.ps1` | PASS |
| Settings corruption/migration | `stemplayer-platform` settings tests | PASS |
| Real controller wiring | `stemplayer` controller tests | PASS, including load/seek/stop, mixer reset, device-rate change, and bounded shutdown |

Resource evidence:

| Scenario | Steady max private bytes | Average CPU, one core | Repaints per second |
| --- | ---: | ---: | ---: |
| Ready idle | 14,200,832 | 0% | 0 |
| Playing UI with deterministic mock | 14,245,888 | 0.298% | 4 |
| Playing minimized | 14,114,816 | 0% | 0 |
| Paused minimized | 14,188,544 | 0% | 0 |

Artifacts:

- `artifacts/phase8/windows/ui-qualification.json`
- `artifacts/phase8/windows/resource-report.json`
- `artifacts/phase8/windows/compact.png`
- `artifacts/phase8/windows/large-text-unicode.png`
- `artifacts/phase8/windows-nvda-core/nvda-qualification.json`
- `artifacts/phase8/windows-nvda-error/nvda-qualification.json`

Exit-gate conclusion:

- Windows local playback and mixer tasks are available through a native, keyboard-operable, screen-reader-exposed UI.
- Error recovery and preferences are bounded and safe.
- Idle/minimized repaint, CPU, memory, and process-tree gates pass.
- Phase 8 passes. Phase 9 may begin.
