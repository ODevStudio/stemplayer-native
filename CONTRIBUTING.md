# Contributing

Keep changes inside the native repository. Do not add a checkout dependency on the web player or converter, and do not commit private media, generated reports, screenshots, machine paths, or credentials.

Preserve `Cargo.lock`, `rust-toolchain.toml`, workspace path dependencies, and the declared format profiles. Security checks at file, URL, manifest, and archive boundaries are required even when a change is small.

Before submitting a change, run the narrowest relevant checks and then the full locked workspace checks when shared behavior or public contracts change:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --workspace
cargo check --locked --no-default-features
cargo check --locked --all-features
```

Record user-visible behavior and qualification results in `CHANGELOG.md` or `docs/qualification/`. Keep current phase status in `STATUS.md` only.
