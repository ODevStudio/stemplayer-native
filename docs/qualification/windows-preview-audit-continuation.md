# Windows Preview Audit Continuation

Date: 2026-10-22. Windows x64, Rust 1.97.1. Base source: `9f6c767980e0901e882ef8aa026ce1335988e807`, plus the approved local AAC patch. [STATUS.md](../../STATUS.md) owns current blockers. The 2026-10-21 preview ZIP predates the audit repairs and must not serve as the current candidate.

## Scope

The maintainer approved the decoder patch and enabling Actions. The [patch record](../../vendor/symphonia-codec-aac/PATCHES.md) identifies the checksum-verified upstream archive, the two changed source files, MPL terms, and removal trigger. No dependency version, runtime API, supported format, feature set, or settings schema changed. The lockfile replaces only the registry AAC package with that same-version local patch; remaining registry versions and checksums stay intact.

Packet-local seeding makes PNS independent of decoder history. It changes noise-substituted PCM from upstream; listening and broader codec qualification remain required. Both existing failing demonstrations now run in the ordinary suite. New tests cover signed/large timestamps and differing positions/channels. This is bounded work on the decoder worker, with no whole-file replay or PCM cache.

Preview notices now distinguish upstream sources from the modified AAC sources, binding the latter to the recorded build commit. SBOM path sanitization covers the vendored non-workspace package. Synthetic packaging checks cover those notices/references and reject the collector's observed placeholder copyright text. They do not qualify a binary or establish complete license compliance.

## Executed Checks

