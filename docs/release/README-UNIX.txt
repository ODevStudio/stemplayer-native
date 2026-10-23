Stemplayer Native - audio and stem player

Linux: Ubuntu 24.04 or newer, x86-64, glibc 2.39 or newer, a graphical desktop,
and an audio output device. Extract the complete tar.gz and run ./stemplayer.
The Linux package uses system libraries rather than bundling a Linux runtime.
On Ubuntu, install these runtime packages if they are not already present:

sudo apt install libasound2t64 libfontconfig1 libx11-6 libx11-xcb1 libxcursor1 libxrandr2 libxi6 libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 libwayland-cursor0 libwayland-egl1 libdbus-1-3 xdg-desktop-portal xdg-desktop-portal-gtk

Use a working desktop audio configuration. X11 is the initial desktop target;
Wayland, other distributions, and headless audio setups are not qualified.
RUNTIME-DEPENDENCIES.txt lists the linked libraries from the build environment.

macOS: macOS 14 or newer, Apple silicon. Intel Macs are not included.
Extract the complete ZIP and open Stemplayer Native.app. Keep the accompanying
license, notice, and build-information files when redistributing the package.
The app is ad-hoc signed, not Developer ID signed or notarized. An ad-hoc
signature provides code integrity, not a verified publisher identity.
macOS may block a downloaded app pending its normal per-app security approval.
Do not disable Gatekeeper, remove quarantine attributes, or change system-wide
security settings to run it. Stop and report an unexpected warning.

Each archive has a matching .sha256 download. Compare it before extraction:
sha256sum -c stemplayer-native-0.1.0-preview.1-linux-x86_64.tar.gz.sha256
shasum -a 256 -c stemplayer-native-0.1.0-preview.1-macos-aarch64.zip.sha256
Checksums detect changed downloads; they do not authenticate the publisher.

Open local WAV, FLAC, MP3, Ogg Vorbis, AAC, or ALAC in supported containers,
Studio WAV bundles, stem MP4/M4A files, or explicit stem-hls-v1 URLs.
Keep bundle manifests and tracks together. The player does not create stems
from an ordinary recording or access DRM streaming services.
Use Play, Pause, Stop, seeking, and the four individual stem gain/mute controls.
The separate master reference is not affected by individual stem gains.
Networking is used only for explicitly opened HLS sources. Local playback
works offline. There is no telemetry or automatic updater.

Linux settings, data, and cache use stemplayer-native under XDG_CONFIG_HOME,
XDG_DATA_HOME, and XDG_CACHE_HOME. Defaults are ~/.config, ~/.local/share,
and ~/.cache respectively.
macOS settings use ~/Library/Preferences/stemplayer-native; data uses
~/Library/Application Support/stemplayer-native and cache uses
~/Library/Caches/stemplayer-native.
Close the player before switching versions and back up settings for rollback.
Delete the extraction to remove the program. To reset its data as well, remove
only the named stemplayer-native directories after closing the player.

Both project licenses and THIRD-PARTY-NOTICES.txt are included. Linux also
includes complete application and Plasma protocol-library source in
CORRESPONDING-SOURCE.tar.gz, with REBUILD-LINUX.txt for rebuilding or modifying
that library. Source and build identity are recorded in BUILD-INFO.json.

Report bugs at https://github.com/ODevStudio/stemplayer-native/issues with the
version/commit, operating system, source format, and reproduction steps.
Do not upload private audio, settings, full paths, credentials, or secret URLs.
Report suspected vulnerabilities privately to the repository owner.
