# Stemplayer Native

Play your music, or take control of the individual stems. Stemplayer Native is a Windows desktop audio player with a four-stem mixer, waveform navigation, playlists, and local or HLS playback.

## Download

[Download for Windows](https://github.com/ODevStudio/stemplayer-native/releases/tag/v0.1.0-preview.1)

Requires **Windows 11 24H2 or newer, x64**. Download the ZIP, extract it, and run `stemplayer.exe`. No installation or administrator access is needed.

The preview is unsigned, so Windows may display a reputation warning. Keep Defender, SmartScreen, and other protections enabled. The release includes `SHA256SUMS.txt` for checking download integrity.

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

Settings and playlists are stored in `%APPDATA%/stemplayer-native`; data and waveform cache are stored in `%LOCALAPPDATA%/stemplayer-native`. The portable ZIP does not keep these inside its extraction folder.

To remove the application, close it and delete its extracted folder. To reset settings and cache as well, remove only the two named per-user folders. Back up settings before switching versions.

## Feedback

[Report a bug](https://github.com/ODevStudio/stemplayer-native/issues) with the version and commit from `BUILD-INFO.json`, Windows version, source format, and steps to reproduce it. Keep private audio, settings, full paths, and secret URLs out of reports. See the [security policy](SECURITY.md) for vulnerabilities.

![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/v1.17.1/logo/MadeWithSlint-logo-whitebg.png)

## Build From Source

Install Rust and the native platform build tools. The repository pins the Rust toolchain:

```powershell
cargo build --locked --release -p stemplayer
cargo run --locked --release -p stemplayer
```

Linux source builds require the audio and font development libraries listed in CI. Downloadable releases currently target Windows x64.

## Documentation

- [Current status](STATUS.md)
- [Roadmap](ROADMAP.md)
- [Documentation index](docs/README.md)
- [Compatibility policy](docs/compatibility.md)
- [Security policy](SECURITY.md)
- [Contributing](CONTRIBUTING.md)

## License

This project is licensed under either the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option.
