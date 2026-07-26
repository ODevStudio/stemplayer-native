# Phase 4 — Studio WAV Bundle Validation and Bounded Decode

## Goal

Implement the first real source path: secure inspection and bounded decoding of Studio WAV bundles containing a manifest, four required stereo WAV stems, and an optional stereo master. This phase does not yet require audible playback.

## Entry criteria

- Phase 1 domain, format, error, and probe foundations exist.
- Phase 0 Studio WAV specification is approved.
- Public synthetic fixture generation is available.

Phase 4 can proceed in parallel with Phases 2 and 3 after Phase 1.

## Work package 4.1 — Source-selection model

Support explicit user selections:

- A `manifest.json` plus its sibling files.
- A selected folder containing the manifest and files.
- A drag-and-dropped folder or compatible group resolved by the platform layer.

The format crate receives a constrained source provider, not unrestricted filesystem access. It resolves only files explicitly included in the selected group/folder.

## Work package 4.2 — Manifest parser

Implement a bounded JSON parser and validator for the approved manifest schema versions:

- Fatal UTF-8 decoding.
- Maximum manifest bytes.
- Maximum nesting depth and node count.
- Rejection of duplicate semantic fields where the parser exposes them.
- Rejection of prototype-like or forbidden keys if generic maps are used.
- Required unique roles: drums, bass, other, vocals.
- Optional unique master.
- Ignore or report non-routed outputs according to the specification; never execute them.
- Deterministic canonical form for hashing or diagnostics where required.

Unknown fields follow the documented compatibility rule. Invalid declarations fail before media decoding begins.

## Work package 4.3 — Safe basename resolution

For every referenced file:

- Require a normalized basename only.
- Reject absolute paths, parent traversal, separators, URL schemes, drive prefixes, UNC/network paths, device paths, empty names, control characters, and normalization ambiguity.
- Resolve case-insensitively only according to the approved cross-platform rule.
- Reject duplicate case-folded or Unicode-normalized names.
- Never search outside the selected source group.

The implementation must be robust on case-sensitive and case-insensitive filesystems.

## Work package 4.4 — Hash verification

When SHA-256 is declared:

- Require exactly the specified hexadecimal representation.
- Hash in bounded chunks.
- Permit cancellation.
- Report per-file progress through a bounded progress channel.
- Fail closed on mismatch.
- Avoid retaining full file contents.

Hash state must not run on the real-time audio thread in later phases.

## Work package 4.5 — RIFF/WAVE parser

Implement explicit validation for the qualified profile:

- RIFF/WAVE container.
- Chunk bounds, padding, and ordering tolerance.
- `fmt ` and `data` requirements.
- Stereo only.
- 44.1 kHz or 48 kHz input.
- PCM16, PCM24, PCM32 integer, and IEEE Float32.
- Correct byte rate and block alignment.
- Data length divisible by frame size.
- Bounded unknown-chunk skipping.
- Reject RF64, Float64, compressed WAV, mono, multichannel, truncated, overflowed, or malformed inputs unless a later profile explicitly adds them.

Use checked integer arithmetic for all offsets, lengths, and frame counts.

## Work package 4.6 — PCM conversion

Decode bounded blocks into planar or interleaved Float32 according to the engine contract:

- PCM16 divide by 32,768.
- PCM24 sign-extend and divide by 8,388,608.
- PCM32 integer divide by 2,147,483,648.
- Float32 copy unchanged, including values outside [-1, 1].
- Reject or diagnose NaN and Infinity according to the approved source policy; do not silently normalize.
- Maximum block size initially 4,096 frames.
- Reuse preallocated buffers where possible.

Golden tests must prove exact conversion values at minimum, maximum, zero, and sign boundaries.

## Work package 4.7 — Cross-track consistency

Before READY eligibility, require every routed track to have:

- Same sample rate.
- Stereo channel count.
- Same total frame count.
- Valid and unique role.

Set session start frame to zero and end frame to common exclusive total frames. No trimming, padding, resampling, or silence insertion repairs a mismatch.

## Work package 4.8 — Bounded source access

Implement file readers that:

- Read only requested ranges.
- Keep a bounded number of open handles.
- Avoid whole-file memory maps by default; any mapping decision must have platform and address-space evidence.
- Support cancellation on generation change.
- Report bytes read and peak pending blocks.
- Detect file mutation after validation using size, modification metadata, or revalidation policy.

A source changed during playback must produce a deterministic error, not undefined audio.

## Work package 4.9 — Diagnostics and probe

Implement:

```text
stemplayer-probe inspect <manifest-or-folder>
stemplayer-probe validate <manifest-or-folder>
stemplayer-probe decode-window <source> --start-frame <n> --frames <n>
```

Diagnostics include roles, sample rate, frames, duration, bit depths, hash status, bytes read, ignored outputs, and stable warnings. Full paths are omitted from normal JSON output unless an explicit local-debug flag is enabled.

## Deliverables

- Studio manifest parser and validator.
- Safe file resolver.
- Bounded SHA-256 verifier.
- RIFF/WAVE parser.
- PCM conversion adapters.
- Cross-track consistency validator.
- Cancellable range reader.
- Probe support and fixture corpus.
- Studio source specification conformance report.

## Tests and evidence

