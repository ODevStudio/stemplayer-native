# Phase 11 — Operating-System Integration, Packaging, Signing, and Installation

## Goal

Deliver native desktop behavior and easy installation on Windows. Add file associations, single-instance routing, media controls, power/device lifecycle integration, secure settings paths, signed packages, dependency notices, and optional signed updates without bundling browser or converter runtimes. macOS and Linux packaging are optional Phase 12 platform expansions.

## Entry criteria

- Local UI, playback, mixer, waveform, and diagnostics phases are complete.
- HLS is complete if the targeted release promises HLS parity.
- Supported OS/architecture matrix is final.
- Application identifier, publisher identity, icons, and versioning policy are approved.
- Signing/notarization credentials and secure CI secret handling are available.

## Work package 11.1 — Application identity and versioning

Define:

- Stable application identifier per platform.
- Semantic versioning policy.
- Build metadata policy.
- Config/cache schema versions independent of application version.
- Release channel naming.
- Window/class identifiers.
- Protocol handler only if justified; avoid unnecessary URL schemes.

The native player name must not imply it contains the converter.

## Work package 11.2 — Single-instance and open routing

Implement one primary application instance per user session unless platform conventions require otherwise:

- A second launch forwards explicitly provided files/URLs to the primary instance.
- Inputs are revalidated in the primary process.
- Requests are bounded and authenticated to the local user context.
- Opening a new source while another plays follows an explicit confirmation or replacement policy.
- Malformed IPC cannot crash the player.

Do not create a general command-execution IPC surface.

## Work package 11.3 — File associations

Register only specific player formats:

- `.stem.m4a`
- `.stem.mp4`
- A dedicated Studio bundle extension if one exists in the approved format roadmap.

Do not associate generic `manifest.json`, `.m4a`, `.mp4`, or `.wav` globally. Studio manifests remain available through Open Folder/Open Manifest unless a dedicated extension is introduced.

File association launch must preserve paths exactly and pass them through native validation.

## Work package 11.4 — Media controls

Integrate platform media controls:

- Play/pause.
- Stop where supported.
- Seek where supported and reliable.
- Position/duration updates at a bounded rate.
- Track display name without leaking full paths.
- Artwork absent unless explicitly available and safe.

Platform targets:

- Windows system media transport controls.
- macOS media command center.
- Linux MPRIS.

Media commands map to the same bounded controller interface as the UI.

## Work package 11.5 — Power and session lifecycle

Handle:

- System suspend.
- Resume.
- User session lock/unlock where relevant.
- Audio device removal during suspend.
- Display sleep.
- Shutdown/logoff notification.

Policy must define whether playback pauses before suspend and whether it resumes automatically. Conservative default: do not resume audible playback automatically after a disruptive lifecycle event without confirmed prior state and product approval.

## Work package 11.6 — Platform directories and permissions

Use platform-standard locations for:

- Configuration.
- Cache.
- Logs when enabled.
- Crash records when enabled.
- Update downloads.

The application should run as a normal user. Installation should not require permanent administrator privileges for ordinary per-user scenarios. Media files remain read-only unless the user explicitly exports diagnostics elsewhere.

## Work package 11.7 — Windows package

Produce a signed installer with:

- Per-user default installation.
- Start menu integration.
- Clean uninstall.
- File associations.
- Upgrade and downgrade policy.
- No WebView bootstrapper.
- No Python, Node.js, FFmpeg, or converter payload.
- Architecture-specific package or approved multi-architecture strategy.

Test install, repair/upgrade if supported, uninstall, path with Unicode, non-admin account, and locked-file behavior.

## Work package 11.8 — Optional macOS package

Produce:

- Native `.app` bundle.
- Signed and notarized distribution artifact.
- DMG or approved installer format.
- Correct entitlements with minimum privilege.
- File association declarations.
- Apple silicon build; universal or Intel build according to Phase 0 decision.

Test Gatekeeper first launch, quarantine behavior, upgrade, uninstall guidance, device permissions, and app translocation-sensitive paths.

## Work package 11.9 — Optional Linux packages

Produce the approved set, normally:

