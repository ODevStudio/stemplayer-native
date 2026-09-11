# Hostile-input fuzzing

This repository uses two complementary fuzzing layers for parser and recovery boundaries. The normal Rust test suite contains deterministic mutation smoke tests that must stay fast enough for ordinary pull requests. Coverage-guided libFuzzer targets live in the isolated `fuzz/` workspace and run only when explicitly requested.

This is security-testing infrastructure, not qualification evidence by itself. A fuzz run proves only that the exercised build did not hit a sanitizer finding, panic, abort, timeout, or configured resource limit during that run.

## Covered surfaces

| Target | Production boundary |
| --- | --- |
| `studio_manifest` | Studio manifest JSON plus safe filename normalization |
| `local_containers` | RIFF/WAV inspection and MP4 container structure inspection through `BoundedReader` |
| `hls_json` | `stem-hls-v1` manifest and waveform JSON |
| `hls_playlist` | Static VOD media-playlist parsing, durations, segment references, and timeline math |
| `hls_url_policy` | Entry URL and child-reference scheme/origin/path policy using the production URL-policy source |
| `settings` | Versioned local-settings loading and corrupt/oversized recovery |
| `waveform_cache` | Waveform-cache binary decoding, both raw corruption and payloads with recomputed SHA-256 trailers |

The ordinary workspace tests exercise the same boundary classes with deterministic mutation campaigns so parser hardening is covered on every normal all-feature test run.

## Local coverage-guided run

`cargo-fuzz` requires a nightly Rust toolchain and is not a production/runtime dependency. The fuzz workspace is intentionally independent, so its generated lockfile, corpus, artifacts, and target directory do not modify the main workspace or `Cargo.lock`.

```bash
rustup toolchain install nightly-2026-09-01 --profile minimal
cargo install --locked cargo-fuzz --version 0.13.2
cd fuzz
cargo +nightly-2026-09-01 fuzz run studio_manifest -- -max_total_time=60 -timeout=5 -rss_limit_mb=2048 -max_len=4194304
```

Replace `studio_manifest` with another target from the table. Keep ad-hoc runs bounded unless a dedicated machine and budget are available. Individual targets apply stricter internal input caps where the production boundary is intentionally smaller.

## GitHub Actions budget

The existing `native-rust` workflow exposes fuzzing only through `workflow_dispatch`. Choose one fuzz target and a duration from 10 through 300 seconds. Fuzz mode runs the small workflow-integrity job plus one Ubuntu fuzz runner and skips the normal Ubuntu/Windows/macOS quality matrix for that dispatch. It is never scheduled automatically and never adds a runner to ordinary pull requests.

Routine pull requests still exercise the deterministic mutation regressions through `cargo test --locked --workspace --all-features` on the normal free-tier Ubuntu quality runner.

## Corpus and crash handling

Do not commit private media, URLs containing credentials, machine paths, or third-party files as fuzz seeds. Prefer small synthetic inputs produced from the documented formats. Generated `fuzz/corpus/` and `fuzz/artifacts/` stay ignored.

When libFuzzer finds a failure:

1. Reproduce the artifact locally against the same revision and target.
2. Minimize it with `cargo fuzz tmin` when practical.
3. Convert the smallest legal synthetic reproducer into an owning-crate regression test.
4. Fix the affected parser/recovery boundary without widening accepted input unless the format contract is deliberately changed.
5. Run the relevant package test, shared verification tier, and the fuzz target again.

Do not attach private crash artifacts or raw machine paths to public issue text.

## Invariants

Fuzz targets must keep input sizes explicit and bounded, must not make external network requests, and must not depend on audio hardware, UI state, signing credentials, or private fixtures. File-backed fuzz targets may use process-scoped temporary files because the production WAV/MP4, settings, and cache contracts include filesystem identity and bounded regular-file checks.

A new parser or untrusted serialization boundary should receive both a deterministic malformed-input regression layer and a coverage-guided target when the boundary can be exercised without unsafe external side effects.
