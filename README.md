# Stemplayer Native

Stemplayer Native is a standalone Rust desktop player for supported Studio WAV bundles, stem MP4/M4A sources, and explicit `stem-hls-v1` sources. It does not build, launch, or bundle the web player, converter, Node.js, Python, FFmpeg, or a browser runtime.

## Availability

The current checkout is a source build. The first qualified release target is Windows; packaging, signing, installation, and clean-system qualification remain blocked by the inputs listed in [STATUS.md](STATUS.md). Do not treat a local release build as a signed distributable.

## Build and run

Install the pinned Rust toolchain, then run:

```powershell
cargo fmt --all -- --check
cargo test --locked --workspace --all-features
cargo build --locked --release -p stemplayer
cargo run --locked --release -p stemplayer
```

Linux compilation requires the system audio and font development libraries used by CI. Product qualification remains Windows-first unless the status and roadmap say otherwise.

## Documentation

- [Current status](STATUS.md)
- [Roadmap](ROADMAP.md)
- [Documentation index](docs/README.md)
- [Compatibility policy](docs/compatibility.md)
- [Security policy](SECURITY.md)
- [Contributing](CONTRIBUTING.md)

Coding agents should begin with [AGENTS.md](AGENTS.md), which links to task-specific context without loading the complete documentation set.

## License

This project is licensed under either the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option.