- `.deb`
- `.rpm`
- AppImage or Flatpak as a portable/managed option

Document runtime audio and graphics dependencies. Test at least the chosen PipeWire and fallback environments. Desktop file, icons, MIME registrations, MPRIS, uninstall, and sandbox limitations must be qualified.

Do not label a package “portable” if it writes outside documented user directories.

## Work package 11.10 — Signing and supply chain

- Sign release commits/tags according to policy.
- Build in controlled CI.
- Pin dependencies and toolchain.
- Generate checksums.
- Generate dependency license notices.
- Generate SBOM.
- Scan artifacts for accidentally bundled secrets, paths, private fixtures, browser engines, converter binaries, and debug symbols.
- Retain provenance and build logs.

Reproducible builds are a goal; where platform signing prevents bit-for-bit reproducibility, preserve reproducible unsigned intermediates and documented signing steps.

## Work package 11.11 — Update strategy

Automatic updates are optional for the first release. If enabled:

- Manifest and packages are signed.
- HTTPS is mandatory.
- Channel is explicit.
- Download is bounded and resumable only if safely implemented.
- Update is staged outside the running binary.
- Rollback or failed-update recovery is documented.
- No update check occurs when disabled.
- Update service starts lazily.

Never enable unsigned self-update.

## Work package 11.12 — Release configuration

Release builds:

- Disable development fake engine.
- Disable verbose path logging.
- Strip or separate symbols according to crash policy.
- Use panic strategy and unwind/abort decision from architecture review.
- Enable only promised source features.
- Include version and diagnostic build identifiers.
- Preserve local error reporting without network telemetry.

## Deliverables

- Platform integration crate implementations.
- Single-instance open routing.
- Safe file associations.
- Media controls.
- Suspend/resume and device lifecycle behavior.
- Signed Windows installer.
- Optional signed/notarized macOS artifact when the Phase 12 macOS expansion is selected.
- Optional Linux packages when the Phase 12 Linux expansion is selected.
- SBOM, licenses, checksums, and release provenance.
- Optional signed updater.
- Install/uninstall qualification reports.

## Tests and evidence

- Fresh install, upgrade, and uninstall on clean systems.
- Standard-user installation.
- File association open and malicious argument cases.
- Second-instance routing flood and malformed local IPC.
- Media key behavior.
- Suspend/resume while playing, paused, and seeking.
- Default-device change.
- Offline launch and update-disabled launch.
- Artifact content scan proving no browser/converter runtime.
- Windows signature verification.
- Optional macOS notarization and Linux dependency/desktop integration checks belong to their Phase 12 platform expansions.

## Resource checks

- Installed size by platform.
- First-launch and steady-state memory.
- Update and media-control services idle cost.
- No extra helper process remains running after player exit.
- No background process is installed for ordinary playback unless explicitly approved.

## Risks and mitigations

- **Packaging tool adds runtime baggage:** inspect final artifacts and process tree.
- **File association too broad:** register only dedicated stem formats.
- **Updater increases attack surface:** keep disabled until signing and rollback are proven.
- **Platform media APIs leak paths:** publish sanitized titles only.
- **Linux package fragmentation:** define a supported package matrix and test it, rather than claiming every distribution.

## Exit gate

Phase 11 is complete when clean Windows systems can install, upgrade, launch, open supported sources, use media controls, survive power/device lifecycle events, and uninstall the signed native player without browser or converter runtime baggage. macOS and Linux packages are optional Phase 12 platform-expansion deliverables and remain unpublished unless qualified.

## Implementation record

### 2026-07-28 - Playing seek cancellation recovery

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Prevented decode work canceled by a newer seek, stop, unload, or source-open command from being published as a player error when cancellation reaches the active playback pump.
- Applied the same cancellation policy to command handling and periodic pump, waveform-analysis, and snapshot work.
- Added a focused regression check that distinguishes expected canceled decode work from genuine decode and internal failures.

Verification:

- `cargo test --locked -p stemplayer -p stemplayer-engine -p stemplayer-ui`: PASS, 30 passed and 2 existing hardware-gated tests ignored.
- `cargo fmt --all -- --check`: PASS.
- Focused cancellation-publication regression: PASS.

