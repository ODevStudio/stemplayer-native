# Phase 2 — Native UI Shell, Controller Boundary, and Headless Harness

## Progress

- Status: exit gate passed (2026-07-22).
- Source of truth and Phase 1 contracts reviewed before implementation.
- Scope is limited to `stemplayer-native`; the webplayer and converter remain untouched.

## Goal

Create the fully native application shell and prove that UI lifecycle, command dispatch, snapshot subscription, accessibility basics, and event-driven rendering work without a browser or real audio engine. In parallel, complete the headless harness used by subsequent phases.

## Boundary

The UI is newly implemented. It may reproduce user-facing concepts from the webplayer, but it does not embed the webplayer, render its HTML, import its React components, or require its build artifacts.

## Entry criteria

- Phase 1 controller, command, snapshot, error, mixer-state, and transport-state contracts compile.
- Phase 0 UI toolkit gate passed.
- Initial UX behavior matrix exists.

## Work package 2.1 — Application lifecycle

Implement:

- Single main application process.
- Main window creation and clean shutdown.
- Controller startup and shutdown handshake.
- Safe behavior when shutdown occurs during open, analysis, seek, or playback commands.
- Platform-correct application data, cache, and configuration directories through `stemplayer-platform`.
- Local settings load with schema versioning and corrupt-settings fallback.
- No automatic network access.

The application must close promptly even when a mock operation is intentionally stalled.

## Work package 2.2 — Presentation model

Create a UI-facing model derived from `PlayerSnapshot`:

- Transport availability.
- Current and total time.
- Mixer row values.
- Effective output mode.
- Buffering and error banners.
- File display name.
- Source and sample-rate summary.
- Device display name.
- Waveform placeholder state.
- Diagnostics visibility.

The presentation model performs formatting and interpolation but does not own playback truth. It must be deterministic and unit tested.

## Work package 2.3 — Command adapter

Map UI actions into typed commands:

- Open source.
- Play, pause, stop.
- Seek.
- Gain, mute, solo.
- Reset.
- Output preference.
- Select output device.
- Clear cache.
- Close application.

Use a bounded command path. Repeated slider movements should coalesce rather than create an unbounded queue. Seek dragging uses preview state and commits a bounded number of seeks.

## Work package 2.4 — Mock engine

Implement a deterministic fake controller that simulates:

- Load progression.
- Ready, play, pause, seek, end, and error states.
- A 10-minute timeline.
- Master present and absent.
- Buffering diagnostics.
- Device changes.
- Waveform availability.

The fake must use a virtual clock in tests so UI behavior is deterministic. It must not be included in production builds except behind an explicit development feature.

## Work package 2.5 — Minimal native UI

Build a functional but visually restrained shell:

- File/open area.
- Transport controls.
- Time display.
- Four stem rows.
- Master/stem indicator.
- Waveform placeholder and seek surface.
- Output-device selector placeholder.
- Collapsible diagnostics.
- Error dialog/banner.

Do not spend this phase on visual polish. Focus on state correctness, focus order, keyboard operation, scaling, and low idle resource use.

## Work package 2.6 — Event-driven playhead

The controller publishes authoritative position snapshots at a bounded rate. While playing, the UI interpolates the visible cursor using a monotonic clock and the source sample rate. It corrects to new authoritative snapshots without visibly moving backward unless an explicit seek occurred.

Requirements:

- No audio-buffer-level UI messages.
- No permanent 60 Hz engine event stream.
- UI repaint requested only while the playhead is visibly moving.
- Repaint stops immediately when paused, ended, hidden, or minimized.
- Diagnostics update at a lower rate than the cursor.

## Work package 2.7 — Accessibility and keyboard baseline

Provide:

- Accessible names and roles.
- Value text for gain controls.
- State text for mute, solo, transport, and output mode.
- Logical focus order.
- Visible focus indicator.
- Spacebar play/pause only when it does not conflict with an active control.
- Dedicated shortcuts for transport, seeking, mute/solo navigation, reset, open, and diagnostics.
- Windows UI Automation and screen-reader exposure smoke tests. Native macOS/Linux screen-reader task tests are optional Phase 12 platform-expansion evidence.

## Work package 2.8 — Drag-and-drop and dialogs abstraction

Define platform traits for:

- Open a single stem container.
- Open a Studio bundle manifest plus related files or a folder.
- Receive dropped files/folders.
- Show a location in the system file manager.
- Confirm destructive cache actions.

Use mock implementations in UI tests. Actual source validation belongs to later format phases.

## Work package 2.9 — Headless harness completion

Expand `stemplayer-probe` with a machine-readable JSON output schema and commands that later phases will fill:

```text
inspect
validate
render
playback-sim
benchmark
cache-inspect
hls-validate
```

Rules:

- UTF-8 JSON Lines for streaming diagnostics is acceptable in the probe only; the production UI/controller uses typed Rust values.
- Paths are accepted only as explicit command arguments.
- Exit codes distinguish usage, validation, unsupported feature, and internal failure.
- Output order is deterministic.

## Deliverables

- Native application window with mock playback behavior.
- Presentation-model crate and tests.
- Bounded UI command adapter.
- Virtual-clock fake engine.
- Event-driven playhead interpolation.
- Accessibility and keyboard baseline.
- Dialog/drag-and-drop traits.
- Completed headless harness command surface.
- UI lifecycle and resource report.

