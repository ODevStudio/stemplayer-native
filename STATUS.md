# Status

Current phase: Phase 11, operating-system integration, packaging, signing, and installation.

Current status: blocked.

The local player, mixer, waveform, diagnostics, `stem-hls-v1` playback gates, and Windows icon set are implemented, but the audit integration blockers below remain open. Phase 11 also cannot advance until the product identity, publisher identity, application identifier, signing credentials, Windows packaging toolchain, and clean-system qualification environments are supplied.

The first release targets Windows. Additional Linux x64 and macOS Apple silicon preview packaging is now authorized; stable platform qualification remains separate. Automatic updates remain disabled.

## Windows Preview

An unsigned portable Windows 11 24H2-or-newer x64 preview is approved as a separate, limited release path. It does not complete Phase 11 or Phase 12. The initial version is `0.1.0-preview.1`; product identity, icons, default features, and per-user settings locations are unchanged.

The experimental preview uses the exact CI artifact after automated build, integrity, dependency/license, archive, and checksum checks. The [preview checklist](docs/release/windows-preview.md) retains the separate manual qualification work; unexecuted checks are not passes or stable-release qualification. Public-repository history and metadata review must precede any visibility change. No clean-system, human accessibility, audio-device, suspend/resume, one-hour playback, or preview performance qualification is claimed by the packaging implementation.

The 2026-10-21 [security refresh](docs/qualification/windows-preview-security-refresh.md) fixes all eight reported vulnerability findings with patch updates. The workspace and Windows-target dependency audits now pass. The maintainer approved three exact maintenance-advisory exceptions for retired Linux-only D-Bus dependencies; a product test prevents those packages from entering the Windows graph. The five earlier exceptions remain unchanged. CI packaging still requires the dependency-policy gate.

## Public Source Preparation

The 2026-10-22 exposure review covered all local refs, including the four advertised remote branches and local checkpoints, all 92 reachable commits with merge diffs, and the Git-visible working source. Checksum-verified Gitleaks 8.30.1 ran locally in fully redacted mode. Its single source/history finding was fixture-policy prose, not a credential. Path, author, file-history, and fixture-provenance checks identified no private media, screenshots, raw qualification output, or personal machine paths in the publishable source/history. Original synthetic codec fixtures and both project licenses are retained.

GitHub review covered repository metadata, both PRs and their discussions/reviews, and all nine available Actions logs. The nine log scanner matches were Cargo cache identifiers, not credentials. At that review there were no releases/assets, Actions artifacts, or Actions caches; wiki and discussions were disabled. Actions remained disabled during the exposure review; the approved continuation below enabled it without dispatching a hosted build.

Local environment and signing-key files are now ignored. The repository checker passes from the normal root and still inspects force-added ignored Markdown. Raw scan/download evidence remains ignored, not publication content.

Existing Git author/committer identities are retained. On 2026-10-22 the maintainer approved making the repository public, pushing the reviewed changes, and preparing the preview release. The final exposure review found no new secrets or private material in the pending changes; remote branches, issues, logs, and artifact inventories were unchanged. Source publication and draft-release preparation do not waive the binary-release blockers above. No history rewrite is authorized or required.

The repository is now public. Reviewed source commit `dfbf9903a638a3acb85ee2b20e2a059bc53e048f` was pushed to `master`; its staged Gitleaks scan found no leaks. No binary assets or release tag were published.

## Split Boundary

The web repository boundary is source commit `08dfe03cc4ada7b8cac84d6191198e25e59aeb1e`. The native repository was extracted from source commit `7d6a9b689b84c9b93c82d573cc5020f5dd8397a2` with `git-filter-repo`; generated qualification output was excluded from native history.

## Verification Baseline

- Locked Rust workspace tests passed before extraction.
- Local Windows preview packaging, static-CRT imports, and extracted startup checks passed on 2026-10-21; see the [redacted verification summary](docs/qualification/windows-preview-packaging.md). This is not clean-system or release qualification.
- The [security refresh verification](docs/qualification/windows-preview-security-refresh.md) records passing patched-graph audits, tests, fresh ZIP packaging, imports, and non-elevated extracted startup; earlier binary measurements do not apply to the refreshed candidate. Manual preview gates remain open.
- Phase 10 HLS qualification passed on 2026-07-28.
- Raw qualification output is retained outside this repository only as an unapproved local archive until release storage is selected.

## Audit Integration

All 14 audit reports were gathered on 2026-10-22. Repairs have completed source-review cycles and serialized Windows x64 verification. A testing follow-up repaired the legacy saved-output migration and Windows cache-publication regressions; both fixes passed source review and the automated checks below. These changes are not a qualified preview candidate.

- Windows waveform cache publication now succeeds without relaxing directory protections. The [cache contract](docs/architecture/cache-privacy.md) describes lock-serialized exclusive publication and partial-record recovery. Old index-based output selections migrate to System default; missing stable endpoint selections still require an explicit choice.
- The maintainer approved a scoped AAC perceptual-noise-substitution fix on 2026-10-22. The [vendored patch](vendor/symphonia-codec-aac/PATCHES.md) restores deterministic bounded-seek noise output; both former failing demonstrations now run without `ignore`. Broader codec/listening qualification remains open; the accepted format profile is unchanged.
- Real UI/NVDA, audio devices, suspend/resume, long soaks, Unix execution, the actual preview ZIP, and clean-system/release qualification have not been executed for these repairs.

### Integration Checks

