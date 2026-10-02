# Changelog

## Unreleased

- Added a native local-only support command with embedded build identity, non-overwriting JSON export, and a bounded deterministic source-ring benchmark.
- Added unsigned Windows build-evidence tooling for dependency inventories, license texts, checksums, provenance records, and artifact-content checks. These tools do not sign, publish, or qualify a release.

## 2026-08-19

- Added the native Windows application icon and aligned its four stem bars with the product waveform colors.

## 2026-07-31

- Added playlist append, bounded multi-selection, removal, context actions, and pointer reordering.
- Preserved playlist selection, playback state, and waveform-analysis state while entries are changed or reordered.
- Qualified ordinary-audio waveform frequency coloring and bounded paused-playlist analysis.
- Completed the native `stem-hls-v1` playback implementation and its bounded security and continuity checks.

## 2026-07-22

- Completed the native player foundations, domain contracts, output core, format validation, transport, mixer, UI, waveform, and diagnostics phases.
