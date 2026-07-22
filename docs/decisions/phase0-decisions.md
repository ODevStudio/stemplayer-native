# Phase 0 Decisions

Date: 2026-07-22. Status: Phase 0 decisions accepted; optional platform expansion remains Phase 12 work.

## Repository Boundary

Decision: the production project is a separate repository named `stemplayer-native`. This temporary `stemplayer-native/` directory is an independently buildable Cargo workspace used only to preserve Phase 0 evidence until that repository exists.

Mechanical rules:

- The Cargo workspace has no path dependency outside its directory.
- Its build scripts may read only native workspace files and Cargo inputs.
- Native CI changes use native paths only.
- No Node.js, pnpm, Python, FFmpeg, webplayer, or converter command is part of build/test/run.
- Copying the directory alone must preserve `cargo build --locked` and `cargo test --locked`.

A named product owner and release/security owner are required for the Phase 12 release decision, not for Phase 1 entry.

## Platform Support

Decision: accepted as the first release matrix.

| Platform | GA target | Audio policy | Graphics policy | Status |
| --- | --- | --- | --- | --- |
| Windows | Windows 11 24H2 or newer, x86-64 | WASAPI shared mode; exclusive mode out of scope | Slint winit/software | Required release platform |
| macOS | macOS 14 or newer, Apple silicon | CoreAudio | Slint winit/software, measured GPU renderer only if needed | Optional Phase 12 expansion |
| Linux | Ubuntu 24.04 LTS and Fedora 44, x86-64, glibc desktop | PipeWire first, PulseAudio second, ALSA fallback | X11 initially; Wayland after advisory-clean dependency refresh | Optional Phase 12 expansion |
| Windows ARM64 | Windows 11 | WASAPI | Same policy | Post-GA |
| macOS Intel | No initial promise | CoreAudio | Same policy | Post-GA |
| JACK | No initial promise | None | Not applicable | Post-GA |

No discrete GPU is required. The software renderer is a release requirement for VMs and incompatible GPUs, but it must meet text/accessibility correctness and idle budgets. Linux real-time priority elevation is best-effort; denial is diagnostic and cannot prevent playback. macOS and Linux cannot be advertised or packaged as supported unless their optional Phase 12 expansion passes.

## UI Toolkit

Decision: Slint 1.17.1 is adopted for Windows, pinned with winit, software rendering, accessibility, and without system tray. FemtoVG was measured separately and rejected. macOS/Linux adoption remains conditional on their optional Phase 12 platform-expansion evidence.

Rationale:

- It is Rust-native at runtime and provides Windows, macOS, Linux, OS accessibility integration, event-driven rendering, system color schemes, and software fallback. The Phase 0 graph enables X11 only; Wayland remains disabled until its advisory path is patched.
- The Phase 0 proof contains one window, four mixer rows, a waveform placeholder, native widgets with keyboard focus, accessible roles/labels, external drop capture, system theme use, and no permanent animation/timer while running normally.
- The royalty-free desktop license permits proprietary desktop distribution with attribution; release design must include the required About attribution or release webpage badge. GPLv3 remains an alternative only if the complete distributed application is compatible.

Renderer policy: use the software renderer as the initial Windows default. The measured FemtoVG proof exceeds the 50 MiB idle-private-memory budget and is not release-eligible. Keep renderer selection replaceable, never fall back to a WebView, and exclude Skia until a measured need justifies its larger footprint.

Windows evidence: the software-only proof used 11,218,944 private bytes and 27,738,112 resident bytes; `winit-femtovg` used 106,668,032 through 108,503,040 private bytes and fails the 50 MiB idle budget. Windows UI Automation exposed all required names and 15 keyboard-focusable elements at 225% DPI in dark mode. NVDA 2026.1.1 announced all four gain sliders plus Play and Stop. Visible, minimized, and restored samples reported zero normalized CPU/context switches; software rendering reported zero GPU allocation. Native macOS/Linux evidence is optional Phase 12 work.