### 2026-07-28 - Native UI state recovery and immediate mixer feedback

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Kept the native UI snapshot listener subscribed across quiet receive intervals so loading or playing another source after an ended or otherwise idle track updates the source metadata, transport, playhead, waveform, and mixer presentation.
- Made stem enabled toggles update their local checked state before submitting the mute command, so the first pointer, keyboard, or accessibility action is visible immediately.
- Derived each gain percentage directly from the live fader value instead of waiting for an engine snapshot.
- Added a focused regression check that publishes a snapshot only after repeated listener timeouts.

Verification:

- `cargo test --locked -p stemplayer -p stemplayer-ui`: PASS, 17 passed and 1 existing hardware-gated test ignored.
- `cargo fmt --all -- --check`: PASS.
- Focused listener regression: PASS after multiple 1 ms idle receive timeouts before the next snapshot.

### 2026-07-28 - Transport, coherent master gain, mixer controls, and window state

Status: blocked. Phase 11 entry blockers unchanged and the phase does not advance.

Implemented:

- Made interrupted waveform seeks retarget while the engine remains in Seeking, preserving whether successful completion resumes playing or remains paused.
- Changed automatic output selection to keep using the master reference when every audible stem gain is equal; the common gain now scales the master reference.
- Added an overall gain fader in master-reference mode that updates all four stem gains through the existing 16 ms command coalescer. A differing gain or disabled stem returns automatic output to the stem mix.
- Doubled stem gain-fader width, removed Solo controls and shortcuts, and replaced Mute buttons with colored stem-name enabled toggles. Enabled is the default audible state and uses a role-colored outline over the normal button background; disabled removes that outline and mutes the stem.
- Removed cache hit/miss detail from the visible Waveform ready status while retaining it in diagnostics.
- Added settings schema v3 migration, native maximized-window persistence, saved normal bounds, and a centered 90% monitor-size first launch.
- Requested a full Slint redraw when Windows focus returns, occlusion ends, or a restored window regains nonzero size, preventing stale or foreign backbuffer content after returning to the player.

Verification:

- cargo test --locked -p stemplayer -p stemplayer-ui -p stemplayer-platform -p stemplayer-engine -p stemplayer-mixer: PASS, 47 passed and 2 existing hardware-gated tests ignored.
- Native Slint development smoke with isolated settings: PASS. Fresh first launch persisted centered 1536x864 bounds at (192, 108), exactly 90% of the test desktop, and exited cleanly.
- Engine regression covers retargeting from interrupted Seeking while preserving playback state.
- Mixer regressions cover coherent equal-gain master selection and master-reference loudness scaling.
- Settings round trip covers the maximized state and older schema migration.
- Scoped git diff --check: PASS; only existing Windows line-ending warnings reported.

Remaining verification limitation:

- Windows UI Automation capture could not run because the Codex Windows sandbox helper failed during runtime initialization. Native Slint compilation, tests, and executable smoke passed.

### 2026-07-28 - Native gain fader correction

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Replaced all four custom rotary gain knobs with compact horizontal Slint Slider controls while preserving the existing gain-position, gain-changed(float), gain-commit(), enabled-state, and percentage-readout interfaces.
- Kept the Rust 0..1000 nonlinear gain conversion unchanged while mapping the native fader track linearly from 0% through centered 100% to 200%.
- Set the native keyboard step to 10 and retained native focus, arrow, Home/End, track-click, drag, and accessibility value behavior.
- Restored double-click reset to 100% through two matching native release events within 300 ms, without adding a pointer overlay or custom drag handling.
- Widened the mixer-control area to 194px, retained the mute and solo button behavior and dimensions, and moved waveform rows, click-to-seek bounds, and playhead projection to the same offset.
- Removed the knob painting, vertical-drag calculation, wheel handling, and custom focus/key handling. Rust gain conversion and 16 ms command coalescing remain unchanged.

Verification:

