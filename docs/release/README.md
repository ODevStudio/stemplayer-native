# Build evidence and local support

This is the maintainer contract for unsigned build evidence and the native support command. Release approval remains governed by [Status](../../STATUS.md) and the [Phase 11 plan](../plans/PLAN_11.md). Update this document with changes to the tools or report versions; it is not a qualification report.

## Build identity

The application build script creates a version-1, sorted JSON identity containing the package/version, source commit, dirty-source indicator, compiler version, pinned toolchain channel and file hash, Cargo lock hash, target, profile, and sorted package features. No wall-clock timestamp, host name, source directory, environment dump, or publisher identity is embedded. Source archives or unavailable Git metadata have explicit null source fields, never a guessed revision.

On Windows, the same JSON is embedded as resource type RCDATA, ID 2, in the player and support executables. The console support executable also embeds the JSON on every build target. This does not add a console window to the graphical player. The collector requires the exact identity bytes in both Windows executables and binds their hashes in its manifest; it rejects stale or unrelated binaries.

The support executable is built alongside the player by the existing package build. `cargo run -p stemplayer` still selects the graphical player. No dependency versions, lock entries, runtime services, telemetry, installers, or update behavior are changed. Build hashing reuses the existing format crate's SHA-256 implementation.

## Native support command

From a matching build:

```powershell
cargo build --locked --release -p stemplayer
target/release/stemplayer-support.exe --build-info
target/release/stemplayer-support.exe --report --output support-report.json
target/release/stemplayer-support.exe --benchmark --iterations 4 --output support-benchmark.json
```

Omitting `--output` writes JSON to standard output. Files are created exclusively and are never overwritten; an incomplete file is removed after a reported write failure. Reports are limited to 64 KiB. Command errors do not echo arguments or filesystem paths. Exit status 0 indicates command success, and 2 indicates invalid arguments or a failed export/self-check.

The report is deliberately limited to build identity and an explicit collection allowlist. It does not inspect settings, recent sources, media, URLs, environment variables, devices, or another running player. It is not a live playback diagnostic dump. Reports are never uploaded by the command. Users may inspect and share them explicitly.

The benchmark exercises the real synchronized source ring with eight deterministic channels, 32,768 frames per iteration, and 256-frame blocks. Iterations are bounded to 1–64. It verifies every sample, frame counter, and overflow counter and reports a stable little-endian Float32 SHA-256 fingerprint. The generator uses integer arithmetic and exactly representable binary fractions; its reference hash is checked by the CLI tests.

Timings include sample generation, allocations, ring operations, comparisons, and encoding. Timing observations are not deterministic and have no release performance threshold. They do not measure decoder throughput, first-audio latency, audible quality, callback deadlines, hardware underruns, or long-run drift. The benchmark never opens an audio device, source file, or network connection.

## Collect Windows build evidence

Use a clean checkout, the pinned Rust toolchain, a default product release build, and Cargo metadata generated for that same build target and feature selection. CI shows the canonical path-remapping setup in [ci.yml](../../.github/workflows/ci.yml). Unmapped private build paths cause the content scan to fail.

The following commands use PowerShell and preserve JSON as UTF-8 rather than relying on shell redirection defaults:

```powershell
New-Item -ItemType Directory -Force artifacts/tmp | Out-Null
cargo build --locked --release -p stemplayer
target/release/stemplayer-support.exe --build-info --output artifacts/tmp/build-info.json
$build = Get-Content artifacts/tmp/build-info.json -Raw | ConvertFrom-Json
$metadata = cargo metadata --locked --offline --format-version 1 --manifest-path apps/stemplayer/Cargo.toml --filter-platform $build.target
if ($LASTEXITCODE -ne 0) { throw "Cargo metadata failed" }
[IO.File]::WriteAllText("artifacts/tmp/release-cargo-metadata.json", ($metadata -join "`n"), [Text.UTF8Encoding]::new($false))
python scripts/release_evidence.py collect --build-info artifacts/tmp/build-info.json --metadata artifacts/tmp/release-cargo-metadata.json --binary target/release/stemplayer.exe --support-binary target/release/stemplayer-support.exe --output artifacts/unsigned-build-evidence
python scripts/release_evidence.py verify artifacts/unsigned-build-evidence
```

The output directory must not already exist. Keep metadata and evidence under the ignored `artifacts/` directory. Raw Cargo metadata contains machine paths and is never copied into the evidence package. Python 3.11 or newer is a maintainer/CI requirement only; neither native executable needs Python or Cargo at runtime. Collection and verification perform no network requests. Dependencies must already be present locally for offline Cargo metadata.

## Evidence package

| File | Contract |
| --- | --- |
| `bin/stemplayer.exe`, `bin/stemplayer-support.exe` | Explicit, bounded native payload allowlist; no implicit directory copying. |
| `build-info.json` | The common embedded build identity. |
| `sbom.cdx.json` | CycloneDX 1.6 source inventory with package URLs, declared license expressions, dependency edges, and registry archive hashes from the lockfile. |
| `THIRD-PARTY-NOTICES.txt` | Actual license/notice texts from resolved source packages, including license directories. Missing text fails collection. |
| `release-manifest.json` | Source/build inputs and payload/document hashes; explicitly unsigned, unattested, and not qualified. |
| `artifact-scan.json` | Redacted scan rules, checked filenames, and limitations. |
| `SHA256SUMS` | Hashes of all other package files. It excludes itself. |

The inventory follows normal and build dependency edges and excludes dev-only edges. Cargo metadata can conservatively unify dependency features; this is a source/build dependency inventory, not proof that every component is linked into the executable. The supplied metadata must come from the documented target-filtered command. The collector checks the root package/version, exact package feature set, source revision, clean worktree, lock/toolchain hashes, approved dependency sources, graph completeness, and registry checksums. It checks the source again before publishing the local directory.

License alternatives remain declared alternatives, not an automated selection of distribution terms. `cargo deny check` continues enforcing the repository dependency policy. The notice bundle does not waive license review or prove that every distribution obligation has been fulfilled.

## Security and verification boundary

Reads, package counts, license sizes, binary sizes, and staging contents are bounded. Final symlinks and Windows reparse-point inputs are rejected. The collector rejects private-key material, selected high-confidence token signatures, private home/build paths, explicit build-root markers, common debug signatures, and unexpected staged files such as runtimes, media, or archives. UTF-16LE strings are inspected as well as byte strings. Failures identify a rule and safe artifact name, not the matched data or source path.

This is a conservative heuristic scanner, not a comprehensive secret detector, PE verifier, malware scanner, or proof against deliberately concealed content. Verification checks the complete file set, checksums, manifest/payload consistency, embedded identity, and content rules again. Checksums detect changes relative to the supplied manifest; they do not authenticate an attacker-replaced package. The provenance record is local evidence, not a signed attestation or reproducible-build claim.

CI runs the Python regression suite, Rust CLI tests, native report/benchmark smoke commands, and Windows collection/verification. It does not upload or publish these artifacts, select release storage, sign binaries, create an installer, or mark a platform qualified. Full signing, clean-system, accessibility, performance, and installer gates remain separate.

Run the [verification guide](../agent/verification.md) for shared and release-sensitive changes, plus:

```powershell
python -m unittest discover -s scripts -p "test_*.py"
cargo test --locked -p stemplayer-format
cargo test --locked -p stemplayer --test support_cli
```
