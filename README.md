# Stemplayer Native

Stemplayer Native is a standalone Rust desktop player for supported Studio WAV bundles, stem MP4/M4A sources, and explicit `stem-hls-v1` sources.

The player does not build, launch, or bundle the web player, converter, Node.js, Python, FFmpeg, or a browser runtime. The native repository owns the client behavior and conformance evidence for the formats it consumes.

## Build

Install the pinned Rust toolchain, then run:

```powershell
cargo fmt --all -- --check
cargo test --locked --workspace
cargo build --locked --release -p stemplayer
```

Run the desktop player with `cargo run --locked --release -p stemplayer` on a qualified Windows system.

## Documentation

- [Current status](STATUS.md)
- [Roadmap](ROADMAP.md)
- [Compatibility policy](docs/compatibility.md)
- [Documentation index](docs/README.md)
- [Security policy](SECURITY.md)

## License

This project is licensed under either the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option.
