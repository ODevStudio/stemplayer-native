# Windows Preview Runbook

This is the maintainer procedure for an unsigned portable preview, not a signed installer or stable-release qualification. [STATUS.md](../../STATUS.md) records remaining blockers. Start at `0.1.0-preview.1`, retain the existing product identity and default `product` features, and target Windows 11 24H2 or newer, x64 only.

## Build a Candidate

Merge reviewed changes into `master`, then manually dispatch the existing `native-rust` workflow on that commit. The packaging job runs only for manual dispatch on `master`, after the integrity, quality, and dependency-policy jobs pass. It uses a fresh build directory, no restored build cache, read-only permissions, and pinned tools. It uploads only a ZIP and `SHA256SUMS.txt`; it never publishes a release.

For a private repository on a free account, confirm the included Actions allowance before dispatching the multi-platform workflow. Enabling Actions alone does not run a build. Local verification does not replace the required CI artifact or authorize a visibility change.

For local reproduction, use a clean committed checkout, the pinned Rust toolchain, Windows, and PowerShell 7 or newer:

```powershell
cargo install --locked --debug cargo-about --version 0.9.2 --features cli --root artifacts/release-tools
cargo install --locked --debug cargo-cyclonedx --version 0.5.9 --root artifacts/release-tools
pwsh -NoProfile -File scripts/test-windows-preview.ps1
pwsh -NoProfile -File scripts/package-windows-preview.ps1
```

The packaging script builds `cargo build --locked --release -p stemplayer --target x86_64-pc-windows-msvc`. It preserves the release profile and uses packaging-scoped compiler flags for static CRT linkage and workspace/Cargo source-path remapping. MSVC Build Tools with `vswhere` and `dumpbin` are required on the build machine, not the user's PC. The import check rejects non-system DLLs, including VC redistributable DLLs. The script records the default product feature set and rejects dirty source trees, a changed source revision before archiving, non-preview versions, incomplete inputs, changed Slint terms, mismatched build identity, or existing release assets. Versioned output is under ignored `artifacts/windows-preview/`; temporary build and packaging-check directories are local evidence, not release assets.

Build-only notice/SBOM tools use debug builds to avoid spending the fresh runner's time optimizing tools that are not shipped. The application still uses the unchanged release profile. The packaging job allows 90 minutes for the clean build.

The ZIP has one versioned folder containing only `stemplayer.exe`, `README.txt`, both project license files, `THIRD-PARTY-NOTICES.txt`, `sbom.cdx.json`, and `BUILD-INFO.json`. Build information records source commit, compiler, target, features, lock/binary hashes, and an explicitly unqualified unsigned state.

Notices cover the selected Windows product graph, including build-time dependencies. Registry URLs identify unmodified upstream sources. The scoped [AAC patch](../../vendor/symphonia-codec-aac/PATCHES.md) uses MPL-2.0 and requires complete modified sources at the exact public repository commit in `BUILD-INFO.json`; the script appends that source URL and rejects a mismatched notice. Do not distribute while this source location is private or unavailable. Slint uses the royalty-free 2.0 application license, not its GPL or commercial alternatives. cargo-about 0.9.2 omits matching LicenseRef text despite successful clarification; the script verifies the Slint license SHA-256 and appends that exact text. Collection uses network access for checksum-bound upstream clarifications. Unexpected collector warnings, including failed clarifications with a zero tool exit, block packaging. Do not dismiss other unresolved license failures. Re-review these clarifications and the supplemental template grants when dependencies change.

cargo-cyclonedx 0.5.9 writes per-workspace-member files and embeds absolute local package IDs. The wrapper uses unique filenames, retains only the product SBOM, removes its temporary files, and replaces local package IDs, including the non-workspace vendored decoder, with stable references while preserving dependency links. It runs offline after fetching and checks that the lockfile did not change. Its locked installation currently warns about a yanked build-tool-only `xml-rs 0.8.19`; this is not a shipped dependency or a new product advisory exception, and the generator consumes Cargo metadata and emits JSON rather than parsing untrusted XML. Review this tool warning before publication; do not remove `--locked` to hide it.

## Upstream Notice Prerequisite

