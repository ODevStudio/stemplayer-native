# Windows Hobby Preview Runbook

This is the maintainer procedure for an unsigned portable preview, not a signed installer or stable-release qualification. [STATUS.md](../../STATUS.md) records remaining blockers. Start at `0.1.0-preview.1`, retain the existing product identity and default `product` features, and target Windows 11 24H2 or newer, x64 only.

## Build a Candidate

Merge reviewed changes into `master`, then manually dispatch the existing `native-rust` workflow on that commit. The packaging job runs only for manual dispatch on `master`, after the integrity, quality, and dependency-policy jobs pass. It uses a fresh build directory, no restored build cache, read-only permissions, and pinned tools. It uploads only a ZIP and `SHA256SUMS.txt`; it never publishes a release.

For local reproduction, use a clean committed checkout, the pinned Rust toolchain, Windows, and PowerShell 7 or newer:

```powershell
cargo install --locked cargo-about --version 0.9.2 --features cli --root artifacts/release-tools
cargo install --locked cargo-cyclonedx --version 0.5.9 --root artifacts/release-tools
pwsh -NoProfile -File scripts/test-windows-preview.ps1
pwsh -NoProfile -File scripts/package-windows-preview.ps1
```

The packaging script builds `cargo build --locked --release -p stemplayer --target x86_64-pc-windows-msvc`. It preserves the release profile and uses packaging-scoped compiler flags for static CRT linkage and workspace/Cargo source-path remapping. MSVC Build Tools with `vswhere` and `dumpbin` are required on the build machine, not the user's PC. The import check rejects non-system DLLs, including VC redistributable DLLs. The script records the default product feature set and rejects dirty source trees, a changed source revision before archiving, non-preview versions, incomplete inputs, changed Slint terms, mismatched build identity, or existing release assets. Versioned output is under ignored `artifacts/windows-preview/`; temporary build and packaging-check directories are local evidence, not release assets.

The ZIP has one versioned folder containing only `stemplayer.exe`, `README.txt`, both project license files, `THIRD-PARTY-NOTICES.txt`, `sbom.cdx.json`, and `BUILD-INFO.json`. Build information records source commit, compiler, target, features, lock/binary hashes, and an explicitly unqualified unsigned state.

Notices cover the selected Windows product graph, including build-time dependencies. MPL component notices include exact unmodified crate source download URLs. Slint uses the royalty-free 2.0 application license, not its GPL or commercial alternatives. cargo-about 0.9.2 omits matching LicenseRef text despite successful clarification; the script verifies the Slint license SHA-256 and appends that exact text. Do not dismiss other unresolved license failures. Re-review these clarifications when dependencies change.

cargo-cyclonedx 0.5.9 writes per-workspace-member files and embeds absolute local package IDs. The wrapper uses unique filenames, retains only the product SBOM, removes its temporary files, and replaces local package IDs throughout the parsed JSON with stable workspace references while preserving dependency links. It runs offline after fetching and checks that the lockfile did not change. Its locked installation currently warns about a yanked build-tool-only `xml-rs 0.8.19`; this is not a shipped dependency or a new product advisory exception, and the generator consumes Cargo metadata and emits JSON rather than parsing untrusted XML. Review this tool warning before publication; do not remove `--locked` to hide it.

## Upstream Notice Prerequisite

