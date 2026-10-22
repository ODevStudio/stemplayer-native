Stemplayer Native - unsigned Windows hobby preview

Requirements: Windows 11 24H2 or newer, x64. Other platforms are unsupported.
Extract the whole ZIP into a folder you control, then run stemplayer.exe.
No administrator access, installer, development tools, browser, converter,
Node.js, Python, or FFmpeg runtime is required.

Use the Open controls to select supported local audio, Studio WAV bundles,
stem MP4/M4A sources, or an explicit stem-hls-v1 URL. A Studio WAV bundle
needs its manifest and matching tracks together; arbitrary WAV files do not
become stem bundles. HLS is not a generic streaming-site or DRM client.
Ordinary supported audio includes WAV, FLAC, MP3, Ogg Vorbis, AAC, and ALAC
in the containers accepted by the native format profiles.

Use Play, Pause, Stop, seeking, and the four stem gain/mute controls as
appropriate for the source. Master-reference and stem-mix output are distinct;
the master reference does not respond to individual stem gain controls.
Networking is used only for explicitly opened HLS sources. There is no
automatic updater or telemetry. Local playback works offline.

This build is unsigned. Windows may display reputation or protection warnings.
Do not disable Defender, SmartScreen, or other security protections. If Windows
blocks the application, stop and report the warning to the maintainer.
Compare the ZIP's SHA-256 with SHA256SUMS.txt from the official GitHub release.
A matching checksum detects changed downloads; it does not authenticate the
publisher. Obtain both files only from the project's official release page.

Settings and the restored playlist remain under %APPDATA%/stemplayer-native.
Data and waveform cache remain under %LOCALAPPDATA%/stemplayer-native.
Portable means no installation, not self-contained user data. Extraction
folders share this profile. Close the player before changing versions, keep
the old extraction for rollback, and back up settings before trying a new
preview. Restored sources must not start audible playback automatically.
Remove the extracted folder to remove the application. To reset user data,
close the player and remove only its two named per-user folders; do not remove
parent AppData folders. These folders may retain private source paths.

If playback fails, check the selected output device, reconnect the device,
retry output, and confirm that the selected source/container is supported.
For a missing DLL or startup error on clean Windows, report the exact message;
do not download individual DLLs from third-party websites.

Known limitations: experimental unsigned preview, Windows x64 only, no
installer, file associations, single-instance routing, system media keys,
automatic updates, or converter. A successful CI build is not clean-system,
listening, screen-reader, device-lifecycle, or stable-release qualification.

Report bugs at https://github.com/ODevStudio/stemplayer-native/issues with
the version and commit from BUILD-INFO.json, Windows version, source format,
audio-device description, redacted error code/message, and reproduction steps.
Do not attach private media, settings, full paths, tokens, or secret URLs.
Report suspected vulnerabilities privately to the repository owner first.

License: MIT OR Apache-2.0 for this project; see both bundled license files.
THIRD-PARTY-NOTICES.txt contains dependency terms and source locations.
Slint is used under its royalty-free 2.0 application license. The official
public download page must display the Made with Slint attribution badge.