Dependency/license review is mandatory. [Native PR #1](https://github.com/ODevStudio/stemplayer-native/pull/1#issuecomment-5774733220) recorded 25 Windows-target packages without packaged license texts. Successful `cargo-about --fail` generation alone does not resolve notice review: the tool can fall back to canonical SPDX text without preserving available crate-specific attribution.

- `accesskit 0.24.1` omits license files from the cached crate. Its recorded upstream revision is [`a55d3e1a18bb9ef0e4bccc9083fb13c3e0ad8969`](https://github.com/AccessKit/accesskit/tree/a55d3e1a18bb9ef0e4bccc9083fb13c3e0ad8969), with the crate under `common/`. The reviewed clarifications now bind its root `LICENSE-APACHE` and `LICENSE.chromium` texts by checksum, including the Chromium BSD obligation omitted by generic MIT fallback. The same review covers the locked consumer/Windows/winit crates at their recorded revisions.
- `realfft 3.5.0` explicitly declares MIT in its manifest and README at [`d0d4eee0525fd27c96c8a046d6d107acd5ed84a6`](https://github.com/HEnquist/realfft/tree/d0d4eee0525fd27c96c8a046d6d107acd5ed84a6); the [upstream owner's statement](https://github.com/HEnquist/realfft/issues/45) also confirms MIT. The reviewed notice uses that declaration, checksum-bound README (`1e92c54e7fa08298d2a34ea9aa63b85626c47519873a91cd1028f234e1221608`), upstream author metadata explicitly labeled as metadata, and standard MIT terms. Neither the published crate nor that source tree supplies a separate copyright notice. Do not invent one from the authors field or ship the collector's placeholder. The template handles only this exact version and single-component MIT section; other notices are unchanged.

Review the remaining reported packages against the exact Windows candidate graph and retain verified provenance for every required notice. A fix for these two examples alone does not close the earlier inventory. Keep the accepted-license policy and missing-license failures intact; publication requires review of the actual candidate notices, not a successful tool exit alone.

The package validator rejects the pinned collector's synthesized MIT copyright placeholder and requires both AccessKit's Chromium attribution and realfft's exact upstream declaration. This catches observed collector defects; it does not replace review of the candidate notices. The [audit continuation](../qualification/windows-preview-audit-continuation.md#notice-repairs-and-public-ci) records the earlier upstream inventory. The accepted-license policy and missing-license/collector-diagnostic failures remain enforced.

## Candidate Qualification

Download the successful workflow artifact and retain the ZIP bytes unchanged. Compare its SHA-256 to `SHA256SUMS.txt` with `Get-FileHash -Algorithm SHA256`. A checksum detects changed downloads, not publisher identity. Extract a test copy with Windows tools and record the version, commit, target, feature set, compiler, ZIP/binary/lock hashes, Windows build, test date, and operator. Store raw evidence outside source control; commit only a concise redacted summary when approved.

Automated build, integrity, dependency/license, archive, and checksum checks are mandatory before experimental preview publication. The manual checks below remain qualification work, not implied passes: unavailable manual environments do not turn the preview into a qualified or stable release. Known security, license, corruption, accessibility, unexpected-resume, or unbounded-resource failures still block distribution.

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

After the mandatory automated and asset checks pass, use the exact CI source commit and artifact, never a newly rebuilt binary. Check that version, tag, artifact name, and build information agree. Preserve the artifact in approved release storage before workflow retention expires. Keep manual qualification status accurate in `STATUS.md`; release notes describe the program without claiming unexecuted checks passed.

1. Tag the exact tested commit `v0.1.0-preview.1` and push that tag without moving an existing tag.
2. Create or update the GitHub prerelease for that tag. Attach only the original candidate ZIP and matching `SHA256SUMS.txt`.
3. Use the product-focused [preview release notes](preview-notes.md), the exact commit in `BUILD-INFO.json`, and the separate checksum asset. Keep the Slint badge visible on the actual public download page. Do not claim stable qualification or unexecuted checks passed.
4. Download the published assets and compare hashes against the verified originals. Confirm the release is public and marked as a prerelease, not a stable/latest release, with the unsigned-warning and privacy-conscious issue instructions intact.
5. Update `STATUS.md` with actual preview evidence without marking Phase 11/12 complete. Do not commit raw output or machine-specific information.

Updates are manual. Keep published assets and tags immutable; a fix increments the preview version and repeats affected gates. For an unsafe release, withdraw access to its assets, publish an advisory with affected versions and mitigation, and issue a new version. Do not silently replace a binary or ask users to disable protection. No telemetry or scheduled monitoring is added; the maintainer handles opt-in issues and private security reports.
