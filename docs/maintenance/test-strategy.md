# Test strategy

Tests are executable contracts. Add coverage at the narrowest stable boundary that can reproduce the defect or requirement.

## Existing layers

- Inline unit tests cover bounded values, state transitions, scheduling, mixing, output, platform behavior, presentation, networking, waveform analysis and cache, controllers, and playlist behavior.
- Integration tests cover the probe CLI, decode/resource behavior, committed synthetic audio fixtures, and Studio bundle/manifest validation.
- PowerShell and Python scripts cover Windows hardware, UI/NVDA, resource, waveform/cache, and HLS qualification.

Inspect existing coverage before adding a parallel harness.

## Placement

| Behavior | Preferred test |
| --- | --- |
| Pure value/state/function | inline unit test in the owning crate |
| Public crate boundary or fixture | crate integration test under `tests/` |
| Product controller/playlist behavior | app Rust test with deterministic fake dependencies |
| CLI schema and exit code | `stemplayer-probe` integration test |
| Malformed file/manifest/container | synthetic bytes or generated temporary fixture |
| Network policy | fake transport unit test plus a small loopback integration test where mocks cannot prove behavior |
| Real-time callback rule | output test plus callback audit evidence |
| UI accessibility or OS integration | focused Windows qualification, not a generic unit test |
| Installer/signature/upgrade | clean-system Phase 11 qualification |

## Required CI coverage

CI must cover every workspace member, not only `default-members`, and should include all-feature workspace tests, clippy for all workspace targets/features, explicit minimal/no-default feature compilation, Windows and Ubuntu where platform compilation differs, a Windows release-profile product build, documentation integrity checks, workflow lint, and dependency policy.

## Compatibility policy

The workspace declares Rust 1.97 and `rust-toolchain.toml` pins 1.97.1. Keep those values aligned; the repository does not claim compatibility with older Rust releases.

## High-value gaps to evaluate

- End-to-end synthetic HLS loopback behavior beyond fake transport coverage.
- Redirect revalidation, response and segment bounds, cancellation, and segment continuity.
- UI compile/smoke coverage that does not require unreliable hosted-runner interaction.
- Manual workflow entry points for existing hardware and qualification scripts.

## Fixtures and evidence

Use original synthetic fixtures and bounded temporary files. Do not commit third-party recordings, private media, raw reports, screenshots, credentials, absolute machine paths, or unredacted URLs.

A test name should state the invariant or regression. A security fix needs a failing-before/passing-after regression test at the affected boundary.