| Command | Result |
| --- | --- |
| `cargo test --locked -p stemplayer-decode --test aac_pns -- --include-ignored` before patch | Expected FAIL: 1 passed, 2 PCM-equivalence assertions failed |
| `cargo test --locked -p stemplayer-decode --test aac_pns` after patch | PASS: 5 passed, none ignored |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --locked --workspace --all-features --no-fail-fast` | PASS: 331 passed, 4 existing qualification tests ignored |
| `cargo test --locked -p stemplayer` | PASS: 97 passed, 1 existing device test ignored |
| `cargo test --locked -p symphonia-codec-aac` | PASS: 1 upstream invalid-channel regression |
| `cargo check --locked --workspace --no-default-features` | PASS |
| `cargo check --locked --workspace --all-features` | PASS |
| `cargo build --locked --release -p stemplayer --target x86_64-pc-windows-msvc` | PASS: source build only, without packaging-scoped CRT/path flags |
| `stemplayer.exe --ui-smoke 1000` on that source build | PASS: exit 0 with isolated per-user directories; not an extracted-candidate check |
| `cargo run --locked --release -p stemplayer-callback-audit` | PASS: zero allocations in 100,000 callbacks and 1,000 render iterations; automation p95 100 ns against 30,000,000 ns budget |
| `cargo deny --format json check` | PASS: exit 0, zero errors in each check summary |
| `cargo deny --target x86_64-pc-windows-msvc --format json check` | PASS: exit 0, zero errors in each check summary |
| `pwsh -NoProfile -File scripts/test-windows-preview.ps1` | PASS: synthetic payload, provenance, placeholder, reference, and archive checks |
| `python scripts/check_repository.py` | PASS |
| `python -m unittest discover -s scripts -p "test_*.py" -v` | PASS: 4 tests |
| `git diff --check` and untracked-file whitespace checks | Tracked/first-party changes pass; original upstream MPL text retains its trailing space at line 38 |

The first full-suite attempt failed while allocating a large temporary test fixture because the system temp volume had insufficient free space. The successful run used process-local `TEMP` and `TMP` under ignored workspace artifacts on a volume with sufficient space. No unrelated files were removed and no test assertion was relaxed.

All 15 imported files other than the two changed source files match the cached upstream release byte-for-byte. The original archive SHA-256 matches its former lockfile checksum. No first-party unsafe exception or advisory ignore was added. cargo-deny retains the existing duplicate/wildcard warnings and 20 workspace-dependency tool diagnostics; its successful summaries do not establish complete copyright/attribution coverage.

The full raw change diff and relevant diagnostics received review. The sole new whitespace diagnostic belongs to the unmodified upstream MPL license, which remains byte-identical rather than silently rewritten.

The real CycloneDX generator and sanitizer also ran against the current graph as analysis only. The 431-component SBOM contains 433 unique references, no broken dependency links or known private workspace/home paths, and the stable local AAC reference `workspace:symphonia-codec-aac@0.6.0`. No ZIP was created by this check. Raw SBOM/collector output and startup logs remain ignored.

## Notice Review

Initial offline and online cargo-about 0.9.2 collection completed, but neither resolved the copyright/attribution prerequisite. That selected-license output used `Copyright (c) <year> <copyright holders>` for packages including AccessKit, audio-core, realfft, and zune-inflate. A zero tool exit is not approval of these notices. The later repairs and remaining upstream dependency are recorded below.

The current realfft repository still lacks a license file, and its existing issue #45 concerns adding an Apache alternative, with no authoritative release-specific copyright text supplied. Its Cargo/README MIT declaration alone does not close the evidence required by the [runbook](../release/windows-preview.md#upstream-notice-prerequisite). The earlier missing-notice inventory also remains open; no invented copyright statement or broader license allowance was added. Raw collector output remains ignored local evidence.

No public upstream issue or comment was posted. This remains a free hobby preview under the existing royalty-free Slint route; no paid or commercial licensing arrangement was introduced. Redistribution notice requirements remain a separate check from whether downloads are sold.

## Unexecuted Gates

No refreshed candidate ZIP was created while notice obligations remain unresolved. No private-repository hosted run was dispatched; enabling Actions does not confirm or consume an allowance by itself. No release tag, signing, or binary publication occurred. Source review/freeze and exact-commit CI packaging must follow notice resolution.

Clean Windows, extracted-candidate launch, human keyboard/screen-reader/high-DPI tasks, local/HLS listening, offline behavior, device loss/recovery, suspend/resume, one-hour playback, and candidate performance budgets remain `not_tested` for the current candidate. Earlier measurements do not qualify changed code. Phase 11 and Phase 12 remain incomplete.

## Notice Repairs and Public CI

The maintainer subsequently authorized public source publication. Commit `dfbf9903a638a3acb85ee2b20e2a059bc53e048f` was pushed after the final exposure check; the staged redacted Gitleaks scan found no leaks. No history was rewritten. The first public [CI run](https://github.com/ODevStudio/stemplayer-native/actions/runs/39855938987) passed workflow integrity, dependency policy, and Ubuntu quality. It exposed a macOS-only test compilation error and a Windows packaging-check exit-code error.

The FIFO regression used `rustix::fs::mkfifoat`, which is unavailable on Apple targets. It now creates the same mode-600 FIFO with the native Unix `mkfifo` utility; the nonblocking-read assertion is unchanged. The packaging check now explicitly exits successfully only after all assertions finish. A GitHub-style wrapper reproduced the previous leaked native exit code 7 and passes after the fix. Unexpected license-collector warnings, including failed checksum clarifications that cargo-about can otherwise report with exit code 0, now fail packaging. The existing verified Slint text workaround is the sole diagnostic exception.

The current Windows collector graph has 376 packages. A fresh recursive inventory finds 24 registry packages with no packaged `LICENSE`, `COPYING`, or `NOTICE` file. This is the current graph, not an assertion that the earlier PR's 25-package count was unchanged after dependency updates. The reviewed upstream sources are:

| Packages | Recorded Upstream Revision | Notice Source |
| --- | --- | --- |
| `accesskit 0.24.1` | `a55d3e1a18bb9ef0e4bccc9083fb13c3e0ad8969` | `LICENSE-APACHE` and `LICENSE.chromium` |
| `accesskit_consumer 0.38.0`, `accesskit_windows 0.34.0`, `accesskit_winit 0.33.2` | `c88605b96d04431f9c3c792464a0f2f253480e94` | The same two upstream license blobs |
| `audio-core 0.2.1` | `10489fc46bea493755dcd0195b7d7d605e5b72a8` | `LICENSE-MIT`; published VCS metadata marks a dirty tree, so this is license provenance, not a claim of source identity |
| `clipboard-win 5.4.1` | `3b27cf2bfd1adcfa6e0264eb51c1025ddaf0f342` | `LICENSE`, BSL-1.0 |
| `const-field-offset 0.2.0`, `const-field-offset-macro 0.2.0`, `vtable 0.4.0`, `vtable-macro 0.4.0` | `443908daeed77db500781670448d3688e4be0b9a` | `LICENSES/Apache-2.0.txt`; select their Apache alternative, not Slint's application license |
| `dasp_sample 0.11.0` | `97c3bb9b2363c0b46ac1633858bf1054fd02a980` | `LICENSE-APACHE`; its exact attribution/grant is retained in the template because the collector cannot fetch its `.git`-suffixed repository URL |
| `integer-sqrt 0.1.5` | `282d0d4e26fd41d888894f5578b9d7553f7be0c8` | `LICENSE-MIT` |
| `lyon_algorithms 1.0.20` | `66b9ea7f4ab935763970e828781d8fd5e9305542` | `LICENSE-MIT` |
| `lyon_extra 1.1.0`, `lyon_geom 1.0.19`, `lyon_path 1.0.19` | `56219c95c658d4f39de7b190d98e57d5773ba241` | The same upstream MIT license blob |
| `profiling 1.0.18`, `profiling-procmacros 1.0.18` | `8271551172eb6fa4cba47369aedd93790c623df9` | `LICENSE-MIT` |
| `pulp-wasm-simd-flag 0.1.1` | `5eb07fd7b68edf0a5e19f71737d315f72a510295` | `LICENSE` |
| `rspolib 0.1.2` | `8170d99ebad29d894712ccb3dc6d0be2198d3055` | Parent repository `LICENSE` |
| `simd_helpers 0.1.0` | `ca1a2f84aa386d758e98f8a609d990263932fb85` | Immediate successor `82040194cd05affb060bf94d6f19f82a771d07fb` adds only `LICENSE`, with no source changes |
| `taffy 0.10.1` | `639f0acdd20f914f847abffbeaa903d66cae4882` | `LICENSE.md`, including original Visly attribution |
| `zune-inflate 0.2.54` | `69502ce83fdfecdd0beefd677e2abb3781b29d98` | Parent `LICENSE.md`; the exact upstream grant is retained in the template because the crate has no repository field |
| `realfft 3.5.0` | `d0d4eee0525fd27c96c8a046d6d107acd5ed84a6` | Still unresolved: no authoritative license text found |

The explicit clarifications in `about.toml` verify upstream text with SHA-256 and normally use the crate's recorded VCS revision. The single revision override is the verified license-only `simd_helpers` commit above. The template copies the dasp and zune grants with SHA-256 `756c8d2ab2dc24f256e1455b5f03937f4933d83d7bc9f2d55009fc962377d512` and `e870d7fd7819a9f0453c62798cb52ae95d148a8c91dc2de1d983f997143c72de`, respectively. The full selected Apache text is emitted by the collector. `dpi 0.1.2` and `libm 0.2.16` also have checksum-bound local clarifications so their bundled libm/musl copyright notices are not lost to canonical fallback. Prefer Apache for other dual-licensed packages; the accepted-license set and runtime dependency graph are unchanged.

Online collection with the final clarifications passed the new diagnostic check and produced 135 license sections. Only `realfft 3.5.0` still uses the rejected MIT placeholder. Chromium BSD text and source-specific upstream MIT notices are now present. This closes the identified collector defects, not final candidate notice review or the remaining realfft prerequisite. Packaging still stops; no new ZIP has been created.

The maintainer confirmed that no clean Windows environment is available. Windows Sandbox is not installed and this session lacks Hyper-V access. A hosted runner with development tools is not a substitute for the clean-system gate. No Windows feature installation, elevated VM configuration, or reboot was initiated.

The [replacement CI run](https://github.com/ODevStudio/stemplayer-native/actions/runs/39858357866) passed the Windows packaging check, Windows release build/startup/feature checks, and Ubuntu quality. macOS passed Clippy and the FIFO regression, then exposed two cache-operation busy errors during parallel tests. A Unix descriptor-lifetime regression reproduced the underlying problem deterministically under Ubuntu 24.04 WSL with Rust 1.97.1: closing the guard's descriptor alone leaves its file lock held while a duplicate remains open, as can happen during child-process creation. The new regression failed before the fix. Explicit Unix guard unlock now passes all 30 waveform tests and package all-target/all-feature Clippy with warnings denied. Directory binding, nonblocking acquisition, and cross-process contention assertions remain unchanged; Windows runtime code is unchanged. Full CI, including macOS execution, must confirm the follow-up.

A maintainer-only draft prerelease has been prepared for `v0.1.0-preview.1`, without assets or a published Git tag. It explicitly lists the outstanding upstream notice and manual qualification gates; it is not a published Windows release.
