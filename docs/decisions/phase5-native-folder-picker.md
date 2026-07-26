# Phase 5 Native Folder Picker Decision

Date: 2026-07-23
Status: accepted for the Windows product target

## Decision

Use rfd 0.15.4 with default features disabled, scoped to the Windows application target, for selecting a Studio WAV bundle directory.

## Rationale

The product needs a native directory picker, not a custom filesystem browser. rfd delegates to the Windows dialog API and keeps file selection outside the audio engine, output callback, and shared domain crates. The dependency is absent on non-Windows targets and does not change the headless engine boundary.

## Security and privacy

The dialog returns one operator-selected path. Validation, path normalization, manifest limits, safe-name checks, and file opening remain in stemplayer-platform and stemplayer-format. No directory enumeration, telemetry, network access, or persistent recent-file list is added by the player.

## License and closure

rfd declares MIT. With default features disabled on Windows, its direct runtime closure is log, raw-window-handle, and windows-sys/windows-targets. The locked release application is 12,060,672 bytes and has SHA-256 ECB665C538369C0436C2242AE7C0BC39D50E7F94A7333EE0CC897ED6C0CB75B9.

## Rejected alternatives

A hand-built picker duplicates native platform behavior and accessibility. An environment-variable-only picker is acceptable for headless tests but not the product workflow. Adding a cross-platform UI backend for one Windows dialog would expand the dependency and qualification surface without Phase 5 value.