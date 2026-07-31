# Phase 9 — Waveforms, Bounded Cache, and Diagnostics

## Goal

Add progressive four-stem waveforms, viewport-efficient rendering, a private bounded native cache, and actionable diagnostics without compromising playback. The player remains able to start playback before complete waveform analysis.

## Entry criteria

- Phase 5 playback-priority scheduler exists.
- Phase 8 native UI can display waveform and diagnostics states.
- Native cache privacy and format specifications are approved.
- Local source formats from Phases 4 and 7 are available.

## Work package 9.1 — Waveform model

Define a native waveform snapshot containing:

- Generation/source fingerprint.
- Start and exclusive end frame.
- Sample rate.
- Per-role peak levels.
- Analysis progress.
- Cache status.
- Optional consistency diagnostics.
- Errors and warnings.

Master is not displayed as a fifth stem waveform unless a later product decision explicitly changes the UX.

## Work package 9.2 — Peak pyramid

Generate immutable min/max peaks for each stem at four approved frame resolutions, initially matching the established 256, 1,024, 4,096, and 16,384 frame levels.

Requirements:

- Analyze bounded windows.
- Reuse decoder/source adapters without sharing mutable playback decoder state.
- Do not retain analysis PCM after peak extraction.
- Publish progressive coarse or fine levels according to the UI strategy.
- Exact frame coverage, including final partial bucket.
- Deterministic values across platforms within documented floating-point tolerance.
- Cancellation on source change or cache hit.

## Work package 9.3 — Playback-priority scheduler

Waveform work is always lower priority than playback:

- Pause or reduce analysis when source buffer falls below low watermark.
- Limit analysis worker count.
- Limit pending PCM blocks.
- Yield disk bandwidth under playback pressure.
- Avoid simultaneous independent full-file scans of all stems when a more I/O-efficient order is measured.
- Resume automatically when safe.

Expose analysis throttling in diagnostics, not as a user error.

## Work package 9.4 — Source fingerprint

Create a filename-independent fingerprint using bounded source evidence, for example:

- Format/cache schema version.
- File size and selected first/last byte ranges for containers.
- Manifest canonical content plus routed file sizes and selected ranges for bundles.
- Strong hash where already declared and verified.

The policy must balance collision resistance, reopen speed, and privacy. Full paths and filenames are excluded from cache keys.

## Work package 9.5 — Cache format

Define an explicit versioned binary format:

- Magic and schema version.
- Source fingerprint.
- Sample rate and frame range.
- Role table.
- Level descriptors and offsets.
- Peak arrays.
- Integrity checksum.
- Optional consistency summary.

Use checked parsing and hard limits. A corrupt or unknown cache entry is deleted or ignored safely and never causes source rejection.

## Work package 9.6 — Transactional cache writes

- Write to a temporary sibling file.
- Flush as required by platform policy.
- Validate the completed file.
- Atomically rename into place.
- Never leave a valid-looking partial entry.
- Coordinate concurrent opens with a per-key lock outside the audio path.
- Recover stale temporary files on startup with bounded work.

Cache writes must not block shutdown indefinitely.

## Work package 9.7 — LRU and capacity

Implement:

- Default capacity, initially 512 MiB unless Phase 0 changes it.
- User enable/disable.
- Clear current entry.
- Clear all.
- LRU eviction by cache metadata.
- Maximum entries and directory scan bounds.
- Disk-full behavior.
- Multi-process safety policy.

The cache stores no PCM, encoded media, full paths, or manifest private metadata.

## Work package 9.8 — Native waveform renderer

Render four synchronized rows with:

- Shared absolute-frame viewport.
- Shared cursor.
- Level-of-detail selection.
- HiDPI-safe geometry.
- Zoom, scroll, fit, and seek.
- Bounded vertex/path creation per viewport.
- No full-track draw object allocation at maximum zoom-out.
- Accessible nonvisual seek alternative.

Cursor and peaks are separate layers so cursor movement does not rebuild peak geometry.

## Work package 9.9 — Consistency diagnostics

When a master exists, optionally measure blockwise:

`master - (drums + bass + other + vocals)`

Report peak, RMS, dBFS, frames examined, and non-finite counts. Apply manifest-declared thresholds only as diagnostics. Do not alter or correct playback.

