# Phase 11 - Operating-System Integration, Packaging, Signing, and Installation

## Scope

Deliver the qualified Windows native player as a standalone per-user application. This phase covers application identity, versioning, safe file associations, single-instance open routing, platform media controls, power and device lifecycle handling, platform-standard user directories, installer behavior, signing, release metadata, dependency notices, and provenance.

macOS and Linux packaging are optional Phase 12 expansions. Automatic updates remain disabled unless signed packages, bounded downloads, rollback, and recovery are approved together.

## Entry Criteria

The [Windows hobby preview](../release/windows-preview.md) may produce an explicitly unsigned portable ZIP without signing credentials, an installer toolchain, or new OS application registration. This exception retains the existing identity and runtime behavior, requires manual qualification and publication, and does not pass this phase's exit gate. All requirements below remain in force for the signed installed release.

- Local UI, playback, mixer, waveform, diagnostics, and `stem-hls-v1` gates are complete.
- The supported Windows architecture and release scope are approved.
- Product display name, publisher identity, application identifier, versioning policy, and icon artwork are approved.
- Signing credentials, timestamp policy, secure CI handling, and a Windows installer toolchain are available.

## Requirements

- The application must not imply that it contains the converter.
- A second launch forwards only explicitly supplied files or URLs to the primary process through bounded, user-authenticated local routing. The primary process revalidates every input.
- Register only dedicated stem formats and any separately approved Studio bundle extension. Do not associate generic audio, manifest, or WAV extensions.
- Map platform media commands to the bounded controller interface and expose sanitized track names without full paths.
- Define conservative suspend, resume, session, device-removal, display-sleep, shutdown, and logoff behavior. Do not resume audible playback automatically after a disruptive event without an approved prior-state policy.
- Use platform-standard per-user configuration, cache, log, crash, and update directories. Ordinary playback must run without permanent administrator privileges.
- Produce a signed Windows installer with per-user installation, clean uninstall, file associations, upgrade policy, Unicode-path support, and no browser, converter, Python, Node.js, or FFmpeg runtime baggage.
- Pin dependencies and toolchains, generate checksums, license notices, SBOM data, and provenance, and scan release artifacts for secrets, private paths, private fixtures, bundled runtimes, and debug symbols.
- Keep unsigned automatic updates disabled.

## Security Requirements

Inputs from the shell, file associations, local routing, manifests, URLs, installers, and update metadata are untrusted. Bounds, authentication, scheme policy, path validation, and failure handling must be explicit and tested. No general command-execution IPC surface is permitted.

## Exit Gate

Phase 11 passes only when a clean Windows system can install, upgrade when supported, launch, open every supported source, use qualified media controls, survive the approved power and device lifecycle cases, and uninstall the signed native player without browser or converter runtime baggage. Signature verification, standard-user installation, Unicode paths, locked-file behavior, and artifact-content scans must pass. The current phase remains blocked until the entry criteria are supplied; see `STATUS.md`.
