# Windows Preview Packaging Verification

Date: 2026-10-21. Environment: Windows 11 Pro build 26200, x64, Rust 1.97.1. This records development-machine checks, not release approval. See [STATUS.md](../../STATUS.md) for current blockers and the [runbook](../release/windows-preview.md) for release gates.

Historical baseline before dependency fixes. The [security refresh](windows-preview-security-refresh.md) supersedes the audit outcome and records verification of the patched graph. The measurements below describe the original test binary only.

## Commands and Results

| Command | Result |
| --- | --- |
| `python scripts/check_repository.py` | PASS |
| `python -m unittest discover -s scripts -p "test_*.py"` | PASS, 3 tests |
| `git diff --check` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --locked --workspace --all-features` | PASS, 194 tests; 4 existing hardware/private-fixture/long-qualification tests ignored |
| `cargo test --locked -p stemplayer` | PASS, 46 tests; 1 existing two-device test ignored |
| `cargo check --locked --workspace --no-default-features` | PASS |
| `cargo check --locked --workspace --all-features` | PASS |
| `pwsh -NoProfile -File scripts/test-windows-preview.ps1` | PASS |
| `actionlint -shellcheck= -pyflakes= .github/workflows/ci.yml` | PASS, actionlint 1.7.7 |
| `pwsh -NoProfile -File scripts/package-windows-preview.ps1` | PASS in an isolated committed test snapshot; no release from the uncommitted working tree |
| `cargo deny check` | FAIL, eight vulnerability and three unmaintained diagnostics |
| `cargo deny --format json check` | Same failure; complete diagnostics inspected without changing policy |

The packaging script executed `cargo build --locked --release -p stemplayer --target x86_64-pc-windows-msvc` with default product features, static CRT linkage, source-path remapping, and a fresh target directory. The test snapshot's commit and artifacts are local test evidence, not a tagged source release. No dependency versions changed except the first-party preview version.

## Artifact Checks

The generated ZIP contained exactly seven approved files. ZIP and executable SHA-256 checks matched their recorded values; product/version/target/features agreed across archive naming, build information, and SBOM. The CycloneDX 1.5 SBOM contained 427 components and no broken dependency references after local path removal. The package included full verified Slint royalty-free terms, MPL license text, and component source links.

The extracted executable was unsigned as expected. `dumpbin /dependents` and the packaging import gate found Windows system libraries only, without a VC redistributable DLL requirement. The static-CRT executable measured 22,773,760 bytes. A known private-build-path string check passed; this does not replace the full release secret/history review.

`stemplayer.exe --ui-smoke 1000` exited successfully from a Unicode/spaced extraction path, using isolated per-user data directories. This proves local startup only, not playback, listening, screen-reader, SmartScreen, or clean-system behavior.

## Dependency Audit Observations

| Locked package | Reported advisory IDs |
| --- | --- |
| `rtrb 0.3.4` | RUSTSEC-2026-0274 |
| `rustls 0.23.42` | RUSTSEC-2026-0285, RUSTSEC-2026-0346, RUSTSEC-2026-0348, RUSTSEC-2026-0349 |
| `rustls-webpki 0.103.13` | RUSTSEC-2026-0382 |
| `webbrowser 1.2.1` | RUSTSEC-2026-0257 |
| `webpki-roots 1.0.9` | RUSTSEC-2026-0381 |
| `zbus_names 4.3.4`, `zvariant 5.13.1`, `zvariant_derive 5.13.1` | RUSTSEC-2026-0371, RUSTSEC-2026-0370, RUSTSEC-2026-0372, respectively; unmaintained |

These are dependency-policy diagnostics, not claims that each issue is reachable in the Windows product. The enforcing workspace audit failed. Bans, licenses, and sources checks passed with existing duplicate/wildcard warnings; cargo-deny also emitted workspace-dependency resolution diagnostics. The five existing exceptions remain unchanged. No new ignores, policy relaxation, or dependency upgrades were applied. The packaging CI job requires the audit job to pass.

The pinned cargo-about and cargo-cyclonedx workarounds and the yanked build-tool-only XML dependency warning are documented in the runbook. Generated raw diagnostics and test artifacts remain ignored and are not part of the source change.

## Not Executed

No clean-system qualification, human screen-reader/high-DPI check, listening/device/suspend-resume gate, one-hour playback, performance-budget qualification, public-exposure review, GitHub-hosted run of the changed workflow, visibility change, release tag, signing, or publication was executed. Installer and stable-release gates remain outside this preview verification.