Run this work under the same playback-priority constraints as waveform analysis.

## Work package 9.10 — Diagnostics system

Provide bounded, redacted diagnostics for:

- Session and source profile.
- Transport and generation.
- Source/output sample rates.
- Buffer occupancy.
- Underrun/overflow and ring wrap counts.
- Decode queues and range bytes.
- Mixer automation.
- Resampler delay.
- Waveform progress/cache.
- Device state.
- Recent recoverable errors.

Support copy/export to a local text or JSON file only after explicit user action. Redact paths and URLs by default.

## Deliverables

- Progressive peak analyzer.
- Four-level peak pyramid.
- Playback-priority scheduling.
- Private source fingerprint.
- Versioned transactional cache.
- LRU capacity management.
- Efficient native waveform renderer.
- Optional consistency diagnostics.
- Complete diagnostics panel and export.

## Tests and evidence

- Exact bucket counts and final partial bucket.
- Cache miss, write, hit, corruption, unknown version, and eviction.
- Concurrent open for same key.
- Disk-full and read-only cache directory.
- Playback while analysis is active with zero normal-load underruns.
- Analysis cancellation and source switch.
- Renderer zoom/scroll/fit and very long duration.
- Cache privacy inspection.
- Consistency measurement against exact synthetic sums and known residuals.

## Resource checks

- Peak arrays and cache sizes documented.
- Analysis memory bounded independently of duration except final peak arrays.
- Renderer allocations bounded by viewport.
- Cached reopen latency measured.
- Analysis CPU and I/O measured separately from playback.
- No playback callback or render-thread blocking on cache operations.

## Risks and mitigations

- **Analysis competes with playback:** central playback-priority scheduler and strict low-watermark response.
- **Fingerprint collision:** include strong enough content evidence and cache integrity fields.
- **Cache format becomes accidental API:** version explicitly and treat entries as disposable.
- **Waveform renderer burns CPU:** separate static peaks from cursor and repaint on demand.

## Exit gate

Phase 9 is complete when four-stem waveforms appear progressively, cached reopen is fast, cache contents meet privacy requirements, analysis cannot starve playback, rendering is viewport bounded, and diagnostics are useful without exposing private data or affecting real-time performance.

## Implementation record

### 2026-07-26 - Progressive analyzer, cache, renderer, and diagnostics core

Status: exit gate passed.

Implemented:

- Added immutable four-role min/max peak pyramids at 256, 1,024, 4,096, and 16,384 source frames with exact final partial buckets and progressive snapshots.
- Added independent analysis decoders that reuse the qualified Studio WAV and compressed-container source adapters without sharing playback decoder state.
- Scheduled one bounded analysis block after each playback pump and pause analysis below the playback low watermark.
- Added filename-independent SHA-256 source fingerprints from bounded head and tail evidence, sizes, modification times, and routed format descriptors. Studio fingerprints include the manifest and every routed file.
- Added a versioned checksum-protected binary cache with checked parsing, a 64 MiB record limit, configurable total capacity, bounded directory scans, stale temporary cleanup, in-process same-key serialization, transactional sibling writes, and oldest-entry eviction.
- Kept cache writes off the playback worker by committing completed immutable snapshots on a detached bounded write.
- Added cache enable, disable, clear-current, clear-all, miss, write, and hit paths. Cache enablement and capacity persist, restore, and apply immediately at runtime.
- Added four synchronized native waveform rows, separate playback cursor control, fit, zoom, scroll, nonvisual seek, and viewport projection capped at 4,096 columns. The production UI requests at most 800 columns per row.
- Added optional master-minus-stem peak, RMS, dBFS, examined-frame, and non-finite diagnostics without changing playback.
- Added bounded snapshot diagnostics for waveform progress, cache state, and playback-pressure throttling.
- Added explicit native diagnostics copy and JSON export actions. Export omits the private source display name and reports success or failure in the diagnostics panel.

Resource model:

- Analysis PCM is one routed block: at most 4,096 frames by 10 Float32 channels, about 160 KiB, and is released after peak extraction.
- Peak storage is 32 bytes per bucket across four roles. At 48 kHz, the four levels total about 28.7 MiB per hour of source duration.
- Cache records are capped at 64 MiB and the default cache directory cap remains 512 MiB.
- Fingerprinting reads at most the first and last 64 KiB of one file at a time.
- Viewport projection is capped at 4,096 columns per row; the player UI uses 800.

