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

Both offline and online cargo-about 0.9.2 collection completed, but neither resolved the copyright/attribution prerequisite. The selected-license output still uses `Copyright (c) <year> <copyright holders>` for packages including AccessKit, audio-core, realfft, and zune-inflate. A zero tool exit is not approval of these notices.

The current realfft repository still lacks a license file, and its existing issue #45 concerns adding an Apache alternative, with no authoritative release-specific copyright text supplied. Its Cargo/README MIT declaration alone does not close the evidence required by the [runbook](../release/windows-preview.md#upstream-notice-prerequisite). The earlier missing-notice inventory also remains open; no invented copyright statement or broader license allowance was added. Raw collector output remains ignored local evidence.

No public upstream issue or comment was posted. This remains a free hobby preview under the existing royalty-free Slint route; no paid or commercial licensing arrangement was introduced. Redistribution notice requirements remain a separate check from whether downloads are sold.

## Unexecuted Gates

No refreshed candidate ZIP was created while notice obligations remain unresolved. No private-repository hosted run was dispatched; enabling Actions does not confirm or consume an allowance by itself. No repository visibility change, release tag, signing, or publication occurred. Source review/freeze and exact-commit CI packaging must follow notice resolution.

Clean Windows, extracted-candidate launch, human keyboard/screen-reader/high-DPI tasks, local/HLS listening, offline behavior, device loss/recovery, suspend/resume, one-hour playback, and candidate performance budgets remain `not_tested` for the current candidate. Earlier measurements do not qualify changed code. Phase 11 and Phase 12 remain incomplete.
