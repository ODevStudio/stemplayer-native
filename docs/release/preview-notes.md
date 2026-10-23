# Stemplayer Native 0.1.0-preview.1

Native audio and stem playback for Windows, Linux, and macOS.

![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/v1.17.1/logo/MadeWithSlint-logo-whitebg.png)

## Download and Run

| Platform | Requirements | Archive |
| --- | --- | --- |
| Windows x64 | Windows 11 24H2 or newer | `stemplayer-native-0.1.0-preview.1-windows-x86_64.zip` |
| Linux x64 | Ubuntu 24.04 or newer, graphical desktop | `stemplayer-native-0.1.0-preview.1-linux-x86_64.tar.gz` |
| macOS Apple silicon | macOS 14 or newer | `stemplayer-native-0.1.0-preview.1-macos-aarch64.zip` |

Extract the complete archive. Run `stemplayer.exe` on Windows, `./stemplayer` on Linux, or open `Stemplayer Native.app` on macOS. No installer is needed. Linux uses system audio, font, display, and desktop-portal libraries; see its bundled README for Ubuntu runtime packages. Intel Macs are not included.

Use `SHA256SUMS.txt` for the Windows ZIP and the matching `.sha256` file for each Linux/macOS archive. Checksums detect changed downloads, not publisher identity. Windows may show an unsigned-app reputation warning. The macOS app is ad-hoc signed, not Developer ID signed or notarized. Keep Defender, SmartScreen, Gatekeeper, and other protections enabled; report unexpected blocks rather than disabling protections.

## What It Does

- Play WAV, FLAC, MP3, Ogg Vorbis, AAC, and ALAC in supported containers.
- Open Studio WAV bundles, stem MP4/M4A files, and explicit `stem-hls-v1` streams.
- Adjust four individual stem gains and mutes, or switch to the separate master reference.
- Navigate with waveforms and seeking; waveform analysis is cached for repeat playback.
- Keep playlists and preferences between sessions.
- Play local files offline, without a browser, WebView, FFmpeg, or converter runtime.

## How It Works

Open a local file, a bundle, or an HLS URL using the Open controls. Use Play, Pause, Stop, and seeking to navigate; for stem sources, use the mixer to change the balance. Master-reference playback does not follow individual stem gains. Keep bundle manifests and tracks together. The player does not create stems from ordinary recordings or access DRM streaming services.

Networking is used only for explicitly opened HLS sources. There is no telemetry or automatic updater. Installers, file associations, and system media keys are not included. Windows/Linux ARM64 and macOS Intel builds are not included.

## Data and Removal

Windows settings use `%APPDATA%/stemplayer-native`; data/cache use `%LOCALAPPDATA%/stemplayer-native`. Linux uses `stemplayer-native` under its XDG configuration, data, and cache directories. macOS uses `stemplayer-native` under `~/Library/Preferences`, `~/Library/Application Support`, and `~/Library/Caches`. Close the player before switching versions and back up settings for rollback. To remove it, delete the extraction; remove only its named per-user directories to reset its data as well.

See the bundled `README.txt` for troubleshooting and `BUILD-INFO.json` for the exact source, packaging revision, and build details. Linux includes `CORRESPONDING-SOURCE.tar.gz` with the application, Plasma protocol-library source, and rebuilding instructions. Report bugs through repository issues with version, operating system, source format, and reproduction steps. Do not upload private audio, settings, full paths, credentials, or secret URLs. Report vulnerabilities privately to the repository owner.

Release assets are immutable. A fix receives a new preview version; an unsafe release is withdrawn with an advisory rather than silently replaced.
