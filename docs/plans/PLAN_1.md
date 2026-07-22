# Phase 1 — Independent Rust Workspace and Domain Contracts

## Goal

Create the native project’s compileable, testable foundation: a strictly layered Cargo workspace, stable domain types, command/snapshot APIs, error contracts, feature boundaries, and continuous-integration checks. No production audio or file decoding is required yet.

## Boundary

This phase creates only native-project files. It does not relocate, refactor, or share source with the webplayer. The webplayer’s concepts may inform domain values, but all native contracts are independently documented and tested.

## Entry criteria

- Phase 0 exit gate passed.
- UI, audio, decoder, and resampler direction decided or explicitly time-boxed.
- Supported platform baseline recorded.
- Native format and behavior specifications exist.

## Work package 1.1 — Workspace creation

Create the workspace described in `PLAN.md` with these dependency rules:

- `stemplayer-domain` has no UI, audio, network, filesystem, or decoder dependency.
- `stemplayer-timeline` depends only on domain-safe numeric utilities.
- `stemplayer-format` may depend on domain and bounded parser utilities.
- `stemplayer-decode` depends on domain, format, and codec adapters.
- `stemplayer-mixer` depends on domain and DSP-safe numeric utilities.
- `stemplayer-output` depends on domain, mixer-facing render structures, audio backend, and resampler adapters.
- `stemplayer-engine` orchestrates format, timeline, decode, mixer, and output through traits.
- `stemplayer-waveform` and `stemplayer-network` are independent services consumed by the controller.
- `stemplayer-ui` and `stemplayer-platform` depend on the controller/domain APIs, never the reverse.

Use workspace-level dependency versions and lints. Pin the Rust toolchain and commit `Cargo.lock` for application reproducibility.

## Work package 1.2 — Domain model

Define strongly typed values instead of passing paths, strings, or scalar options through loosely typed maps:

- `StemRole`: drums, bass, other, vocals.
- `TrackRole`: master, stem role, unknown.
- `OutputMode`: master reference or stem mix.
- `OutputPreference`: automatic, explicit master, explicit stem mix.
- `TransportState` and legal transitions.
- `SourceKind`: Studio WAV bundle, local stem container, HLS stemset.
- `SourceFrame`, `FrameCount`, `SampleRate`, `ChannelCount`, `Microseconds`, and generation identifiers.
- `GainDb` with validated range and exact special cases.
- `PlaybackPosition` with absolute frame as the authoritative value.
- Session metadata and public diagnostics.

Use newtypes for values that must not be mixed accidentally. Conversion functions must be checked and deterministic. Rational time calculations must not use accumulating floating-point clocks.

## Work package 1.3 — Mixer state contract

Implement only pure mixer-state derivation in this phase:

- Default controls.
- dB-to-linear conversion.
- Neutral-state detection.
- Mute-over-solo behavior.
- Multiple solo behavior.
- Automatic master/stem selection.
- Explicit output preference validation.
- Reset behavior.

Do not implement audio ramps yet. Property tests must cover all combinations of mute and solo across four roles, gain boundaries, master present/absent, and output preferences.

## Work package 1.4 — Transport state contract

Implement the state machine with explicit transition validation. Commands that are illegal from the current state return a structured error and do not mutate state.

Required properties:

- Closed is terminal.
- Inspection can reach ready or error only.
- Play from ready or ended enters priming first.
- Seek is allowed only from documented loaded states.
- Error recovery requires an explicit reset/unload path.
- State updates are serializable into immutable snapshots.

Tests enumerate every state-to-state pair and prove that the allowed-transition table is complete.

## Work package 1.5 — Command and snapshot API

Define an application-facing controller interface independent of the UI toolkit:

```rust
trait PlayerController {
    fn submit(&self, command: PlayerCommand) -> Result<CommandId, PlayerError>;
    fn latest_snapshot(&self) -> PlayerSnapshot;
    fn subscribe(&self) -> SnapshotSubscription;
}
```

The exact API may differ, but it must preserve these principles:

- Commands are typed and nonblocking from the UI thread.
- Long operations are cancellable.
- Snapshots are immutable, cheap to clone or share, and contain no decoder internals.
- Progress and diagnostics are coalesced.
- Snapshot delivery is bounded.
- UI code never reaches directly into audio or decoder objects.

Define commands for open, unload, play, pause, stop, seek, gain, mute, solo, reset, output preference, device selection, cache control, and shutdown. Unsupported commands return explicit feature errors until later phases implement them.

## Work package 1.6 — Error and diagnostic contracts

Create `PlayerError` with:

- Stable code.
- Human-readable summary.
- Optional user action.
- Optional redacted structured details.
- Source-chain retained internally for logs.
- Severity and recoverability classification.

Create diagnostic snapshots with bounded fields. Avoid strings that grow without limit. Counters use saturating or checked behavior. File paths and URLs are represented by redacted display names unless a user-facing action explicitly requires the full value.

## Work package 1.7 — Feature topology

Define Cargo features to prevent accidental runtime bloat:

- `local-wav`
- `local-mp4`
- `aac`
- `alac`
- `hls`
- `diagnostics-ui`
- platform-specific integration features

The production desktop binary enables the product set. The probe binary can enable format features without UI. Unit tests can use fake output and fake decoder features. Avoid deeply conditional domain types; feature gates should isolate implementations, not redefine semantics.

