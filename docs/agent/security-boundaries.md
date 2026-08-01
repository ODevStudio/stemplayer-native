# Security boundaries

Read `SECURITY.md` first. This file routes implementation work to the relevant contract and tests.

## Local sources and formats

Applies to `stemplayer-format` and `stemplayer-decode`.

- Validate content, structure, declared sizes, normalized names, roles, hashes, timelines, and file identity; do not trust extensions.
- Bound reads, allocations, open files, tracks, frames, metadata, and error detail.
- Reject ambiguity and mutation during inspection/decode.
- Use synthetic fixtures for malformed and boundary cases.
- Load the applicable file under `docs/formats/` before changing accepted input.

## Network and HLS

Applies to `stemplayer-network` and HLS controller code.

- Keep networking behind the `hls` feature and avoid network runtime activity while HLS is unused.
- Revalidate entry, child, and redirect URLs under one scheme, host, and loopback policy.
- Bound response size, manifest entries, segments, retries, redirects, queues, cache, and diagnostic text.
- Preserve cancellation and generation isolation so stale work cannot publish into an active session.
- Load `docs/formats/stem-hls-v1-client-profile.md` and `docs/compatibility.md` for contract changes.

## Real-time audio

Applies to `stemplayer-output` and render/callback paths.

- The device callback copies prepared stereo samples, performs unavoidable format conversion, advances counters, and emits silence on underrun.
- It must not allocate, perform file or network I/O, decode, parse, log, update UI, or take a normal blocking mutex.
- Use the callback audit tool and output tests when touching this boundary.

## Cache, settings, and diagnostics

- Use platform-standard per-user locations.
- Treat cache and settings contents as untrusted and versioned.
- Avoid full private paths, URLs with secrets, media content, and unbounded server text in diagnostics.
- Write atomically where partial state can corrupt later behavior.

## Release and OS integration

Shell inputs, file associations, local routing, installer metadata, signatures, and update metadata are untrusted. Phase 11 work must follow the current plan and cannot proceed past blocked identity, signing, or toolchain gates by inventing placeholder production values.
