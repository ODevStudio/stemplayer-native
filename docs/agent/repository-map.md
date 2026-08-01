# Repository map

Use this map to choose the first files to inspect. `Cargo.toml` remains the authority for current workspace membership and features.

## Applications and probes

- `apps/stemplayer/`: product UI, controller, playlist, settings, and application composition.
- `apps/stemplayer-probe/`: machine-readable headless inspection and qualification commands.
- `phase0-probe/`: legacy platform qualification harness retained while active scripts still depend on capabilities not provided elsewhere.
- `tools/stemplayer-callback-audit/`: specialized callback allocation audit. Its documented unsafe-code exception is local to that tool.

## Core contracts and playback

- `crates/stemplayer-domain/`: commands, errors, states, snapshots, roles, and bounded values.
- `crates/stemplayer-engine/`: playback sessions, scheduling, synchronized rings, transport, and decode coordination.
- `crates/stemplayer-timeline/`: exact source/output timeline and MP4 edit mapping.
- `crates/stemplayer-mixer/`: four-stem controls, master-reference behavior, ramps, and stereo mix.
- `crates/stemplayer-output/`: device lifecycle, output ring, resampling, callback, and output qualification.

## Input, network, and analysis

- `crates/stemplayer-format/`: Studio WAV and stem container structure, manifests, hashing, bounded readers, and role resolution.
- `crates/stemplayer-decode/`: compressed/local audio inspection and bounded decoding.
- `crates/stemplayer-network/`: optional `stem-hls-v1` URL policy, manifests, playlists, fetches, and grouped source behavior.
- `crates/stemplayer-waveform/`: analysis, fingerprinting, cache format, priority, and viewport projection.

## UI and platform

- `crates/stemplayer-ui/`: presentation, playhead, command adaptation, and deterministic mocks.
- `crates/stemplayer-platform/`: settings paths, platform integration, and OS-specific policy boundaries.
- `apps/stemplayer/ui/`: Slint presentation files; behavior still belongs in Rust controller/domain layers where practical.

## Where to start by change

| Change | Start with |
| --- | --- |
| Source validation or manifest bug | `stemplayer-format`, then format spec and failing test |
| Codec or seek behavior | `stemplayer-decode`, `stemplayer-timeline`, fixture test |
| Transport or buffering | `stemplayer-engine`, domain state/error contracts |
| Gain, mute, solo, master, resampling | `stemplayer-mixer`, then `stemplayer-output` |
| HLS, redirects, TLS, cache, network diagnostics | `stemplayer-network` and HLS client profile |
| Playlist, UI, persistence, accessibility | `apps/stemplayer`, `stemplayer-ui`, platform settings |
| Waveform analysis/cache | `stemplayer-waveform` and app analysis controller |
| CI or dependency policy | `.github/workflows/`, root Cargo files, `docs/agent/ci-workflows.md` |