The dependency/license review required by [STATUS.md](../../STATUS.md) remains a publication blocker. [Native PR #1](https://github.com/ODevStudio/stemplayer-native/pull/1#issuecomment-5774733220) recorded 25 Windows-target packages without packaged license texts. Successful `cargo-about --fail` generation does not resolve this prerequisite: the tool can fall back to canonical SPDX text without crate-specific copyright or attribution evidence.

- `accesskit 0.24.1` omits license files from the cached crate. Its recorded upstream revision is [`a55d3e1a18bb9ef0e4bccc9083fb13c3e0ad8969`](https://github.com/AccessKit/accesskit/tree/a55d3e1a18bb9ef0e4bccc9083fb13c3e0ad8969), with the crate under `common/` and `LICENSE-MIT`, `LICENSE-APACHE`, and `LICENSE.chromium` at the repository root. Review the notices applicable to the distributed sources and bind the approved exact texts to this crate version before treating its notice coverage as complete.
- `realfft 3.5.0` declares MIT in its manifest and README, but neither the cached crate nor its recorded upstream tree at [`d0d4eee0525fd27c96c8a046d6d107acd5ed84a6`](https://github.com/HEnquist/realfft/tree/d0d4eee0525fd27c96c8a046d6d107acd5ed84a6) includes a license file. Obtain authoritative release-specific license text and copyright/attribution evidence from upstream, or an approved legal resolution. Do not invent a copyright notice from the Cargo authors field.

Review the remaining reported packages against the exact Windows candidate graph and retain verified provenance for every required notice. A fix for these two examples alone does not close the earlier inventory. Keep the accepted-license policy and missing-license failures intact; publication requires review of the actual candidate notices, not a successful tool exit alone.

## Candidate Qualification

Download the successful workflow artifact and retain the ZIP bytes unchanged. Compare its SHA-256 to `SHA256SUMS.txt` with `Get-FileHash -Algorithm SHA256`. A checksum detects changed downloads, not publisher identity. Extract a test copy with Windows tools and record the version, commit, target, feature set, compiler, ZIP/binary/lock hashes, Windows build, test date, and operator. Store raw evidence outside source control; commit only a concise redacted summary when approved.

- [ ] Run the [Tier 2 and release checks](../agent/verification.md), product tests, packaging check, and `cargo deny check`. Review the explicit advisory exceptions; do not widen them to make a release pass.
- [ ] Inspect archive entries, notices, SBOM identity/dependencies, and executable imports. Scan the actual assets for secrets, private source paths, private fixtures, raw qualification data, debug symbols, and unwanted runtimes. Preserve and review complete diagnostics. A dependency inventory or hosted build alone is insufficient evidence.
- [ ] On a clean Windows 11 x64 VM or physical machine without Rust, Visual Studio, extra redistributables, or other development tools, extract into a Unicode/spaced standard-user path and launch without elevation. Missing DLLs or other runtime requirements block publication; do not obtain DLLs from unofficial websites.
- [ ] Verify supported ordinary audio, Studio WAV, stem MP4/M4A, and explicit `stem-hls-v1`; exercise open/play/pause/stop/seek, four-stem gain/mute, master-reference behavior, playlists, waveform/cache behavior, and errors. Use approved synthetic or locally authorized fixtures only.
- [ ] Verify local playback offline, no startup network access or automatic updater, settings persistence without restored-source autoplay, and the documented per-user cache/data paths. Remove the extraction without affecting unrelated files; test preservation/reset and rollback using backed-up settings.
- [ ] Complete core tasks using only the keyboard and using a screen reader. Check focus after dialogs/errors, high DPI/large text, contrast, and clear device/format errors.
- [ ] On a real audio-capable Windows system, listen for clicks, channel/routing mistakes, and incorrect endings; test device disconnect/recovery and suspend/resume. Unexpected audible resume blocks publication.
- [ ] Complete one hour of normal playback with zero crashes, deadlocks, underruns, overflows, source corruption, or unbounded memory/queue/cache growth. Exercise seeks and playback while waveforms are analyzed.
- [ ] Measure against existing budgets using the [measurement methodology](../measurement/methodology.md), including idle/minimized resources, startup/playback/seek behavior, and sustained memory/handle growth. Do not lower budgets or pool away failures. Unexecuted measurements are `not_tested`, not PASS.

Use the existing focused Windows qualification scripts where they apply, but ensure the released executable itself is tested: scripts that rebuild another executable are supporting evidence, not a substitute. Full stable-release eight-hour/fuzz/installer/signing gates remain deferred, not passed. Security, license, core accessibility, corruption, and unbounded-resource failures cannot be accepted as preview limitations.

## Review Public Exposure

Do not change repository visibility before this review is complete. Review every branch/tag and full reachable history, releases/assets, issues/PRs and attachments, wiki/discussions where enabled, workflow logs/artifacts/caches, and repository metadata. Use an approved secret scanner in redacted mode across all refs, then manually inspect private media, screenshots, raw qualification output, machine paths, and personal information that secret scanners miss. Revoke exposed credentials before any visibility change; removal from the current tree is not enough. Resolve findings before publication and do not rewrite shared history without separate approval.

The maintainer changes the existing repository to public only after approving the exposure review. Confirm the public download page can be read without authentication and displays the required [Made with Slint badge](https://raw.githubusercontent.com/slint-ui/slint/v1.17.1/logo/MadeWithSlint-logo-whitebg.png). CI must never change visibility or hold a privileged publication token.

## Manual Publication

After all preview gates pass, use the tested source commit, never a newly rebuilt binary. Check that version, tag, artifact name, and build information agree. Preserve the artifact in approved release storage before workflow retention expires.

1. Tag the exact tested commit `v0.1.0-preview.1` and push that tag without moving an existing tag.
2. Create a draft GitHub prerelease for that tag. Attach only the original candidate ZIP and matching `SHA256SUMS.txt`.
3. Start with [preview release notes](preview-notes.md). Append the tested commit, hashes, Windows build, redacted qualification/performance results, and accepted non-blocking limitations. Keep the Slint badge visible on the actual public download page.
4. Download the draft assets and compare hashes against the tested originals. Review unsigned-warning wording and privacy-conscious issue instructions, then manually publish as a prerelease, not a stable/latest release.
5. Update `STATUS.md` with actual preview evidence without marking Phase 11/12 complete. Do not commit raw output or machine-specific information.

Updates are manual. Keep published assets and tags immutable; a fix increments the preview version and repeats affected gates. For an unsafe release, withdraw access to its assets, publish an advisory with affected versions and mitigation, and issue a new version. Do not silently replace a binary or ask users to disable protection. No telemetry or scheduled monitoring is added; the maintainer handles opt-in issues and private security reports.