This table is the pre-continuation Windows x64 baseline at `9f6c767`, not evidence for the earlier preview binary or the later decoder patch. Its six ignored tests included the two AAC PNS demonstrations. See the continuation record below for current checks.

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --locked -p stemplayer --all-targets -- -D warnings` | Passed |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | Passed |
| `cargo test --locked -p stemplayer-platform` | Passed: 16 tests |
| `cargo test --locked -p stemplayer-waveform` | Passed: 36 tests |
| `cargo test --locked -p stemplayer` | Passed: 97 passed, 1 ignored |
| `cargo test --locked --workspace --all-features --no-fail-fast` | Passed: 327 passed, 6 ignored |
| `cargo check --locked --workspace --no-default-features` | Passed |
| `cargo check --locked --workspace --all-features` | Passed |
| `cargo build --locked --release -p stemplayer` | Passed; build evidence only |
| `cargo run --locked --release -p stemplayer-callback-audit` | Passed earlier in this audit: zero allocations in 100000 callbacks and 1000 render iterations |
| `cargo deny check` | Passed earlier in this audit with existing tool warnings/diagnostics; dependency graph unchanged |
| `cargo deny --target x86_64-pc-windows-msvc check` | Passed earlier in this audit with existing tool warnings/diagnostics; dependency graph unchanged |
| `python -m unittest discover -s scripts -p "test_*.py" -v` | Passed: 4 tests after public-source cleanup |
| `pwsh -NoProfile -File scripts/test-windows-preview.ps1` | Passed earlier in this audit; synthetic packaging only |
| `pwsh -NoProfile -File scripts/test-qualification.ps1` | Passed earlier in this audit; synthetic script checks only |
| `python scripts/check_repository.py` | Passed from the normal repository root after public-source cleanup |

Repository-integrity checks no longer need a source-snapshot workaround. Rust commands were not repeated for the documentation/checker-only cleanup. No release, hardware, or clean-system gate is inferred from these results.

## Release Continuation

The [continuation record](docs/qualification/windows-preview-audit-continuation.md#final-source-ci) covers the audited source, approved decoder patch, notice repairs, and CI follow-ups. At source commit `20a196d2bcd42ff8a7a12cbe5a30568345eef05c`, all 332 Windows workspace tests pass, with only four existing qualification tests ignored; 97 product tests pass, with one existing device test ignored. The package validator rejects the pinned collector's placeholder copyright text and binds modified AAC source notices to the build commit. Source checks do not qualify a distributable.

The [notice review](docs/release/windows-preview.md#upstream-notice-prerequisite) now retains the checksum-bound `realfft 3.5.0` upstream MIT declaration, author metadata, and standard MIT terms without inventing a copyright notice. The accepted-license set is unchanged. The [other notice repairs](docs/qualification/windows-preview-audit-continuation.md#notice-repairs-and-public-ci) retain Chromium BSD and libm/musl attribution and reject unexpected collector warnings. Do not reuse the 2026-10-21 ZIP after the audit/code changes, or claim its qualification applies to current source. A fresh exact-commit CI artifact is required.

Actions is enabled with maintainer approval. The full [source CI run](https://github.com/ODevStudio/stemplayer-native/actions/runs/39862835870) passed at `20a196d2bcd42ff8a7a12cbe5a30568345eef05c`: workflow integrity, dependency policy, and quality jobs on Windows, Ubuntu, and macOS. This includes workspace Clippy/tests, product tests/builds, separate development-profile UI startup checks, both feature checks, and the Windows synthetic packaging check. The previously failing cache-lock and fixture regressions pass on macOS. No test was suppressed or validation weakened. Subsequent changes only record this evidence; they do not change runtime code or qualify a release artifact.

[Version 0.1.0-preview.1 is published](https://github.com/ODevStudio/stemplayer-native/releases/tag/v0.1.0-preview.1) with the unchanged Windows ZIP and `SHA256SUMS.txt` from successful [manual CI run 39959398372](https://github.com/ODevStudio/stemplayer-native/actions/runs/39959398372), source/tag commit `f589012f48c8a340609e61a751001a28cdbd33b9`. Anonymous downloads match the original assets; ZIP SHA-256 is `fab2ecf57a3d4d965568b7bf3e4b0885fc7e13045104ae7cdffb889d070add53`. The archive contains only the seven approved files, its x64 executable imports only approved system DLLs, and its SBOM has 431 components and no broken dependency references. No private-repository hosted job was dispatched.

Clean Windows, human accessibility/listening, device recovery, suspend/resume, one-hour playback, and candidate performance checks remain unexecuted for this artifact. No signing or stable-release completion is claimed.

## Additional Platform Previews

The public release now includes Linux x64 and macOS Apple silicon archives from the [Unix packaging path](docs/release/unix-preview.md), using the unchanged `v0.1.0-preview.1` application commit. Existing Windows assets, checksum, and tag remain immutable. Linux uses an Ubuntu 24.04/glibc desktop baseline; macOS uses a macOS 14 minimum and an ad-hoc-signed application bundle without Developer ID or notarization. Dependency review retains the existing exact maintenance-only exceptions and adds no advisory ignores. Linux includes complete application/Plasma protocol-library source and replacement instructions for the protocol specifications' additional license obligations.

The [manual CI run 39970520197](https://github.com/ODevStudio/stemplayer-native/actions/runs/39970520197) at packaging commit `8a0725c01564a8fb93536766e76d35485a766edc` passed integrity, dependency policy, three-platform quality checks, and both native packaging jobs. Native checks covered archive permissions, runtime imports, macOS ad-hoc signing, and extracted executable startup from Unicode/spaced paths. Final review verified approved archive contents, build identity, checksums, all SBOM references (540 Linux components; 453 macOS components), and byte-identical tagged Linux source with the original Plasma crate. Anonymous downloads of both archives and their separate checksum files match the tested artifacts. Existing Windows asset IDs/digests and the annotated release tag are unchanged.

Native-device listening, accessibility, high-DPI, suspend/resume, soak, and clean-system results are not claimed, and the optional Phase 12 platform qualifications remain incomplete.
