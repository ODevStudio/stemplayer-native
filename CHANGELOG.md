# Changelog

## 0.1.0-preview.1 (Unreleased)

- Added unsigned Windows x64 portable ZIP packaging with dependency notices, SBOM, checksums, build identity, static CRT linkage, and manual CI artifact preparation.
- Patched eight dependency vulnerability findings in the ring buffer, TLS/certificate stack, and browser-opening helper; restricted three reviewed Linux D-Bus maintenance exceptions with a Windows dependency-graph check.

## 2026-08-19

- Added the native Windows application icon and aligned its four stem bars with the product waveform colors.

## 2026-07-31

- Added playlist append, bounded multi-selection, removal, context actions, and pointer reordering.
- Preserved playlist selection, playback state, and waveform-analysis state while entries are changed or reordered.
- Qualified ordinary-audio waveform frequency coloring and bounded paused-playlist analysis.
- Completed the native `stem-hls-v1` playback implementation and its bounded security and continuity checks.

## 2026-07-22

- Completed the native player foundations, domain contracts, output core, format validation, transport, mixer, UI, waveform, and diagnostics phases.
