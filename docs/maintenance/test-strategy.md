# Test strategy

Tests are executable contracts. Add coverage at the narrowest stable boundary that can reproduce the defect or requirement.

## Existing layers

- Inline unit tests cover bounded values, state transitions, scheduling, mixing, output, platform behavior, presentation, networking, waveform analysis and cache, controllers, and playlist behavior.
- Integration tests cover the probe CLI, decode/resource behavior, committed synthetic audio fixtures, Studio bundle/manifest validation, and deterministic hostile-input mutation campaigns.
- The isolated `fuzz/` workspace provides coverage-guided libFuzzer targets for untrusted parser/recovery boundaries; see [Hostile-input fuzzing](fuzzing.md).
- PowerShell and Python scripts cover Windows hardware, UI/NVDA, resource, waveform/cache, and HLS qualification.

Inspect existing coverage before adding a parallel harness.

## Placement

| Behavior | Preferred test |
| --- | --- |
| Pure value/state/function | inline unit test in the owning crate |
| Public crate boundary or fixture | crate integration test under `tests/` |
| Product controller/playlist behavior | app Rust test with deterministic fake dependencies |
| CLI schema and exit code | `stemplayer-probe` integration test |
| Malformed file/manifest/container | synthetic bytes or generated temporary fixture plus deterministic mutations |
| Untrusted parser/serialization boundary | fast owning-crate mutation regression plus an isolated coverage-guided fuzz target when side-effect safe |
| Network policy | fake transport unit test plus a small loopback integration test where mocks cannot prove behavior |
| Real-time callback rule | output test plus callback audit evidence |
| UI accessibility or OS integration | focused Windows qualification, not a generic unit test |
| Installer/signature/upgrade | clean-system Phase 11 qualification |

## Required CI coverage

CI must cover every workspace member, not only `default-members`, and should include all-feature workspace tests, clippy for all workspace targets/features, explicit minimal/no-default feature compilation, Windows and Ubuntu where platform compilation differs, a Windows release-profile product build, documentation integrity checks, workflow lint, and dependency policy.

Coverage-guided fuzzing is manual and quota-bounded rather than a required pull-request job. Ordinary all-feature workspace tests must run the deterministic hostile-input regressions so parser hardening does not depend on nightly Rust or additional hosted runners.

## Compatibility policy

The workspace declares Rust 1.97 and `rust-toolchain.toml` pins 1.97.1. Keep those values aligned; the repository does not claim compatibility with older Rust releases. The isolated fuzz workspace may use a separately pinned nightly toolchain only for libFuzzer instrumentation; that does not change product compiler support.

## High-value gaps to evaluate

- End-to-end synthetic HLS loopback behavior beyond fake transport coverage.
- Redirect revalidation, response and segment bounds, cancellation, and segment continuity.
- UI compile/smoke coverage that does not require unreliable hosted-runner interaction.
- Manual workflow entry points for existing hardware and qualification scripts.
- Persisting only minimized, legal synthetic fuzz regressions that add durable coverage without committing raw corpora.

## Fixtures and evidence

Use original synthetic fixtures and bounded temporary files. Do not commit third-party recordings, private media, raw reports, fuzz corpora, crash artifacts, screenshots, credentials, absolute machine paths, or unredacted URLs.

A test name should state the invariant or regression. A security fix needs a failing-before/passing-after regression test at the affected boundary.
