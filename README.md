# Stemplayer Native

Play your music, or take control of the individual stems. Stemplayer Native is a desktop audio player with a four-stem mixer, waveform navigation, playlists, and local or HLS playback.

## Download

| Platform | Requirements | Download |
| --- | --- | --- |
| Windows x64 | Windows 11 24H2 or newer | [Portable ZIP](https://github.com/ODevStudio/stemplayer-native/releases/download/v0.1.0-preview.1/stemplayer-native-0.1.0-preview.1-windows-x86_64.zip) |
| Linux x64 | Ubuntu 24.04 or newer, graphical desktop | [tar.gz](https://github.com/ODevStudio/stemplayer-native/releases/download/v0.1.0-preview.1/stemplayer-native-0.1.0-preview.1-linux-x86_64.tar.gz) |
| macOS Apple silicon | macOS 14 or newer | [Application ZIP](https://github.com/ODevStudio/stemplayer-native/releases/download/v0.1.0-preview.1/stemplayer-native-0.1.0-preview.1-macos-aarch64.zip) |

Extract the complete archive. On Windows, run `stemplayer.exe`; on Linux, run `./stemplayer`; on macOS, open `Stemplayer Native.app`. No installer is needed. Linux uses system audio, font, display, and desktop-portal libraries; the bundled README lists the required Ubuntu packages. Intel Macs are not included.

The preview has no verified publisher signature. Windows may display a reputation warning; macOS uses an ad-hoc signature and is not notarized. Keep Defender, SmartScreen, Gatekeeper, and other protections enabled. The [release page](https://github.com/ODevStudio/stemplayer-native/releases/tag/v0.1.0-preview.1) includes checksums for download integrity.

## Features

- **Four-stem mixing:** adjust gain and mute individual stems, or listen to the source's master reference.
- **Waveform navigation:** see the audio timeline and seek through a track, with cached waveform analysis for repeat playback.
- **Local audio:** play WAV, FLAC, MP3, Ogg Vorbis, AAC, and ALAC in supported containers.
- **Stem sources:** open Studio WAV bundles and supported stem MP4/M4A files.
- **HLS playback:** open explicit `stem-hls-v1` streams alongside your local collection.
- **Playlists and settings:** keep your listening queue and preferences between sessions.

The player runs natively in Rust with a Slint interface. There is no browser, WebView, FFmpeg, or converter runtime. Local playback works offline; networking is used for HLS sources you explicitly open. There is no telemetry or automatic updater.

## Using the Player

Open a local audio file, a Studio WAV bundle, a stem MP4/M4A source, or a `stem-hls-v1` URL. Use the transport controls to play, pause, stop, and seek. For stem sources, use the four gain and mute controls to change the mix. The master reference is separate and does not follow individual stem gains.

Keep a Studio WAV bundle's manifest and tracks together. The player does not separate ordinary recordings into stems, and its HLS support is not a general streaming-site or DRM client. See the [format documentation](docs/README.md) for accepted source profiles.

## Settings and Removal

Settings and user data are stored outside the extraction folder:

- Windows: `%APPDATA%/stemplayer-native` for settings and playlists; `%LOCALAPPDATA%/stemplayer-native` for data and cache.
- Linux: `stemplayer-native` under the XDG configuration, data, and cache directories, normally `~/.config`, `~/.local/share`, and `~/.cache`.
- macOS: `stemplayer-native` under `~/Library/Preferences`, `~/Library/Application Support`, and `~/Library/Caches`.

To remove the application, close it and delete its extracted folder. To reset settings and cache as well, remove only its named `stemplayer-native` directories. Back up settings before switching versions.

## Feedback

[Report a bug](https://github.com/ODevStudio/stemplayer-native/issues) with the version and commit from `BUILD-INFO.json`, operating system, source format, and steps to reproduce it. Keep private audio, settings, full paths, and secret URLs out of reports. See the [security policy](SECURITY.md) for vulnerabilities.

![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/v1.17.1/logo/MadeWithSlint-logo-whitebg.png)

## Build From Source

Install Rust and the native platform build tools. The repository pins the Rust toolchain:

```powershell
cargo build --locked --release -p stemplayer
cargo run --locked --release -p stemplayer
```

Linux source builds require the audio and font development libraries listed in CI. The Linux package also includes application and Plasma protocol-library source with rebuilding instructions.

## Documentation

- [Current status](STATUS.md)
- [Roadmap](ROADMAP.md)
- [Documentation index](docs/README.md)
- [Compatibility policy](docs/compatibility.md)
- [Security policy](SECURITY.md)
- [Contributing](CONTRIBUTING.md)

## License

This project is licensed under either the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option.
