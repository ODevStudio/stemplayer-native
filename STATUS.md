# Status

Current phase: Phase 11, operating-system integration, packaging, signing, and installation.

Current status: blocked.

Unsigned release-evidence collection is also blocked by missing license texts in the locked source-dependency inventory. Verified texts and their package/version provenance must be supplied before the Windows evidence gate can pass; license declarations alone do not satisfy that gate.

The local player, mixer, waveform, diagnostics, `stem-hls-v1` playback gates, and Windows icon set are complete. Phase 11 cannot advance until the product identity, publisher identity, application identifier, signing credentials, Windows packaging toolchain, and clean-system qualification environments are supplied.

The first release remains Windows-qualified. macOS and Linux packaging are optional later platform expansions. Automatic updates remain disabled.

## Split Boundary

The web repository boundary is source commit `08dfe03cc4ada7b8cac84d6191198e25e59aeb1e`. The native repository was extracted from source commit `7d6a9b689b84c9b93c82d573cc5020f5dd8397a2` with `git-filter-repo`; generated qualification output was excluded from native history.

## Verification Baseline

- Locked Rust workspace tests passed before extraction.
- Phase 10 HLS qualification passed on 2026-07-28.
- Raw qualification output is retained outside this repository only as an unapproved local archive until release storage is selected.