Verification:

| Check | Evidence | Result |
| --- | --- | --- |
| Formatting | `cargo fmt --all -- --check` after final rustfmt | PASS |
| Focused waveform and controller tests | `cargo test -p stemplayer-waveform -p stemplayer` | PASS, 22 passed and 1 hardware-gated test ignored |
| Workspace regression | `cargo test --workspace` | PASS, 139 passed and 4 environment-gated tests ignored |
| Workspace lints | `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| Production release build | `cargo build --release -p stemplayer` | PASS |
| Cache lifecycle | Unit and real-controller tests cover miss, write, hit, corruption deletion, hostile timeline and resolution rejection, unknown version deletion, eviction, concurrent in-process same-key writes, privacy strings, clear-current, clear-all, runtime capacity reduction, and cached reopen | PASS |
| Peak and consistency correctness | Tests cover exact buckets, final partial bucket, exact master sum, and known residual peak/RMS | PASS |
| Viewport bounds | Long-duration projection and zoomed viewport tests cap every row by pixel width | PASS |
| Analysis cancellation | A real controller source switch during progressive analysis publishes only the replacement generation and timeline | PASS |
| Direct Windows UI Automation | Open, P shortcut, Playing state, four waveform accessible rows, fit, zoom, viewport, and playback-position controls enumerated | PASS |
| Redacted diagnostics export | Focused app test serializes a private source name and verifies it is absent while sample-rate and waveform fields remain | PASS |
| Cache write failures | Injected `StorageFull` and `PermissionDenied` writes verify no temporary or valid-looking entry remains | PASS |
| Windows playback-under-analysis qualification | `scripts/qualify-phase9-windows.ps1` with a 30-second, 48 kHz, ten-channel Studio bundle and master | PASS, zero underruns and overflows during analysis and cached playback |
| Long-source resource qualification | The same harness with a 120-second, 5,760,000-frame source | PASS, 40,095,744-byte peak private memory and zero underruns and overflows |
| Cached reopen | Cache-hit process launch, audio startup, source open, and UI settle | PASS, 604.649 ms for 30 seconds and 568.228 ms for 120 seconds |
| Analysis cost | Windows process counters, one-core-normalized CPU p95 and I/O deltas against cached playback | PASS, 6.25 percentage-point and 42,569,203-byte overhead for 30 seconds; 18.75 percentage-point and 151,045,616-byte overhead for 120 seconds |
| Cache privacy and size | Binary inspection excludes fixture paths and routed filenames | PASS, 239,276 bytes for 30 seconds and 956,460 bytes for 120 seconds |
| Windows UI captures | `artifacts/phase9/windows/{normal,compact,zoomed,cached-reopen}.png` at 216 DPI | PASS, visually inspected with the complete compact interface visible |
| Legacy Phase 8 Windows harness | Replaced by `scripts/qualify-phase9-windows.ps1`, which uses cache completion and snapshots instead of SendKeys timing | SUPERSEDED |

Exit-gate evidence:

- Progressive four-stem waveforms and exact peak construction pass focused and controller tests.
- Cached reopen, bounded cache contents, corruption handling, failure cleanup, eviction, and privacy pass.
- Real Windows playback remains at zero underruns and overflows while analysis is active for both 30-second and 120-second sources.
- Renderer projection remains viewport bounded and normal, compact, zoomed, and cached-reopen UI states were captured and visually inspected.
- Diagnostics expose playback and waveform state and support explicit redacted copy and JSON export.
- Phase 9 exit gate passed on 2026-07-26.

### 2026-07-26 - Phase 9 started

Status: in progress.

Plan:

- Implement the bounded four-level analyzer, source fingerprint, transactional cache, eviction, playback-pressure policy, viewport projection, consistency measurement, and redacted diagnostics in the existing waveform crate.
- Reuse the engine's source adapters and controller cancellation boundary for progressive analysis.
- Wire four synchronized waveform rows and cache controls into the Phase 8 UI without adding an idle repaint loop.
- Qualify cache privacy and lifecycle, analysis cancellation and playback priority, viewport bounds, diagnostics, and workspace regressions before evaluating the exit gate.
