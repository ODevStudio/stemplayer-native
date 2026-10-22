# Windows Preview Security Refresh

Date: 2026-10-21. Environment: Windows 11 Pro build 26200, x64, Rust 1.97.1. This development-machine record updates the [initial packaging baseline](windows-preview-packaging.md). See [STATUS.md](../../STATUS.md) for publication blockers.

## Dependency Review

All five patched packages come from crates.io with checksums in `Cargo.lock`. No runtime API, feature, format, or settings contract changed.

| Package | Previous | Patched | License | Findings removed |
| --- | --- | --- | --- | --- |
| rtrb | 0.3.4 | 0.3.5 | MIT OR Apache-2.0 | RUSTSEC-2026-0274 |
| rustls | 0.23.42 | 0.23.46 | Apache-2.0 OR ISC OR MIT | RUSTSEC-2026-0285, RUSTSEC-2026-0346, RUSTSEC-2026-0348, RUSTSEC-2026-0349 |
| rustls-webpki | 0.103.13 | 0.103.16 | ISC | RUSTSEC-2026-0382 |
| webbrowser | 1.2.1 | 1.2.2 | MIT OR Apache-2.0 | RUSTSEC-2026-0257 |
| webpki-roots | 1.0.9 | 1.0.10 | CDLA-Permissive-2.0 | RUSTSEC-2026-0381 |

The root workspace and Phase 0 probe pin rtrb 0.3.5. The remaining updates are locked transitive patches. Cargo also removed the unused core-foundation 0.10.1 entry when webbrowser switched its macOS backend to objc2, and re-resolved existing getrandom/windows-sys dependency edges without changing their package versions. The complete lockfile diff received review.

The rtrb source diff adds a panic guard to chunk destruction so a panicking element destructor cannot leave already-dropped slots readable. Its unsafe implementation remains behind the existing output adapter; the patch adds no dependencies. TLS/certificate work remains on network workers, outside the callback. The browser helper remains transitive through Slint, with no new browser runtime dependency or startup action. The patch updates do not expand first-party unsafe exceptions or supported platforms.

The maintainer approved exactly three additional maintenance IDs for Linux-only zbus 5 dependencies. [Dependency policy](../agent/ci-workflows.md#linux-d-bus-maintenance-exceptions) records their paths and removal trigger. The five prior exceptions retain their existing rationale and scope. No vulnerability ignore was added. Workspace audit enforcement and the packaging job dependency remain intact.

## Commands and Results

| Command | Result |
| --- | --- |
| `cargo test --locked -p stemplayer --test dependencies` | PASS; retired D-Bus packages absent from the reachable default Windows product graph |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --locked --workspace --all-features` | PASS, 195 tests; 4 existing qualification tests ignored |
| `cargo test --locked -p stemplayer` | PASS, 47 tests including the dependency guard; 1 existing two-device test ignored |
| `cargo check --locked --workspace --no-default-features` | PASS |
| `cargo check --locked --workspace --all-features` | PASS |
| `cargo run --locked --release -p stemplayer-callback-audit` | PASS; 100,000 callbacks, 1,000 render iterations, zero allocations in both; automation p95 100 ns against 30,000,000 ns budget |
| `cargo deny --format json check` | PASS, exit 0; zero advisory, license, source, or bans errors |
| `cargo deny --target x86_64-pc-windows-msvc --format json check` | PASS, exit 0 |
| `cargo fmt --all -- --check` | PASS |
| `python scripts/check_repository.py` | PASS |
| `python -m unittest discover -s scripts -p "test_*.py"` | PASS, 3 tests |
| `pwsh -NoProfile -File scripts/test-windows-preview.ps1` | PASS |
| `actionlint -shellcheck= -pyflakes= .github/workflows/ci.yml` | PASS, actionlint 1.7.7 |
| `pwsh -NoProfile -File scripts/package-windows-preview.ps1` | PASS in an isolated committed test snapshot, using a fresh build directory |
| `git diff --check` and untracked-file whitespace checks | PASS; full raw source diff and relevant diagnostics reviewed |

cargo-deny 0.20.2 still emits 44 duplicate and 13 wildcard warnings plus 20 `unresolved-workspace-dependency` tool diagnostics in the full workspace audit. These concern the existing cross-platform/local path graph; no rule was disabled to hide them. The command returns success and reports zero errors in each check summary. The Windows-filtered audit also warns about unused cross-platform license/advisory allowances. Raw diagnostics remain ignored local evidence.

## Refreshed Artifact

The local test snapshot commit is `895c009f884921258596ea82e888b89e1ef7faea`, not a release commit or tag. The packaging build used `cargo build --locked --release -p stemplayer --target x86_64-pc-windows-msvc`, default product features including HLS, static CRT linkage, and source-path remapping. Final documentation and test-formatting edits do not affect this binary. No root-workspace commit was made.

The ZIP contains exactly the seven approved files. `Get-FileHash -Algorithm SHA256` matched the external ZIP checksum, executable build-information hash, and current workspace lockfile hash. `Assert-PreviewPayload` accepted the extracted payload. All five patched versions appear in both notices and SBOM; the 427-component CycloneDX 1.5 graph has 429 unique references and no broken dependency edges. None of the three retired D-Bus packages appear in the Windows SBOM. The wrapper appended the verified full Slint royalty-free license text after the documented cargo-about LicenseRef warnings. No other license failure occurred.

| Measurement | Result |
| --- | --- |
| ZIP size | 10,371,070 bytes |
| Executable size | 22,778,880 bytes; +5,120 bytes from the pre-refresh static-CRT test binary |
| ZIP SHA-256 | `cce5a2be5ba4cae574f043ad60e751ee0ef895b1b214ff5c4bbd077ef289e246` |
| Executable SHA-256 | `32ace07fbd0ffda6e79f1b27774475d1e0ee008ade36e88bdea22f43677ceae7` |
| Cargo.lock SHA-256 | `ecb3023a8bb1afe0715d0888f51a82f272403fff951ca2d3e24c3dca05a5a719` |

`dumpbin /dependents` and the packaging import allowlist found Windows system DLLs only, without a VC redistributable dependency. `Get-AuthenticodeSignature` returned `NotSigned`, as intended. A known private-build-path string check passed; it does not replace the public-exposure review.

After native ZIP extraction into a Unicode/spaced path, `stemplayer.exe --ui-smoke 1000` exited 0 with empty stdout/stderr. The test used a non-elevated token and isolated `APPDATA`/`LOCALAPPDATA` folders on this development machine. It proves local startup only.

## Remaining Gates

No clean-system, human accessibility/high-DPI, device/listening, suspend/resume, one-hour playback, sustained resource/performance qualification, or public-history/metadata review ran during this refresh. No GitHub-hosted workflow run, repository visibility change, release tag, signing, or publication took place. These gates remain in the [preview runbook](../release/windows-preview.md); this record does not complete Phase 11 or Phase 12.
