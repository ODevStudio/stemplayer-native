# Phase 11 — Operating-System Integration, Packaging, Signing, and Installation

## Goal

Deliver native desktop behavior and easy installation on Windows. Add file associations, single-instance routing, media controls, power/device lifecycle integration, secure settings paths, signed packages, dependency notices, and optional signed updates without bundling browser or converter runtimes. macOS and Linux packaging are optional Phase 12 platform expansions.

## Entry criteria

- Local UI, playback, mixer, waveform, and diagnostics phases are complete.
- HLS is complete if the targeted release promises HLS parity.
- Supported OS/architecture matrix is final.
- Application identifier, publisher identity, icons, and versioning policy are approved.
- Signing/notarization credentials and secure CI secret handling are available.

## Work package 11.1 — Application identity and versioning

Define:

- Stable application identifier per platform.
- Semantic versioning policy.
- Build metadata policy.
- Config/cache schema versions independent of application version.
- Release channel naming.
- Window/class identifiers.
- Protocol handler only if justified; avoid unnecessary URL schemes.

The native player name must not imply it contains the converter.

## Work package 11.2 — Single-instance and open routing

Implement one primary application instance per user session unless platform conventions require otherwise:

- A second launch forwards explicitly provided files/URLs to the primary instance.
- Inputs are revalidated in the primary process.
- Requests are bounded and authenticated to the local user context.
- Opening a new source while another plays follows an explicit confirmation or replacement policy.
- Malformed IPC cannot crash the player.

Do not create a general command-execution IPC surface.

## Work package 11.3 — File associations

Register only specific player formats:

- `.stem.m4a`
- `.stem.mp4`
- A dedicated Studio bundle extension if one exists in the approved format roadmap.

Do not associate generic `manifest.json`, `.m4a`, `.mp4`, or `.wav` globally. Studio manifests remain available through Open Folder/Open Manifest unless a dedicated extension is introduced.

File association launch must preserve paths exactly and pass them through native validation.

## Work package 11.4 — Media controls

Integrate platform media controls:

- Play/pause.
- Stop where supported.
- Seek where supported and reliable.
- Position/duration updates at a bounded rate.
- Track display name without leaking full paths.
- Artwork absent unless explicitly available and safe.

Platform targets:

- Windows system media transport controls.
- macOS media command center.
- Linux MPRIS.

Media commands map to the same bounded controller interface as the UI.

## Work package 11.5 — Power and session lifecycle

Handle:

- System suspend.
- Resume.
- User session lock/unlock where relevant.
- Audio device removal during suspend.
- Display sleep.
- Shutdown/logoff notification.

Policy must define whether playback pauses before suspend and whether it resumes automatically. Conservative default: do not resume audible playback automatically after a disruptive lifecycle event without confirmed prior state and product approval.

## Work package 11.6 — Platform directories and permissions

Use platform-standard locations for:

- Configuration.
- Cache.
- Logs when enabled.
- Crash records when enabled.
- Update downloads.

The application should run as a normal user. Installation should not require permanent administrator privileges for ordinary per-user scenarios. Media files remain read-only unless the user explicitly exports diagnostics elsewhere.

## Work package 11.7 — Windows package

Produce a signed installer with:

- Per-user default installation.
- Start menu integration.
- Clean uninstall.
- File associations.
- Upgrade and downgrade policy.
- No WebView bootstrapper.
- No Python, Node.js, FFmpeg, or converter payload.
- Architecture-specific package or approved multi-architecture strategy.

Test install, repair/upgrade if supported, uninstall, path with Unicode, non-admin account, and locked-file behavior.

## Work package 11.8 — Optional macOS package

Produce:

- Native `.app` bundle.
- Signed and notarized distribution artifact.
- DMG or approved installer format.
- Correct entitlements with minimum privilege.
- File association declarations.
- Apple silicon build; universal or Intel build according to Phase 0 decision.

Test Gatekeeper first launch, quarantine behavior, upgrade, uninstall guidance, device permissions, and app translocation-sensitive paths.

## Work package 11.9 — Optional Linux packages

Produce the approved set, normally:

- `.deb`
- `.rpm`
- AppImage or Flatpak as a portable/managed option

Document runtime audio and graphics dependencies. Test at least the chosen PipeWire and fallback environments. Desktop file, icons, MIME registrations, MPRIS, uninstall, and sandbox limitations must be qualified.

Do not label a package “portable” if it writes outside documented user directories.

## Work package 11.10 — Signing and supply chain

- Sign release commits/tags according to policy.
- Build in controlled CI.
- Pin dependencies and toolchain.
- Generate checksums.
- Generate dependency license notices.
- Generate SBOM.
- Scan artifacts for accidentally bundled secrets, paths, private fixtures, browser engines, converter binaries, and debug symbols.
- Retain provenance and build logs.

Reproducible builds are a goal; where platform signing prevents bit-for-bit reproducibility, preserve reproducible unsigned intermediates and documented signing steps.

## Work package 11.11 — Update strategy

Automatic updates are optional for the first release. If enabled:

- Manifest and packages are signed.
- HTTPS is mandatory.
- Channel is explicit.
- Download is bounded and resumable only if safely implemented.
- Update is staged outside the running binary.
- Rollback or failed-update recovery is documented.
- No update check occurs when disabled.
- Update service starts lazily.

Never enable unsigned self-update.

## Work package 11.12 — Release configuration

Release builds:

- Disable development fake engine.
- Disable verbose path logging.
- Strip or separate symbols according to crash policy.
- Use panic strategy and unwind/abort decision from architecture review.
- Enable only promised source features.
- Include version and diagnostic build identifiers.
- Preserve local error reporting without network telemetry.

## Deliverables

- Platform integration crate implementations.
- Single-instance open routing.
- Safe file associations.
- Media controls.
- Suspend/resume and device lifecycle behavior.
- Signed Windows installer.
- Optional signed/notarized macOS artifact when the Phase 12 macOS expansion is selected.
- Optional Linux packages when the Phase 12 Linux expansion is selected.
- SBOM, licenses, checksums, and release provenance.
- Optional signed updater.
- Install/uninstall qualification reports.

## Tests and evidence

- Fresh install, upgrade, and uninstall on clean systems.
- Standard-user installation.
- File association open and malicious argument cases.
- Second-instance routing flood and malformed local IPC.
- Media key behavior.
- Suspend/resume while playing, paused, and seeking.
- Default-device change.
- Offline launch and update-disabled launch.
- Artifact content scan proving no browser/converter runtime.
- Windows signature verification.
- Optional macOS notarization and Linux dependency/desktop integration checks belong to their Phase 12 platform expansions.

## Resource checks

- Installed size by platform.
- First-launch and steady-state memory.
- Update and media-control services idle cost.
- No extra helper process remains running after player exit.
- No background process is installed for ordinary playback unless explicitly approved.

## Risks and mitigations

- **Packaging tool adds runtime baggage:** inspect final artifacts and process tree.
- **File association too broad:** register only dedicated stem formats.
- **Updater increases attack surface:** keep disabled until signing and rollback are proven.
- **Platform media APIs leak paths:** publish sanitized titles only.
- **Linux package fragmentation:** define a supported package matrix and test it, rather than claiming every distribution.

## Exit gate

Phase 11 is complete when clean Windows systems can install, upgrade, launch, open supported sources, use media controls, survive power/device lifecycle events, and uninstall the signed native player without browser or converter runtime baggage. macOS and Linux packages are optional Phase 12 platform-expansion deliverables and remain unpublished unless qualified.
