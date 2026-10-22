# Status

Current phase: Phase 11, operating-system integration, packaging, signing, and installation.

Current status: blocked.

The local player, mixer, waveform, diagnostics, `stem-hls-v1` playback gates, and Windows icon set are complete. Phase 11 cannot advance until the product identity, publisher identity, application identifier, signing credentials, Windows packaging toolchain, and clean-system qualification environments are supplied.

The first release remains Windows-qualified. macOS and Linux packaging are optional later platform expansions. Automatic updates remain disabled.

## Hobby Preview

An unsigned portable Windows 11 24H2-or-newer x64 preview is approved as a separate, limited release path. It does not complete Phase 11 or Phase 12. The initial version is `0.1.0-preview.1`; product identity, icons, default features, and per-user settings locations are unchanged.

Publication remains blocked until the exact CI artifact passes the [preview checklist](docs/release/windows-preview.md), dependency/license review, and clean-system qualification. Public-repository history and metadata review must precede any visibility change. No clean-system, human accessibility, audio-device, suspend/resume, one-hour playback, or preview performance qualification is claimed by the packaging implementation.

The 2026-10-21 [security refresh](docs/qualification/windows-preview-security-refresh.md) fixes all eight reported vulnerability findings with patch updates. The workspace and Windows-target dependency audits now pass. The maintainer approved three exact maintenance-advisory exceptions for retired Linux-only D-Bus dependencies; a product test prevents those packages from entering the Windows graph. The five earlier exceptions remain unchanged. CI packaging still requires the dependency-policy gate.

## Split Boundary

The web repository boundary is source commit `08dfe03cc4ada7b8cac84d6191198e25e59aeb1e`. The native repository was extracted from source commit `7d6a9b689b84c9b93c82d573cc5020f5dd8397a2` with `git-filter-repo`; generated qualification output was excluded from native history.

## Verification Baseline

- Locked Rust workspace tests passed before extraction.
- Local Windows preview packaging, static-CRT imports, and extracted startup checks passed on 2026-10-21; see the [redacted verification summary](docs/qualification/windows-preview-packaging.md). This is not clean-system or release qualification.
- The [security refresh verification](docs/qualification/windows-preview-security-refresh.md) records passing patched-graph audits, tests, fresh ZIP packaging, imports, and non-elevated extracted startup; earlier binary measurements do not apply to the refreshed candidate. Manual preview gates remain open.
- Phase 10 HLS qualification passed on 2026-07-28.
- Raw qualification output is retained outside this repository only as an unapproved local archive until release storage is selected.