## Tests and evidence

- Automated UI tests for every transport state and command enablement rule.
- Keyboard-only operation test.
- Focus-order snapshot test.
- Slider command coalescing test.
- Seek-drag throttling test.
- Shutdown-during-operation test.
- Minimize/restore and hidden-window repaint test.
- Screen-reader smoke-test record.
- Headless JSON schema golden tests.

## Resource checks

- Idle private memory against the Phase 0 baseline.
- No continuous CPU use while the mock player is paused.
- No application-owned timer firing while the window is minimized.
- Bounded command and snapshot queues under synthetic event floods.
- UI response remains interactive while mock operations stall.

## Risks and mitigations

- **Immediate-mode or renderer loop burns CPU:** enforce repaint-on-demand and measure minimized behavior.
- **UI becomes coupled to fake assumptions:** all UI state comes from public snapshots and capabilities.
- **Slider floods controller:** coalesce by role and command type.
- **Accessibility deferred until late:** require baseline evidence before real-engine integration.

## Exit gate

Phase 2 is complete when the Windows native application can exercise the complete user interaction model against a fake engine, consumes no browser technology, remains idle when not playing, and the headless harness is ready to drive real format and engine implementations. Native macOS/Linux UI runtime qualification is optional Phase 12 work.

## Phase 2 execution record - 2026-07-22

Status: exit gate passed.

Implementation remained limited to `stemplayer-native/`, plus the required phase-status records. The existing webplayer and converter were not modified or referenced at build or runtime.

### Deliverables

- Native Slint application shell with open/drop, transport, seek, four mixer rows, output-device placeholder, diagnostics, error presentation, and clean shutdown.
- Deterministic presentation model covering every empty and loaded transport state.
- Bounded command adapter with per-role gain coalescing and one seek commit per drag.
- Development-feature-only mock controller with virtual time, ten-minute duration, master presence, buffering, device, waveform, error, and stalled-operation behavior.
- Monotonic playhead interpolation at 30 Hz from authoritative mock snapshots published at 4 Hz.
- Cursor and snapshot timers disabled while paused, ended, or minimized.
- Platform-correct data, cache, and configuration paths; versioned local settings load and corrupt fallback.
- Native dialog and drop boundaries without source validation from later phases.
- Keyboard shortcuts, deterministic focus order, accessible names and values, UI Automation exposure, and NVDA announcements.
- Complete probe surface for `inspect`, `validate`, `render`, `playback-sim`, `benchmark`, `cache-inspect`, and `hls-validate` with deterministic JSON and distinct exit classes.

### Decisions

- Reused standard-library bounded synchronization and the pinned Slint dependency; no async runtime or new third-party dependency was added.
- Kept the mock engine behind the explicit `development` feature. Minimal and production feature builds exclude it.
- Kept future probe operations explicitly unsupported instead of creating Phase 3-10 implementations early.
- Used native environment conventions for application paths instead of adding a directory helper dependency.

### Exit-gate evidence

| Gate | Evidence | Result |
| --- | --- | --- |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Lint | `cargo clippy --locked --all-targets --all-features -- -D warnings` | PASS |
| Windows tests | `cargo test --locked --all-features` | PASS, 40 tests |
| Feature boundary | `cargo check --locked --no-default-features` and all-feature check | PASS |
| Native smoke | `stemplayer --ui-smoke 3000` | PASS, clean exit |
| Keyboard and accessibility | `scripts/qualify-phase2-windows-ui.ps1` | PASS, 20 focusable controls; open, play, pause, diagnostics, mute, minimize, and restore exercised |
| Screen reader | NVDA 2026.1.1 qualification | PASS, all six required announcements present |
| Resource discipline | `scripts/measure-phase2-windows.ps1 -Seconds 4` | PASS |
| Ubuntu headless | `scripts/qualify-wsl.ps1 -OutputDirectory artifacts/phase2/wsl-ubuntu` | PASS |
| Dependency direction | `cargo tree --locked -p stemplayer-engine` | PASS, no UI or platform dependency |
| Forbidden coupling | exact Rust, Cargo, and Slint scan | PASS, zero web, Node.js, Python, FFmpeg, WebView, Electron, Tauri, or AudioWorklet references |

### Resource evidence

Windows 11 Pro 10.0.26200, Ryzen 7 5800X3D, 32 GiB RAM, x64, Slint software renderer, release development build:

| Scenario | Steady max private bytes | Steady normalized CPU |
| --- | ---: | ---: |
| Ready idle | 13,598,720 | 0% |
| Playing | 13,533,184 | 0.0325% |
| Playing minimized | 13,582,336 | 0% |

Machine-readable evidence:

- `stemplayer-native/artifacts/phase2/windows/resource-report.json`
- `stemplayer-native/artifacts/phase2/windows/ui-qualification.json`
- `stemplayer-native/artifacts/phase2/windows-nvda/nvda-qualification.json`
- `stemplayer-native/artifacts/phase2/wsl-ubuntu/headless.json`

### Limitations

- GPU allocations are not included in the private-memory report. The selected software renderer does not create a separate GPU process.
- External ETW wakeup traces were not collected. Timer bindings stop when paused or minimized, and steady measured CPU was 0% in both states.
- The NVDA smoke proves named focus announcements, not a blind-user usability study.
- Real decoding, audio output, and source validation remain intentionally unimplemented for later phases.
