# Linux and macOS Preview Packaging

The additional archives use the unchanged application source at `v0.1.0-preview.1`, commit `f589012f48c8a340609e61a751001a28cdbd33b9`. Packaging code is versioned separately and recorded in each archive's `BUILD-INFO.json`. Do not move the tag, rebuild or replace the published Windows assets, or overwrite any published checksum. See [STATUS.md](../../STATUS.md) for distribution and qualification status.

## Targets

- Linux: `x86_64-unknown-linux-gnu`, built on Ubuntu 24.04, glibc 2.39 or older requirements, X11 desktop initially. The archive uses system libraries, not a bundled Linux runtime.
- macOS: `aarch64-apple-darwin`, macOS 14 deployment target, built on the macOS 15 Apple silicon runner. The ZIP contains a native application bundle using the existing `stemplayer-native` application identity and icon. It is ad-hoc signed for code integrity, not publisher signed or notarized.

These are additional experimental previews, not completion of the optional Phase 12 platform qualifications. Hosted startup checks do not establish real-device playback, screen-reader, display scaling, suspend/resume, or long-playback qualification. No installer, file association, updater, new publisher identity, Intel Mac, or Linux ARM64 support is added.

## Build and Check

Manually dispatch `native-rust` on `master`, selecting `linux-and-macos`. The Unix jobs depend on successful integrity, quality, and dependency-policy jobs. A separate clean checkout selects the immutable application source above; the primary checkout supplies reviewed packaging code and notices. Both use read-only repository access and SHA-pinned actions. The Windows packaging job is skipped for this selection.

The scripts use PowerShell 7 and native `tar`, `readelf`/`ldd`, or Apple `ditto`, `sips`, `iconutil`, `plutil`, `lipo`, `otool`, and `codesign`. Cargo-about 0.9.2 and cargo-cyclonedx 0.5.9 remain build-only tools, installed locked with debug profiles. The application uses the unchanged release profile and default features, including HLS. No build cache is restored for packaging.

Run the focused structural, failure, permission, checksum, and archive-roundtrip checks with:

```powershell
pwsh -NoProfile -File scripts/test-unix-preview.ps1
```

Both layouts can be structurally checked on Windows; native archive and executable-permission checks run on their respective Unix CI hosts. The real packaging script verifies source and packaging checkout cleanliness, exact source commit, version, target, default features, bounded approved payload files, notices, SBOM, and binary hashes. It rejects links and private build paths. Linux dependency inspection rejects missing or non-system libraries and glibc requirements newer than 2.39. macOS rejects non-system linked libraries and verifies the completed ad-hoc bundle signature.

The produced archive is extracted into a Unicode/spaced path and revalidated, including executable permissions and the macOS signature. Its actual release executable is launched for a bounded startup check with isolated settings/data directories, then the exact owned test process is stopped. This checks loading and startup, not listening or clean-system behavior.

## Dependency Notices

The existing advisory policy is retained without additional ignores. The three retired Linux D-Bus package exceptions remain maintenance-only; Linux packaging now requires review of their actual Linux graph rather than relying on their exclusion from Windows. Existing quick-xml exceptions affect build-time Wayland protocol parsing, not application XML parsing.

Unix AccessKit notices retain both upstream Apache and Chromium BSD texts by the same reviewed checksums. The fixed macOS MIT fallback group is limited to ten exact versions in the template. It preserves published MIT declarations and author metadata, the original objc2-family MIT notice with Steven Sheldon attribution, and complete MIT terms. Other unresolved placeholders still fail validation. The old upstream notice SHA-256 is `e353f37b12aefbb9f9b29490e837cfee05d9bda70804b3562839a3285c1df1e5`; newer objc2 root license declarations reviewed at the recorded revisions have SHA-256 `7f976f7e9cb2d87df7230606feb932c3f21ac0e664045a775b600046ff850c54`. No invented copyright statement is substituted for manifest author metadata.

The Linux Plasma binding crate declares MIT, but its bundled protocol specifications also carry LGPL-2.1-or-later and other notices. Its clarification retains MIT plus full LGPL text by checksum; specification copyright blocks are appended from bounded XML using a DTD-prohibited parser. `CORRESPONDING-SOURCE.tar.gz` includes the complete tagged application, the exact original Plasma crate including those specifications/licenses, and [rebuilding/replacement instructions](REBUILD-LINUX.txt). The crate archive checksum `2b6d8cf1eb2c1c31ed1f5643c88a6e53538129d4af80030c8cabd1f9fa884d91` matches the locked registry source. Application source is supplied for rebuilding with a modified library rather than restricting replacement or debugging.

## Publication

Download each successful workflow artifact. Inspect the actual archive, notices, sources, SBOM, dependencies, modes/signatures, and complete diagnostics; verify its `.sha256`. Scan the assets for secrets/private paths. Attach only the new Linux/macOS archives and their uniquely named checksums to the existing public prerelease. Do not change `SHA256SUMS.txt` or the Windows ZIP. Update the product-focused [release notes](preview-notes.md) and README download links, then anonymously download the new assets and verify that their hashes match the CI originals.