- cargo build --locked -p stemplayer: PASS.
- cargo test --locked -p stemplayer: PASS, 8 passed and 1 hardware-gated test ignored.
- cargo test --locked -p stemplayer-ui: PASS, 8 passed, including gain-scale and command-coalescing coverage.
- Windows UI Automation at 216 DPI: PASS. Four faders exposed range 0..1000 with step 10; 0%, 100%, and 200% landed at 0, 500, and 1000; hover left gain unchanged; track click changed gain; a second matching release reset the fader to 100%; horizontal dragging beyond the thumb reached the maximum; arrows, Home, End, and accessibility value actions worked.
- Rapid 25-step dragging left the player responsive, the mixer Reset command restored all four faders to 100%, and waveform click-to-seek moved from 0:15 to 5:59 on the ten-minute mock source.
- Normal and minimum 560x400 window captures at 216 DPI: PASS. Faders, mute/solo buttons, waveform rows, click bounds, and playhead remained aligned without overlap.
### 2026-07-28 - Waveform deck presentation correction

Status: blocked. Phase 11 entry blockers are unchanged.

Implemented:

- Replaced the continuously updated playback-position slider with a viewport-projected vertical playhead drawn across all four waveform rows.
- Corrected stale playback snapshots so the interpolated playhead preserves its visible frame instead of stepping backward when the UI clock is ahead of the latest engine frame.
- Moved Windows container, manifest, and folder dialogs off Slint's UI callback onto fresh worker threads so the Common Item Dialog receives a clean STA context and does not leave its breadcrumb and directory list out of sync.
- Added click-to-seek directly on the waveform deck while preserving zoom, fit, viewport scrolling, keyboard seeking, and accessibility value actions.
- Integrated each stem's gain, mute, and solo controls beside its waveform. Gain now uses a compact rotary control with drag, wheel, keyboard, accessibility value, and double-click reset to 100 percent.
- Increased usable waveform height, retained the four native min/max peak projections, added center and time-grid lines, and used the existing stem colors.
- Kept waveform and mixer presentation in a focused Slint component file and made the operational layout fill the window while remaining top-aligned.

Verification:

- `cargo build --locked -p stemplayer`: PASS.
- `cargo test --locked -p stemplayer`: PASS, 8 passed and 1 hardware-gated test ignored.
- `cargo test --locked -p stemplayer-ui`: PASS, including the stale-snapshot monotonic playhead regression check.
- DPI-aware Windows capture with the remembered six-minute stem container: PASS. Four filled waveforms, the shared playhead, compact gain controls, transport density, and loaded-source layout were visually inspected.
- The rebuilt GUI was launched with real playback and left running for operator inspection.

### 2026-07-28 - Entry gate blocked

Status: blocked.

Completed inspection:

- Confirmed Phases 8, 9, and 10 are complete and the Windows x86-64 release scope is approved.
- Confirmed the platform crate already provides bounded settings and platform-standard per-user config, cache, and data paths.
- Confirmed the product build has no updater and performs no startup network access, matching the approved first-release security default.
- Confirmed no approved application identifier, publisher identity, or release icon set is recorded in the native workspace or phase decisions.
- Confirmed no signing credential is available in the environment and no Windows signing or installer tool (`signtool`, WiX, or `cargo-wix`) is installed. Only the .NET SDK is present.

Blockers:

- Work package 11.1 cannot be completed without owner-approved product identity, publisher identity, application identifier, and icon artwork.
- Work packages 11.7 and 11.10, and therefore the exit gate, cannot pass without an available code-signing certificate, secure credential handling policy, and Windows installer/signing toolchain.
- The clean-system signed install, upgrade, signature verification, and uninstall qualification required by the exit gate cannot be performed until those inputs are supplied.

Unblock conditions:

- Record the approved display name, reverse-DNS application identifier, publisher string, semantic-version/release-channel policy, and Windows `.ico` asset.
- Provide a release-appropriate Windows code-signing certificate through an approved secret mechanism and identify its timestamp service.
- Install or approve the Windows packaging toolchain and provide clean Windows qualification environments, including a standard-user account.

No implementation was started because these are explicit Phase 11 entry criteria and inventing identity or producing an unsigned substitute would violate the security and exit-gate requirements.
### 2026-07-28 - Windows software-renderer restore correction

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Reset Slint's software-renderer partial repaint cache before redrawing when the Windows player regains focus or leaves occlusion, preventing stale or foreign backbuffer pixels after content changes while hidden.