## Work package 1.8 — Quality gates

Configure:

- Formatting.
- Clippy with warnings denied in CI.
- Documentation tests where useful.
- Dependency audit and license reporting.
- Unsafe-code denial by default.
- Unit, property, and integration test tiers.
- Cross-compilation checks where practical.
- Minimum supported Rust version policy or an explicit “pinned stable only” policy.

Add architecture tests or scripts that detect forbidden dependency directions.

## Work package 1.9 — Headless probe skeleton

Create `stemplayer-probe` with commands such as:

```text
stemplayer-probe version
stemplayer-probe validate-config
stemplayer-probe inspect <source>
stemplayer-probe render-test <output.wav>
```

Only `version` and `validate-config` need full behavior now. Unimplemented commands return structured feature-not-ready errors. The probe becomes the deterministic integration and CI surface for later phases without requiring UI automation.

## Deliverables

- Compileable independent Cargo workspace.
- Domain and newtype crates.
- Pure mixer and transport contracts.
- Controller command/snapshot interface.
- Stable initial error taxonomy.
- Feature topology.
- Probe binary skeleton.
- CI for formatting, linting, tests, dependency policy, and platform compile checks.
- Architecture dependency diagram and crate responsibility document.

## Tests and evidence

- Exhaustive transport transition tests.
- Mixer combination property tests.
- Numeric conversion boundary tests.
- Snapshot boundedness tests.
- Command queue capacity tests using a fake controller.
- Build with all features, minimal features, and no default features where valid.
- Build with the webplayer repository absent.
- Dependency graph evidence proving the engine has no UI dependency.

## Resource checks

- Record debug and release binary sizes for the probe.
- Verify no background thread is created by the domain crates.
- Verify an idle mock controller creates only the documented thread count.
- Establish baseline test runtime and memory for later regression comparison.

## Risks and mitigations

- **Domain types mirror browser implementation too closely:** require native specifications to justify every field.
- **Feature flags create untestable combinations:** publish a supported feature matrix and test only meaningful sets.
- **Command API leaks implementation details:** review it against a mock engine and a second hypothetical UI.
- **Premature async-runtime dependency:** use traits and bounded channels first; introduce runtimes only where required.

## Exit gate

Phase 1 is complete when the independent workspace builds on Windows and compiles/tests headlessly in Ubuntu WSL2, domain semantics are exhaustively tested, controller boundaries are stable enough for parallel UI/audio work, unsupported operations fail predictably, and no native crate depends on the webplayer or converter. Native macOS and Linux desktop builds are optional Phase 12 platform-expansion evidence.

## Phase 1 execution record - 2026-07-22

Status: exit gate passed.

Implementation remained limited to `stemplayer-native/`. The Phase 0 probe and evidence are intact.

### Deliverables

- Independent 13-crate/application default workspace plus the retained Phase 0 probe.
- Strong domain values, typed commands, immutable shared snapshots, bounded diagnostics, stable errors, and internal source-chain retention.
- Exhaustive transport, mixer, numeric, snapshot, command queue, probe, and dependency-boundary tests.
- Checked integer rational timeline conversion with no floating-point clock.
- Capacity-one coalescing snapshot subscriptions and a bounded, zero-worker-thread headless controller.
- Feature topology for local WAV, local MP4, AAC, ALAC, HLS, diagnostics UI, fake adapters, and platform integration.
- `stemplayer-probe` with working `version` and `validate-config`; future operations return `E_PLAYBACK_FEATURE_UNSUPPORTED`.
- Native CI, crate responsibility diagram, feature matrix, and machine-readable qualification evidence.

### Exit-gate evidence

| Gate | Evidence | Result |
| --- | --- | --- |
| Windows formatting | `cargo fmt --all -- --check` | PASS |
| Windows lint | `cargo clippy --locked --all-targets --all-features -- -D warnings` | PASS |
| Windows contracts | `cargo test --locked --all-features` | PASS, 28 tests |
| Feature topology | `cargo check --locked --no-default-features` and `cargo check --locked --all-features` | PASS |
| Ubuntu WSL2 | all-feature tests plus no-default-feature check, Rust 1.97.1 | PASS, 28 tests |
| Standalone boundary | copied to `C:\tmp\stemplayer-native-phase1-boundary-019f894b`; `cargo test --locked --offline --all-features` | PASS, 28 tests |
| Dependency direction | architecture tests plus `cargo tree --locked -p stemplayer-engine` | PASS, no UI/platform/network dependency |
| Forbidden coupling | exact Rust/Cargo/build-script scan for webplayer, converter, Node.js, pnpm, Python, and FFmpeg references | PASS, zero matches |
| Dependency policy | current `cargo audit` and `cargo deny list --format json` | PASS, zero vulnerabilities and zero unlicensed packages; four accepted unmaintained Phase 0 transitive warnings |
| Probe behavior | release `validate-config` and unsupported `inspect` | PASS, structured JSON |

### Resource baseline

- Probe size: 347,648 bytes debug and 188,416 bytes release.
- Warm Ubuntu WSL2 all-feature test run: 1.25 seconds elapsed and 83,000 KiB maximum resident set.
- Domain crates and the headless controller contain zero thread-spawn calls. The headless controller documents and creates zero worker threads.
- Machine-readable evidence: `stemplayer-native/artifacts/phase1/qualification.json`.

### Decisions and limitations