- Every accepted bit depth and sample rate.
- Master present and absent.
- Unknown RIFF chunks and valid padding.
- Truncation at every structural boundary.
- Integer overflow attempts.
- Unicode normalization and case-collision cases.
- Traversal, absolute path, scheme, and network path rejection.
- Hash success, mismatch, malformed declaration, and cancellation.
- Cross-track sample-rate and frame-count mismatch.
- Float32 out-of-range preservation.
- No whole-file PCM retention.

## Resource checks

- Peak decode memory independent of track duration.
- Maximum open file handles documented.
- Decode-window bytes read remain close to requested ranges.
- Hashing and decoding have bounded worker counts.
- Cancellation releases handles and buffers promptly.

## Risks and mitigations

- **Filesystem normalization differs by platform:** use explicit normalization and collision tests before opening.
- **WAV edge cases expand scope:** enforce the qualified profile and fail clearly.
- **Files mutate after validation:** define a session consistency check and error path.
- **Hashing delays READY:** measure; later phases may separate “validated” and “playable” only if integrity policy permits.

## Exit gate

Phase 4 is complete when all qualified Studio bundles can be inspected and decoded in bounded windows with exact PCM conversion, malformed or unsafe inputs fail closed, consistency is proven before session creation, and resource use does not scale with track duration.


## Phase 4 execution record - 2026-07-22

Status: exit gate passed.

### Implemented checkpoint

- Added an explicit platform-owned source selection boundary for folders, selected manifests, and dropped file groups. The format crate receives a constrained `SourceGroup` and never enumerates outside it.
- Added bounded schema 1/2 manifest parsing with fatal UTF-8, byte/depth/value limits, forbidden-key and duplicate semantic-field rejection, required role validation, ignored-output handling, and stable source error codes.
- Added NFC basename validation, cross-platform lowercase matching, normalized collision rejection, regular-file enforcement, and fail-closed path, scheme, drive, UNC, device-name, traversal, and control-character handling.
- Added streamed SHA-256 verification with 64 KiB reads, cancellation, per-file progress through a bounded nonblocking channel, and mismatch rejection.
- Added checked RIFF/WAVE inspection for PCM16, PCM24, PCM32, and finite Float32 stereo at 44.1/48 kHz, including bounded unknown-chunk skipping and valid odd padding.
- Added planar Float32 decode windows capped at 4,096 frames, exact integer divisors, out-of-range finite Float32 preservation, and non-finite rejection.
- Added cross-track sample-rate/frame consistency, post-validation file identity checks, bytes-read diagnostics, and one-open-file synchronous access.
- Added `inspect`, `validate`, and `decode-window` probe commands plus the Studio WAV v1 conformance report.

### Evidence at this checkpoint

| Check | Command | Result |
| --- | --- | --- |
| Focused Phase 4 tests | `cargo test -p stemplayer-format -p stemplayer-decode -p stemplayer-platform -p stemplayer-probe` | PASS, 28 tests |
| Workspace format | `cargo fmt --all -- --check` | PASS |
| Workspace lint | `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | PASS |
| Workspace tests | `cargo test --locked --workspace --all-features` | PASS, 82 tests |
| Linux headless | `scripts/qualify-wsl.ps1 -OutputDirectory artifacts/phase4/wsl-ubuntu` | PASS, Ubuntu 24.04 tests and metadata |
| Dependency audit | `scripts/audit-dependencies.ps1 -OutputDirectory artifacts/phase4/dependencies` | PASS, 623 dependencies, 0 vulnerabilities, 0 unlicensed crates |
| Dependency direction | `cargo tree --locked -p stemplayer-format -p stemplayer-decode` | PASS, format uses serde, serde_json, sha2, Unicode normalization, and domain; decode adds no source-I/O dependency |
| Forbidden coupling | exact Phase 4 source scan | PASS, zero webplayer, Node.js, Python, FFmpeg, WebView, Electron, Tauri, AudioWorklet, or converter references |
| Manifest limits | generated conformance tests | PASS, byte, depth, value, duplicate, forbidden-key, role, hash, path, Unicode, and collision cases |
| WAV profile | generated conformance tests | PASS, all qualified encodings/rates, optional master, odd padding, chunk reordering, structural truncation, overflow, RF64, channel/rate/format/alignment, and duplicate-data cases |
| Decode fidelity | golden conversion tests | PASS, integer extrema/sign boundaries, finite Float32 outside `[-1, 1]`, and NaN/Infinity rejection |
| Resource bound | sparse 4,096-frame vs one-hour 48 kHz test | PASS, identical per-track inspection reads; each 4,096-frame PCM16 window reads 16,384 bytes |
| Mutation and cancellation | generated conformance tests | PASS, deterministic `E_BUNDLE_INCONSISTENT` and `E_BUNDLE_ABORTED` |
| WSL artifact | `artifacts/phase4/wsl-ubuntu/headless.json` | PASS |
| Audit artifact | `artifacts/phase4/dependencies/audit.json` | PASS |

### Exit-gate result

- Passed. Qualified Studio bundles can be securely inspected and decoded in exact bounded windows; malformed and unsafe inputs fail closed; routed-track consistency is established before session creation; and inspection/decode memory and I/O are independent of track duration.

### Limitations

- `STEMPLAYER_NATIVE_STUDIO_BUNDLE` was not configured, so no private listening bundle was tested. The committed synthetic structural gate is complete; audible playback remains Phase 5.
