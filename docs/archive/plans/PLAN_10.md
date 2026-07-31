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

## Implementation record

### 2026-07-26 - Phase 10 started

Status: in progress.

Plan:

- Reuse the qualified compressed-container decoder and controller transport rather than introduce a second playback engine.
- Implement the approved strict manifest and VOD playlist profile, HTTPS-only HTTP policy, complete-group fetching, bounded cache/retry behavior, preroll trimming, and generation cancellation in the existing network crate.
- Add the smallest production UI path for explicit URL open, buffering state, redacted diagnostics, and cache clearing.
- Qualify hostile references, alignment, continuity, seek/recovery, bounded resources, lazy runtime initialization, UI behavior, and workspace regressions before evaluating the exit gate.

### 2026-07-28 - Exit gate passed

Status: exit gate passed.

Implementation:

- Added a feature-gated blocking `ureq` client with Rustls/WebPKI roots, zero redirects, bounded headers and bodies, explicit timeouts, no cookies or ambient credentials, and debug-only loopback HTTP. The blocking client creates no async runtime or background network thread.
- Added strict manifest, waveform, VOD playlist, role, duration, sequence, boundary, codec, and normalized same-origin relative-reference validation. Entry URLs reject credentials, queries, fragments, non-HTTPS schemes, and non-loopback HTTP.
- Added complete-group fetching and publication, generation cancellation, bounded retry delays, exact frame-derived seek selection, AAC-LC segment decoding, declared preroll removal, one-packet terminal AAC padding removal, and a 64 MiB memory-only LRU group cache.
- Added an engine-owned external-source boundary so the engine remains independent of HTTP, plus native controller, URL-open, recent-source preference, remote waveform, buffering/network diagnostics, redacted export, and clear-network-cache integration.
- Added `hls-validate` and `hls-playback-sim` probes and a generated local Windows fixture/server qualification. Private media remains optional through `STEMPLAYER_PRIVATE_HLS_URL` and is never stored in version control.

Security and dependency decisions:

- `ureq 3.3.0` was selected as the smallest synchronous client matching the existing worker model. Default features are disabled and only its Rustls backend is enabled. The resolved chain is `ureq -> rustls 0.23.42 -> rustls-webpki/ring` with WebPKI roots; no Tokio or other async runtime is present.
- Redirects are rejected rather than followed, every fetched body has a type-specific byte cap, segment limits are additionally derived from declared bitrate and duration, and all source diagnostics redact path contents and credentials.
- A self-signed local TLS server is rejected as `E_HLS_TLS_FAILED`; redirects, oversized responses, and credential-bearing URLs are rejected as their strict HLS error codes. Missing, corrupt, and oversized segments stop playback without skipping audio.
- `cargo audit` reported two pre-existing target-specific `quick-xml 0.39.4` advisories through Slint's Linux Wayland build tooling and four pre-existing unmaintained transitive UI dependencies. None are introduced by or reachable from the Phase 10 HTTP/TLS runtime; the new `ureq`/Rustls dependency chain had no advisory finding.

Qualification evidence:

- `cargo test --workspace --all-features`: passed; 147 tests passed and 4 hardware/private-fixture tests remained explicitly ignored.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo build --release -p stemplayer -p stemplayer-probe --features stemplayer-probe/hls`: passed.
- `scripts/qualify-phase10-windows.ps1`: passed. Full playback rendered and ended at exactly 192,000 source frames; all five roles stayed at zero-frame offset; 5,120 duplicated preroll frames were discarded; one forced 503 recovered within the bounded retry policy; peak cache use was 7,936,556 bytes under the 67,108,864-byte cap.
- Representative seeks at frames 0, 96,000, and 187,904 produced audio after re-prime. Missing, corrupt, redirect, oversized response, oversized segment, credentials, and invalid TLS cases returned the expected stable error codes.
- Release idle sample before opening HLS: 48,197,632-byte working set, 14,675,968 private bytes, 0.0625 CPU seconds over three seconds, and 12 process threads. No HLS transport, request, cache group, or network worker exists until URL open.
- `git diff --check`: passed. Full raw Phase 10 diff and dependency boundary were inspected; the webplayer and converter remain untouched.

Exit-gate decision:

- Qualified explicit `stem-hls-v1` sources play through synchronized complete groups with exact boundaries and end frame, bounded recovery and memory, secure URL/TLS handling, stable seeks, native diagnostics and cache control, and no network runtime effect while unused. Phase 10 passes its exit gate.
