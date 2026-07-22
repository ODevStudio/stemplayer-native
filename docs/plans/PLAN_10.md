# Phase 10 — Native `stem-hls-v1` Playback

## Goal

Implement the webplayer’s qualified static HLS VOD capability as an independent native network source. Preserve grouped stem buffering, safe URL resolution, aligned transport, AAC packet behavior, waveforms, mixer semantics, and bounded caches without modifying the webplayer or HLS packager.

## Entry criteria

- Local AAC-LC container decoding is qualified in Phase 7.
- Playback, mixer, resampling, waveform, and diagnostics services are stable.
- `stem-hls-v1` client profile is approved.
- TLS and HTTP dependency gate is complete.
- Security review has defined allowed schemes, redirects, credentials, and cache policy.

## Work package 10.1 — Explicit source model

The user opens an explicit stemset URL. Accept only:

- HTTPS URLs.
- Optionally localhost HTTP in development builds if explicitly enabled.
- File URLs only if the product specification defines a safe local HLS mode; otherwise reject.

Do not crawl pages, discover media automatically, or reuse browser cookies. Store recent URLs only with explicit user preference.

## Work package 10.2 — HTTP client policy

Configure:

- Modern TLS validation.
- No credential prompts or ambient browser credentials.
- Bounded redirects with same-policy revalidation.
- Request, connect, and idle timeouts.
- Maximum manifest, playlist, waveform, segment, and response-header sizes.
- Range request support where useful.
- Content-type diagnostics without trusting MIME alone.
- User-agent identifying application/version without machine identity.
- Proxy behavior documented per platform.

No network thread or async runtime is started until an HLS source is requested or an explicitly enabled update check runs.

## Work package 10.3 — Stemset validation

Validate:

- Schema and format identifier.
- Title and bounded display strings.
- Duration, sample rate, channel count, bitrate.
- Required four stem tracks.
- Optional master.
- Waveform references.
- Unique roles.
- Supported codec/profile declarations.

Every internal reference must be a normalized relative URL path. Reject absolute references, schemes, backslashes, empty components, encoded separators, dot traversal, control characters, queries, fragments, and normalization ambiguity according to the approved profile.

## Work package 10.4 — Playlist validation

For each track:

- Fetch and parse the media playlist.
- Validate media sequence.
- Validate segment durations and counts.
- Resolve relative segment URLs safely.
- Require aligned group structure across roles.
- Enforce total duration tolerance.
- Reject unsupported tags, encryption, live/event playlists, discontinuities, alternate audio, or byte-range modes unless explicitly supported.

The first release targets static VOD only.

## Work package 10.5 — Grouped segment scheduler

Treat corresponding role segments as one playback group:

- Fetch with bounded concurrency.
- Do not publish a group until all required stems are available and validated.
- Optional master belongs to the same group.
- Maintain source-frame continuity across groups.
- Apply backpressure from the source ring.
- Bound encoded and decoded segment cache bytes.
- Prioritize current playback and seek target over prefetch.
- Cancel stale requests on seek or source close.

One slow stem must not cause faster stems to advance independently.

## Work package 10.6 — Segment decode and preroll

Decode qualified self-contained AAC-LC M4A segments using the local container decoder infrastructure.

For segments after the first:

- Honor the profile’s declared duplicated AAC preroll packet behavior.
- Decode required preroll.
- Trim the approved number of initial frames.
- Publish only continuous source frames.
- Validate no gap or overlap at segment boundaries.

Do not re-encode or alter packet payloads.

## Work package 10.7 — HLS seek

- Convert target source frame to segment index using validated durations and exact frame accounting.
- Cancel old generation requests.
- Fetch target group and bounded forward groups.
- Decode/trim preroll.
- Re-prime source ring.
- Resume according to prior desired state.
- Report manifest, group-ready, and seek latency.

Segment duration rounding must not accumulate drift. Use common frame boundaries derived from the approved package profile.

## Work package 10.8 — Network recovery

Define recoverable behavior for:

- Timeout.
- Temporary connection failure.
- Missing segment.
- Partial response.
- TLS failure.
- Redirect policy violation.
- Corrupt segment.
- One role lagging.

Retries are bounded with jitter outside the real-time path. Playback enters controlled buffering. Fatal validation or integrity failures stop the source rather than silently skipping audio.

## Work package 10.9 — Segment cache

Implement a bounded ephemeral encoded-segment cache:

- Memory-first with strict byte cap.
- Optional disk cache only after privacy/security review.
- Keyed by normalized URL plus validators such as ETag where available.
- No credentials stored.
- Clear on source close by default unless persistent caching is explicitly enabled.
- LRU eviction.
- Separate current-group protection from speculative prefetch.

Waveform assets use their own validated bounded cache path.

## Work package 10.10 — UI and diagnostics

Add:

- Open URL dialog with scheme guidance.
- Network/buffering state.
- Download and cache diagnostics.
- Current segment/group.
- Retry and recovery status.
- Clear source cache action.
- Redacted URL display/export.

Diagnostics include manifest latency, group-ready time, fetch count/failures, buffering count, recovery time, seek latency, inter-stem offset, current/peak cache bytes, and warnings.

## Deliverables

- Native HTTP/TLS client policy.
- Stemset and playlist validators.
- Grouped segment scheduler.
- AAC segment decode with preroll trim.
- Frame-accurate HLS seek.
- Bounded network recovery and cache.
- HLS UI and diagnostics.
- Static local test server used only by tests.

## Tests and evidence

- Root, relative, Unicode-encoded, and cross-origin HTTPS fixtures.
- Every unsafe path/reference rejection.
- Missing/wrong role, duration, sequence, and segment alignment.
- Delayed, failed, truncated, corrupt, and redirected responses.
- Duplicated preroll continuity tests.
- Full-duration exact end frame.
- Random seek and retry stress.
- Bandwidth and latency shaping.
- Memory/cache caps under long content.
- TLS validation and no-credential proof.
- Playback/mixer parity with equivalent local content.

## Resource checks

- Async runtime absent when HLS is unused.
- Network concurrency and cache bytes bounded.
- Decode and source rings retain local-player limits.
- CPU and memory measured under normal and constrained networks.
- No unbounded retry, log, URL, or playlist growth.

## Risks and mitigations

- **HLS libraries accept unsafe general features:** place a strict profile validator before scheduling.
- **Per-role downloads desynchronize:** publish complete groups only.
- **Segment rounding causes drift:** derive common frame boundaries and test complete duration.
- **Network runtime increases idle footprint:** initialize lazily and tear down after source close where practical.

## Exit gate

Phase 10 is complete when explicit qualified `stem-hls-v1` sources play with synchronized grouped buffering, exact segment-boundary continuity, bounded network and memory use, secure URL handling, stable seeks and recovery, and no effect on local-player idle footprint when HLS is unused.