Sources: [Slint desktop support](https://docs.slint.dev/latest/docs/slint/guide/platforms/desktop/), [renderer matrix](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/), [Cargo features](https://docs.slint.dev/latest/docs/rust/slint/docs/cargo_features/), and [royalty-free license](https://slint.dev/terms-and-conditions).

## Dependencies

Versions are exact Phase 0 candidates, not permission for automatic updates.

| Concern | Decision | License | Maintenance and footprint | Unsafe / real-time assessment | Limitation and isolation |
| --- | --- | --- | --- | --- | --- |
| UI | Slint 1.17.1, adopt for Windows | GPL-3.0-only OR Slint royalty-free 2.0 OR software license 3.0 | Active 1.x; locked all-target graph is 602 packages after removing Wayland; binary measured below | Third-party unsafe exists below window/render backends; no UI code enters callback | `UiPort` depends only on domain snapshots; optional platforms require Phase 12 qualification. |
| Audio | CPAL 0.18.1, adopt | Apache-2.0 | Active cross-platform low-level crate; OS backends selected per target | Callback API can be allocation-free; backend internals contain platform unsafe; error/promotion callbacks stay outside render work | Thin output adapter; no device policy in engine. |
| Demux/decode | Symphonia 0.6.0 with WAV/PCM/ISO-MP4/AAC/ALAC only, accepted for Phase 1 boundary | MPL-2.0 | Active pure-Rust framework; selected features avoid unrelated codecs/formats | Decode allocates and may block; decoder workers only, never callback | Phase 0 exact presentation-window/repeat/seek/memory feasibility passed; Phase 7 retains adversarial and soak qualification. |
| Resampling | Rubato 4.0.0, adopt for prototype | MIT OR Apache-2.0 | Active; FFT graph is non-trivial but bounded | Preallocate resampler and adapters off callback; process prepared stereo on render worker, not device callback | One post-mix adapter; benchmark quality/latency in Phase 6. |
| Ring | rtrb 0.3.4, adopt | MIT OR Apache-2.0 | Small focused SPSC crate | Preallocates once; push/pop lock-free and wait-free; implementation uses reviewed unsafe internally | Wrap in output crate with fixed capacity and overflow counters. |
| SHA-256 | sha2 0.11.0, adopt | MIT OR Apache-2.0 | Active RustCrypto implementation; small selected surface | No hashing in callback; implementation may use target acceleration | One streaming integrity adapter. |
| HTTP/TLS | ureq 3.3.0 with rustls, provisional for Phase 10 | MIT OR Apache-2.0 | Active synchronous client; default gzip must be disabled for bounded media | Blocking/network/allocation only on network workers; TLS graph requires security/license audit | Redirects/credentials disabled in adapter; reconsider async client only if measured concurrency requires it. |

No internal ring, hash, HTTP, or decoder abstraction is built in Phase 0 beyond the smallest adapter boundary named above. The attached audit reports 602 locked packages, zero vulnerabilities, zero ignored advisories, zero unlicensed crates, and four unmaintained Slint-transitive warnings. Handwritten code denies unsafe; dependency unsafe remains isolated behind UI/audio/codec adapters and is reviewed again before release.

## Budgets

Decision: accept the initial budgets in `docs/rustplayer/PLAN.md` unchanged. They are acceptance limits, not current claims. In particular, the 50 MiB idle process-set budget includes all application processes and renderer allocations visible to the OS. A missed budget changes implementation or dependency choice; it does not lower the gate without product and release/security owner approval.

## Release And Security Defaults

- No telemetry, analytics, account, upload, remote logging, or startup network access.
- Handwritten project code denies unsafe code. Slint generated code contains an isolated generated-code exception. Any other exception needs a dedicated crate, soundness note, tests, and review.
- Cargo lockfile and Rust toolchain are committed. Release builds use `--locked`.
- Dependency changes require version, source, license, maintenance, transitive tree, advisories, unsafe surface, and artifact-size delta.
- Automatic update remains disabled until signed artifacts, rollback, and platform signing are proven.