Verification:

- `cargo test --locked -p stemplayer -p stemplayer-ui`: PASS, 18 passed and 1 existing hardware-gated test ignored.
- `cargo fmt --all -- --check`: PASS.
- `cargo build --locked -p stemplayer`: PASS after the previous inspection instance released the executable.
- Native Windows restore smoke: PASS across five minimize/restore cycles; the process remained responsive and a post-restore `PrintWindow` capture contained one complete, coherent Stemplayer frame without foreign pixels.

### 2026-07-28 - Static waveform presentation cost correction

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Removed the waveform zoom and viewport row and fixed waveform presentation to the complete source timeline.
- Stopped rebuilding all eight Slint waveform models for unchanged playback snapshots after waveform analysis completes.

Verification:

- `cargo test --locked -p stemplayer`: PASS, 10 passed and 1 existing hardware-gated test ignored.
- `cargo fmt --all -- --check`: PASS after formatting.
- Scoped raw `git diff --check`: PASS; only existing Windows line-ending warnings reported.
- Corrected release playback after a 12-second warm-up: 0.263% total CPU over 10 seconds on 16 logical processors; the previously observed process was an older debug executable without these corrections.

### 2026-07-28 - Playlist and symmetric player chrome

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Replaced the three source buttons with folder and cloud icon controls; the folder path accepts supported container files, Studio JSON manifests, and inferred role-suffixed WAV bundles.
- Added bounded non-recursive folder discovery, coalesced multi-file drag and drop, manifest de-duplication, and the `track.drums.wav`, `track.bass.wav`, `track.other.wav`, and `track.vocals.wav` naming pattern.
- Added the bottom-aligned collapsible track list, selection, double-click playback, and automatic activation of the next item after playback ends.
- Reorganized source buttons, transport controls, elapsed time, source title, and settings into one toolbar band with optically centered text, symmetric visible spacing above and below, and 12 px horizontal margins.
- Replaced visible transport state, output-mode text, Stop, and waveform zoom controls with the requested compact controls and fixed 100% waveform view.
- Added a compact right-aligned settings panel for output, theme, diagnostics, recent sources, and cache controls, with scrollable 40 px input rows that retain their text height at the minimum window size.
- Used Slint's native Button for the three-dot settings trigger so a single pointer click opens the panel.

Security and limits:

- Folder scans remain non-recursive, reject symbolic-link inputs, stop after 4,096 files, and return at most 512 playlist items.
- JSON manifests retain the format crate's bounded reader and maximum manifest size.

Verification:

- `cargo fmt --all -- --check`: PASS.
- `cargo test --locked -p stemplayer -p stemplayer-format -p stemplayer-platform`: PASS, 41 passed and 2 hardware-gated tests ignored.
- `cargo build --locked --release -p stemplayer`: PASS.
- Raw `git diff --check`: PASS; only existing Windows line-ending warnings reported.
- Native Windows capture at normal and maximized sizes: PASS. The toolbar groups remain symmetric, the settings panel aligns to the toolbar's right margin, and the playlist remains below the waveform deck.

### 2026-07-28 - Source picker and compact window correction

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Removed the unnecessary whole-window scroll container and replaced the shared icon control's manual focus and pointer handling with Slint's native button dispatch.
- Restored supported multi-file selection from the folder icon; folders navigate in the native picker and selected files load into the bounded playlist.
- Bound the minimum window height to the actual collapsed or expanded playlist height.
- Hid the output-mode selector and retained the existing Automatic default.
- Anchored the compact settings panel to the left of the menu button and reduced its maximum height to the remaining controls.
- Replaced separate Play and Pause controls with one state-aware transport button and removed the full-width focus target that consumed the first stem-toggle click.

Verification:

- `cargo test --locked -p stemplayer`: PASS, 11 passed and 1 hardware-gated test ignored.
- `cargo build --locked --release -p stemplayer`: PASS.
- Native Windows automation: PASS. Settings, playlist collapse, and the file picker each responded to one click; the picker displayed supported files and the collapsed playlist removed its item rows.
- Native Windows capture: PASS. The reduced settings panel opens beside the menu button, keeps the menu visible, and fits its remaining controls without blank vertical space.
- Native playback verification: PASS. The transport shows Pause while playing, and the stem role controls respond on the first click.

### 2026-07-30 - Initial playlist playback correction

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Marked the first picker-loaded playlist item active when it is opened, so pressing Play reuses the loaded session and completed waveform instead of reopening and analyzing the same source.

Verification:

- `cargo test --locked -p stemplayer`: PASS, 12 passed and 1 existing hardware-gated test ignored.
- `cargo fmt --all -- --check`: PASS.
- Corrected release build in `target/release-next/release/stemplayer.exe`: PASS.
- The normal release path could not be replaced because the previous executable remained open as PID 30068.

### 2026-07-30 - Flexible playlist and remembered collapse sizing

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Made the expanded playlist consume the window's remaining vertical space instead of stopping at 190 px.
- Kept the native vertical scrollbar visible and disabled the unnecessary horizontal scrollbar.
- Remembered separate expanded and collapsed heights during the session; collapse now resizes to the compact content height and expand restores the user's last expanded height.
- Left maximized window dimensions unchanged when toggling the playlist.
- Preserved the expanded height in settings when the application closes while collapsed or maximized.

Verification:

- `cargo test --locked -p stemplayer`: PASS, 13 passed and 1 existing hardware-gated test ignored.
- Corrected release build in `target/release-next/release/stemplayer.exe`: PASS.

### 2026-07-30 - Monotonic visible playhead correction

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Stopped periodic playback snapshots from overwriting the interpolated visible playhead with an older authoritative position.
- Retained waveform analysis during playback, including the existing low-watermark throttling only when the playback decode buffer is under pressure.
- Left process and audio-thread priority unchanged because the duplicate UI position writer was the confirmed playhead regression.

Verification:

- `cargo test --locked -p stemplayer`: PASS, 13 passed and 1 existing hardware-gated test ignored.
- Scoped `rustfmt --edition 2024 --check`: PASS; the workspace-wide check is blocked by unrelated concurrent edits in `crates/stemplayer-decode/tests/audio.rs`.
- Corrected release build in `target/release-next/release/stemplayer.exe`: PASS.

### 2026-07-30 - Ordinary audio playback and persistent playlist

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Enabled Symphonia MP3, FLAC, WAV/PCM, Ogg/Vorbis, AAC-LC, and ALAC support without adding another decoder library.
- Added LocalAudioFile source metadata, single-track mono/stereo validation, 8-96 kHz bounds, known-duration enforcement, mono duplication, generic packet seeking, and file-identity checks while retaining strict Stem MP4 validation.
- Kept .mp4 stem-only and made .m4a prefer a complete stem container before accepting a single ordinary audio track.
- Routed ordinary audio through the first existing stereo stem lane and filled the other three lanes with silence, leaving the real-time ring, mixer, resampler, and waveform cache formats unchanged.
- Added ordinary file discovery and picker support while preserving complete role-suffixed WAV grouping and excluding incomplete role-suffixed WAV sets.
- Added the ordinary-audio deck with a 64 px vertical 0-200% volume fader and one waveform filling the remaining deck area.
- Added pointer, drag, keyboard, accessibility value, double-click reset, full-region playhead, seek, hidden stem controls, and the Reset volume action.
- Migrated settings schema 3 to 4 with bounded ordered playlist sources and the selected source.
- Persisted playlist replacement, selection, activation, and auto-advance through serialized updates and transactional temporary-file replacement.
- Restored valid entries in order, skipped unavailable entries, de-duplicated paths, reopened the selected or first valid entry at frame zero in Ready, started waveform analysis, and did not autoplay.
- Added original generated tone fixtures for each supported codec plus mono, stereo, corrupt, multichannel, and multi-track cases.

Security and limits:

- Ordinary sources reject symbolic links through the existing bounded source reader, reject unsupported codecs, ambiguous tracks, multichannel layouts, missing or overflowing durations, out-of-range sample rates, cancellation, and changed files.
- Playlist discovery remains non-recursive with the existing 4,096-file scan limit and 512-item result limit.
- Serialized settings remain capped at 64 KiB. Oversize or failed writes preserve the previous settings file.
- Generic audio file associations remain unchanged.

Verification:

- cargo test --locked --workspace: PASS, 161 passed and 4 hardware-gated tests ignored across 31 suites.
- Focused decoder tests: PASS for all seven supported fixture variants, repeated and end-bound seeks, deterministic adjacent decode, finite output, mono duplication, stereo preservation, cancellation, malformed input, multichannel, multi-track, known-duration bounds, and file replacement.
- cargo fmt --all -- --check: PASS.
- cargo build --locked --release -p stemplayer: PASS.
- Native Windows release playback smokes: PASS for mono WAV, stereo float WAV, MP3, FLAC, Ogg Vorbis, M4A AAC-LC, and M4A ALAC; every snapshot reported local-audio-file, finite mixer output, and ready waveform analysis.
- Isolated restart smoke: PASS. The saved selected MP3 reopened with Play visible and Pause absent; no autoplay occurred.
- Windows UI Automation: PASS. Volume, Reset volume, and Play were exposed; no stem toggles were exposed. Accessibility set 150%, Home reached 0%, End reached 200%, Arrow Down changed the value, pointer positioning reached 150%, and double-click reset to 100%.
- Layout checks: PASS at 780 x 620 and maximized 3840 x 2109 with 150% UI scaling. The volume strip measured 64 x 168 logical pixels and 96 x 252 scaled pixels.
- Isolated app-window capture: stemplayer-native/artifacts/phase11-ordinary-audio-window.png, 1783 x 1474, nonblank with 107 sampled colors.
- cargo clippy --locked --workspace --all-targets -- -D warnings: PASS with no issues.
- Final isolated release fallback smoke: PASS. A corrupt saved first entry was removed, the next valid MP3 reopened, Play was visible, and Pause was absent.

### 2026-07-30 - Frequency-colored ordinary waveform

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Replaced ordinary-audio peak height with bounded per-bucket RMS height so limited masters retain visible dynamics instead of filling the deck.
- Added low, mid, and high energy analysis at 250 Hz and 4 kHz crossovers using the existing waveform lanes and cache layout.
- Colored each ordinary waveform column from its band balance: lows red, mids blue/purple, and highs cyan/green.
- Invalidated only old ordinary-audio waveform cache keys; stem waveform analysis, rendering, and cache records remain unchanged.

Verification:

- `cargo test --locked --workspace`: PASS, 162 passed and 4 hardware-gated tests ignored across 31 suites.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS with no issues.
- `cargo build --locked --release -p stemplayer`: PASS.
- Native Windows capture: PASS. The restored ordinary track displayed changing cyan and purple frequency regions with RMS-shaped dynamics in `stemplayer-native/artifacts/phase11-frequency-waveform.png`.

### 2026-07-30 - Ordinary waveform analysis performance

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Reduced progressive waveform publications from one per 4,096 decoded frames to at most 100 per track plus the final snapshot.
- Replaced the four ordinary-audio waveform resolutions with one duration-scaled level containing at most 1,024 columns. Stem waveforms retain their four-level peak pyramid.
- Extended the existing waveform cache record to accept one-level ordinary waveforms while preserving existing four-level stem records and bounded validation.
- Removed the 25 ms worker sleep while nonplaying analysis is active. Playback retains the existing cadence, low-watermark priority, streaming decode blocks, and bounded memory behavior.
- Invalidated only prior ordinary-audio waveform cache keys. No image cache or decoder dependency was added.

Verification:

- `cargo test --locked --workspace`: PASS, 163 passed and 4 hardware-gated tests ignored across 31 suites.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS with no issues.
- `cargo fmt --all -- --check`: PASS.
- `cargo build --locked --release -p stemplayer`: PASS.
- Native Windows paused-analysis benchmark on the same 120-second MP3: previous release 42,822 ms wall time, 875 ms CPU, and 878,864 cache bytes; optimized release 490.9 ms wall time, 359.4 ms CPU, and 32,876 cache bytes.
- Native Windows 8:32 MP3 benchmark: optimized release completed analysis and cache publication in 2,296 ms wall time and 1,031.2 ms CPU with a 32,876-byte cache record.
- Raster image caching remains deferred because the compact one-level data removed the measured analysis, cache-size, and redraw pressure without sacrificing DPI-independent rendering.

