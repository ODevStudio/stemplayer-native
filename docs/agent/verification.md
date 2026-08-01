# Verification guide

Run the narrowest useful check while iterating. Before merging, run the tier implied by the change. Commands assume the repository root and the pinned toolchain.

## Tier 0: documentation-only

- Run `python scripts/check_repository.py`.
- Run `python -m unittest discover -s scripts -p "test_*.py"`.
- Run `git diff --check`.
- Do not claim Rust behavior was verified when no Rust command ran.

## Tier 1: one package or local behavior

Examples:

```powershell
cargo test --locked -p stemplayer-format
cargo test --locked -p stemplayer-network --all-features
cargo test --locked -p stemplayer --all-features playlist
cargo test --locked -p stemplayer-probe --test cli
```

Choose the package and test filter from the affected code. A passing narrow test is iteration evidence, not the complete merge gate for shared contracts.

## Tier 2: shared Rust contract or normal pull request

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo check --locked --workspace --no-default-features
cargo check --locked --workspace --all-features
```

## Tier 3: product or release-sensitive change

Run Tier 2, then:

```powershell
cargo build --locked --release -p stemplayer
```

Also run the relevant probe, synthetic integration, or qualification script. Hardware audio, UI automation, long soaks, installer, signing, and clean-system tests require their approved Windows environment; hosted CI success is not equivalent evidence.

## Evidence rules

Report the exact command, result, target, feature set, operating system, first actionable failure, and why any required check could not run.

Never convert an unexecuted manual gate into a PASS statement.
