# Network and HLS constraints

Applies under `crates/stemplayer-network/`.

- Keep network behavior behind the `hls` feature and preserve zero network activity while HLS is unused.
- Enforce the documented HTTPS and loopback policy for entry URLs, child URLs, and every redirect; revalidate after resolution.
- Bound redirects, retries, response bytes, manifests, playlists, segments, queues, cache, latency accounting, and diagnostic text.
- Preserve grouped stem synchronization, exact segment boundaries, cancellation, and generation isolation.
- Do not introduce browser cookies, ambient credentials, generic protocol handling, or persistent cache expansion without an approved contract.
- Update the HLS client profile and compatibility record only when the producer/client contract actually changes.

Use fake transports for deterministic policy tests and a small synthetic loopback integration only where process-level HTTP behavior matters.
