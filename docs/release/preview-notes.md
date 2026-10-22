# Stemplayer Native 0.1.0-preview.1

Unsigned portable hobby preview for Windows 11 24H2 or newer, x64. Extract the complete ZIP and run `stemplayer.exe`; no installer or administrator access is needed.

![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/v1.17.1/logo/MadeWithSlint-logo-whitebg.png)

## Downloads and Safety

Download `stemplayer-native-0.1.0-preview.1-windows-x86_64.zip` and `SHA256SUMS.txt` from this release. Compare the ZIP's SHA-256 before extraction. Checksums detect changed downloads, not publisher identity. This application is unsigned and Windows may display reputation warnings. Do not disable Defender, SmartScreen, or other protections; stop and report a blocked launch.

## Included

Native local playback, Studio WAV bundles, stem MP4/M4A, explicit `stem-hls-v1` playback, mixer controls, playlists, waveforms, and bounded waveform caching. See the bundled `README.txt` for supported-source guidance, troubleshooting, privacy, data removal, and dependency notices. There is no browser or converter runtime.

## Limitations

Experimental Windows x64 preview, not a stable release or completion of Phase 11/12. No installer, signing, file associations, single-instance routing, system media keys, automatic updates, ARM64, macOS, or Linux support. Portable extraction still uses the existing per-user settings/cache folders. Close the player before switching versions and back up settings for rollback.

## Qualification and Reports

Before publishing, the maintainer must append the tested commit, ZIP SHA-256, Windows build, concise clean-system/audio/accessibility/performance results, and any accepted non-blocking limitations. Do not publish this template with unexecuted checks represented as passing.

Report bugs through repository issues with the version/commit from `BUILD-INFO.json`, Windows version, source format, audio-device description, redacted error, and reproduction steps. Do not upload private audio, settings, full paths, credentials, or secret URLs. Report vulnerabilities privately to the repository owner.

Release assets are immutable. A fix receives a new preview version; an unsafe release is withdrawn with an advisory rather than silently replaced.
