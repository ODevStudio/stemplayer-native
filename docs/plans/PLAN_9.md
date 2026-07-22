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
