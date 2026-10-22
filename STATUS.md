# Status

Current phase: Phase 11, operating-system integration, packaging, signing, and installation.

Current status: blocked.

The local player, mixer, waveform, diagnostics, `stem-hls-v1` playback gates, and Windows icon set are implemented, but the audit integration blockers below remain open. Phase 11 also cannot advance until the product identity, publisher identity, application identifier, signing credentials, Windows packaging toolchain, and clean-system qualification environments are supplied.

The first release remains Windows-qualified. macOS and Linux packaging are optional later platform expansions. Automatic updates remain disabled.

## Hobby Preview

An unsigned portable Windows 11 24H2-or-newer x64 preview is approved as a separate, limited release path. It does not complete Phase 11 or Phase 12. The initial version is `0.1.0-preview.1`; product identity, icons, default features, and per-user settings locations are unchanged.

Publication remains blocked until the exact CI artifact passes the [preview checklist](docs/release/windows-preview.md), dependency/license review, and clean-system qualification. Public-repository history and metadata review must precede any visibility change. No clean-system, human accessibility, audio-device, suspend/resume, one-hour playback, or preview performance qualification is claimed by the packaging implementation.

The 2026-10-21 [security refresh](docs/qualification/windows-preview-security-refresh.md) fixes all eight reported vulnerability findings with patch updates. The workspace and Windows-target dependency audits now pass. The maintainer approved three exact maintenance-advisory exceptions for retired Linux-only D-Bus dependencies; a product test prevents those packages from entering the Windows graph. The five earlier exceptions remain unchanged. CI packaging still requires the dependency-policy gate.

## Public Source Preparation

The 2026-10-22 exposure review covered all local refs, including the four advertised remote branches and local checkpoints, all 92 reachable commits with merge diffs, and the Git-visible working source. Checksum-verified Gitleaks 8.30.1 ran locally in fully redacted mode. Its single source/history finding was fixture-policy prose, not a credential. Path, author, file-history, and fixture-provenance checks identified no private media, screenshots, raw qualification output, or personal machine paths in the publishable source/history. Original synthetic codec fixtures and both project licenses are retained.

GitHub review covered repository metadata, both PRs and their discussions/reviews, and all nine available Actions logs. The nine log scanner matches were Cargo cache identifiers, not credentials. There are no releases/assets, Actions artifacts, or Actions caches; wiki and discussions are disabled. Actions itself remains disabled and was not changed.

Local environment and signing-key files are now ignored. The repository checker passes from the normal root and still inspects force-added ignored Markdown. Raw scan/download evidence remains ignored, not publication content.

Existing Git author/committer identities are retained. Visibility remains private and requires separate maintainer approval to change; no history rewrite or release publication is part of this preparation. Source publication approval is separate from the unchanged binary-release blockers above.

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
- AAC perceptual-noise-substitution seek equivalence is blocked by the pinned decoder's private noise state. Synthetic repeated-seek and sequential-versus-seek comparisons fail and remain explicitly ignored in the normal suite. A scoped decoder patch requires approval and provenance/license review; the accepted format profile has not been weakened.
- Real UI/NVDA, audio devices, suspend/resume, long soaks, Unix execution, the actual preview ZIP, and clean-system/release qualification have not been executed for these repairs.

### Integration Checks

These results apply to the repaired source on Windows x64, not to the earlier preview binary. No executed test failures remain. The six ignored workspace tests include the two known-failing AAC PNS demonstrations and four existing qualification tests.

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
