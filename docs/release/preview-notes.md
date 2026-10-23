# Stemplayer Native 0.1.0-preview.1

Native audio and stem playback for Windows 11 24H2 or newer, x64.

![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/v1.17.1/logo/MadeWithSlint-logo-whitebg.png)

## Download and Run

Download `stemplayer-native-0.1.0-preview.1-windows-x86_64.zip`, extract the complete folder, and run `stemplayer.exe`. No installer or administrator access is needed. `SHA256SUMS.txt` lets you check the ZIP's integrity; it does not authenticate the publisher.

This preview is unsigned and Windows may display a reputation warning. Keep Defender, SmartScreen, and other protections enabled; report a blocked launch rather than disabling them.

## What It Does

- Play WAV, FLAC, MP3, Ogg Vorbis, AAC, and ALAC in supported containers.
- Open Studio WAV bundles, stem MP4/M4A files, and explicit `stem-hls-v1` streams.
- Adjust four individual stem gains and mutes, or switch to the separate master reference.
- Navigate with waveforms and seeking; waveform analysis is cached for repeat playback.
- Keep playlists and preferences between sessions.
- Play local files offline, without a browser, WebView, FFmpeg, or converter runtime.

## How It Works

Open a local file, a bundle, or an HLS URL using the Open controls. Use Play, Pause, Stop, and seeking to navigate; for stem sources, use the mixer to change the balance. Master-reference playback does not follow individual stem gains. Keep bundle manifests and tracks together. The player does not create stems from ordinary recordings or access DRM streaming services.

Networking is used only for explicitly opened HLS sources. There is no telemetry or automatic updater. This release targets Windows x64; installers, file associations, system media keys, and ARM64 are not included.

## Data and Removal

Settings and playlists use `%APPDATA%/stemplayer-native`; data and waveform cache use `%LOCALAPPDATA%/stemplayer-native`. Close the player before switching versions and back up settings for rollback. To remove it, delete the extracted folder. Remove only the two named per-user folders to reset its data as well.

See the bundled `README.txt` for troubleshooting and `BUILD-INFO.json` for the exact version, source commit, and build details. Report bugs through repository issues with those details, Windows version, source format, and reproduction steps. Do not upload private audio, settings, full paths, credentials, or secret URLs. Report vulnerabilities privately to the repository owner.

Release assets are immutable. A fix receives a new preview version; an unsafe release is withdrawn with an advisory rather than silently replaced.