### 2026-07-30 - Paused playlist cache analysis

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Replaced the four-level local stem waveform pyramid with the same duration-scaled level of at most 1,024 columns used by ordinary audio.
- Made legacy four-level cache records disposable so playlist analysis replaces oversized stem records with the bounded single-level format as each source is checked.
- Added a bounded playlist analysis queue for the existing 512 local-item limit. It completes the open source first, skips cache hits, analyzes remaining misses while Ready, Paused, Stopped, or Ended, and stops background work while playback is active.
- Kept background inspection, decoding, file-identity validation, cache checks, and writes separate from the selected source and foreground playback engine.
- Added aggregate playlist cache progress from 0% through 99% at the right edge of the playlist header. The percentage disappears after every item is cached or safely skipped, and remains hidden when waveform caching is disabled.
- Reset the queue after playlist replacement, cache enablement, and cache clearing. Missing, changed, malformed, or failed background sources are skipped individually without blocking playback or the remaining queue.

Verification:

- `cargo test --locked --workspace`: PASS, 164 passed and 4 hardware-gated tests ignored across 31 suites.
- Focused paused-playlist test: PASS. A second uncached stem bundle was analyzed without activation and reopened as an immediate waveform-cache hit.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS with no issues.
- Native Slint compilation and release build in `target/release-next/release/stemplayer.exe`: PASS.
- Native Windows isolated playlist smoke: PASS. The two-item playlist restored without autoplay, completed its cache queue, retained the selected title and waveform, and removed the completed percentage from the playlist header.

### 2026-07-31 - Playlist append, multi-selection, and removal

Status: blocked. Phase 11 entry blockers are unchanged and the phase does not advance.

Implemented:

- Changed picker and drop discovery to append through one bounded playlist path, preserve the existing order, selection, active item, and playback, de-duplicate paths, and retain the 512-item limit.
- Made the first item added to an empty playlist load without autoplay.
- Added central click, Control-click, Shift-click, Control-Shift-click, select-all, context selection, activation, and removal operations with focus and anchor tracking.
- Added `PlaylistRow` presentation state, focused playlist keyboard handling, Control+A, Delete, exclusive double-click activation, and a Slint `PopupWindow` with Remove, outside-click closing, and Escape handling.
- Kept playback untouched when removing entries. Removing the active entry clears playlist focus and disables automatic advance until a manual double-click activates another entry; other removals reindex the active entry and select the next remaining row or previous last row.
- Persisted appended and reduced source order plus the focused source through the existing settings field without a schema migration or dependency change.

Verification:

- `cargo test --locked -p stemplayer`: PASS, 22 passed and 1 existing hardware-gated test ignored. Ten playlist tests cover append, de-duplication, the 512-item limit, unchanged active state, all requested selection modes, context selection, beginning/middle/end/all removal, active-index adjustment, and disabled auto-advance after active removal.
- `cargo clippy --locked -p stemplayer --all-targets -- -D warnings`: PASS with no issues.
- `cargo fmt --all -- --check`: PASS.
- `cargo build --locked --release -p stemplayer`: PASS in the normal `target/release` path.
- Isolated native Windows UI Automation: PASS for empty-playlist load without autoplay, repeated picker append, Control multi-selection, multi-row Delete, uninterrupted playback while appending and removing inactive rows, immediate settings persistence, and reduced-order restore without autoplay after restart.
- Right-click reached the context-selection callback and preserved the multi-selection in native automation. Slint `PopupWindow` does not expose its child button through Windows UI Automation, so direct automated activation of Remove remains an operator check; the popup, outside-click policy, Escape binding, and shared removal callback compile in the release build.
- Full raw `git diff --check`: PASS; only existing line-ending conversion warnings were reported.