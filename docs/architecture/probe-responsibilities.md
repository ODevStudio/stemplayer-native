# Probe Responsibilities

The workspace retains two probes because they cover different capabilities.

- `apps/stemplayer-probe` is the current headless integration and conformance surface. It exercises format validation, decoding, playback simulation, HLS, device tests, and structured failure classes.
- `phase0-probe` is a small legacy qualification harness used by the Windows audio, codec, UI, and resource scripts. It owns the primitive self-check, direct device smoke, codec-check command, and standalone UI smoke path that the current headless probe does not provide.

The old probe is therefore not retained solely for historical plans. It remains until its active qualification scripts are replaced by equivalent product or headless commands. No source or runtime dependency crosses the two probe boundaries.
