# Phase 7 — Native Stem MP4, AAC-LC, and ALAC Playback

## Goal

Add bounded, sample-synchronous playback of qualified local `.stem.m4a` and `.stem.mp4` containers without FFmpeg, browser codecs, or full-file PCM decoding. Preserve role resolution, edit-list mapping, exact transport, mixer, and end-frame behavior.

## Entry criteria

- Phases 5 and 6 provide stable source-ring, transport, mixer, and resampling interfaces.
- Phase 0 container profile and codec dependency decisions are approved.
- Synthetic and private qualification fixtures are available.
- Decoder licensing and distribution review is complete.

## Work package 7.1 — MP4 parser boundary

Implement or adapt a bounded ISO BMFF reader that exposes only required information:

- File type and compatible brands.
- Movie and track timescales.
- Audio track descriptors.
- Codec configuration.
- Sample tables and byte ranges.
- Edit lists.
- Enabled flags.
- Metadata used for role resolution.
- Sample count, duration, and encoded-byte totals.

All offsets and sizes use checked arithmetic. Box depth, box count, metadata bytes, tracks, edits, samples, and allocation sizes have explicit limits.

## Work package 7.2 — Role resolution

Reproduce the approved role-resolution hierarchy:

- Native Instruments or stem-specific metadata when valid.
- Qualified track metadata.
- Standard order only when unambiguous and permitted.
- Explicit failure for ambiguous or unresolved mappings.

Required output is exactly four unique stem roles and optional unique master. Unknown tracks may be ignored only if the specification permits and they cannot affect ambiguity.

Extension does not determine container validity. A valid supported MP4 may be accepted under either qualified extension; a renamed invalid file is rejected by content.

## Work package 7.3 — Timeline and edit lists

Map every decoded packet onto one absolute output-frame timeline:

- Interpret qualified edit-list patterns.
- Convert timescales using rational arithmetic.
- Detect unexplained gaps and overlaps.
- Trim only when container evidence proves the mapping.
- Reject incompatible per-track timelines.
- Produce common start and exclusive end frames.
- Record priming and padding as resolved or unresolved with evidence.

Do not independently “make tracks line up” by resampling, padding, or time stretching.

## Work package 7.4 — Packet indexing and range reads

Build a bounded packet/sample index sufficient for random access:

- Read metadata in bounded chunks.
- Retain compact sample tables, not encoded file contents.
- Resolve packet byte ranges safely.
- Group nearby range reads where measured to help I/O.
- Reuse parsed tables across seeks.
- Cancel reads on generation change.
- Detect truncation and mutation.

Record metadata-read bytes, media-read bytes, and peak pending packet bytes.

## Work package 7.5 — AAC-LC decoder adapter

Implement a decoder adapter with:

- Qualified AAC-LC configurations only.
- Stereo output at 44.1 or 48 kHz according to the profile.
- One decoder instance per active track or an evidence-backed pool policy.
- Bounded decode queues.
- Exact packet timestamp association.
- Decoder flush/reset behavior defined for seek.
- Explicit handling of priming evidence.
- Float32 output conversion without normalization.

Unsupported AAC profiles return a stable feature error.

## Work package 7.6 — ALAC decoder adapter

Implement the corresponding ALAC path:

- Qualified MP4/ALAC configuration validation.
- Stereo output.
- Supported sample depths documented.
- Bounded packet decoding.
- Exact frame accounting.
- Seek reset behavior.
- Corrupt packet detection.

AAC and ALAC adapters expose the same engine-facing trait and diagnostics.

## Work package 7.7 — Synchronized assembler

For each requested common frame window:

- Request packets from every routed track.
- Decode into bounded per-track assemblers.
- Map decoded frames to absolute output frames.
- Trim proven boundary excess.
- Reject unexplained missing/duplicate frames.
- Publish complete synchronized 8/10-channel blocks only.

Assembler state is generation-scoped and has explicit maximum pending frames and blocks.

## Work package 7.8 — Seek strategy

Implement exact frame seeks with decoder preroll where required:

- Find the packet or sync point preceding the target.
- Decode bounded preroll.
- Discard frames before the absolute target.
- Prime the source ring to the start watermark.
- Preserve exact target frame in the session clock.
- Record seek packet count, decoded discard frames, and latency.

Do not expose a seek as complete until current-generation PCM beginning at the target is available.

## Work package 7.9 — Integration and diagnostics

Add source selection for `.stem.m4a` and `.stem.mp4`. UI and probe report:

- Container type.
- Codec per track.
- Role and metadata source.
- Sample rate and channels.
- Edit count.
- Priming/padding status.
- Start/end frames.
- Decoder queue maxima.
- Range-read bytes.
- Seek diagnostics.

No raw metadata or full paths are shown by default.

## Work package 7.10 — Decoder fallback policy

Do not launch FFmpeg or platform media players as a silent fallback. If a codec/profile is unsupported:

- Fail with a precise error.
- Show detected codec/profile.
- Explain the supported profile.
- Preserve application stability and loaded-source cleanup.

A future optional plugin system requires a separate security and distribution plan.

## Deliverables

- Bounded MP4 inspector.
- Deterministic role resolver.
- Edit-list and absolute timeline mapper.
- Packet index and range reader.
- AAC-LC adapter.
- ALAC adapter.
- Synchronized compressed-audio assembler.
- Exact seek implementation.
- Container diagnostics and UI support.

## Tests and evidence

- Valid AAC-LC and ALAC fixtures.
- Master present and absent.
- Every role-resolution source and ambiguous cases.
- Multiple MP4 timescales.
- Qualified edit-list patterns.
- Gaps, overlaps, corrupt tables, truncated packets, and overflow attacks.
- Beginning, middle, near-end, and randomized seek windows.
- Exact common first/last frame.
- 30-minute accelerated drift test.
- Complete-track real-time soak.
- Renamed extension and content mismatch cases.
- Decoder reset and cancellation stress.
- Human listening for channel routing, clicks, priming, and end behavior.

## Resource checks

- Metadata memory bounded by sample-table limits, not PCM duration.
- PCM ring remains duration-independent.
- Decoder queues and assembler blocks have enforced caps.
- AAC/ALAC CPU and memory measured against budgets.
- Seek p95 measured separately for SSD and slower storage.
- No external process is spawned.

## Risks and mitigations

- **Decoder library lacks gapless semantics:** keep edit/priming/timeline policy outside the decoder.
- **Large files create huge sample tables:** enforce profile limits and compact representations.
- **Role metadata is inconsistent:** fail ambiguous inputs instead of guessing.
- **Seek requires excessive preroll:** measure and optimize packet lookup while preserving exactness.

## Exit gate

Phase 7 is complete when qualified local stem containers play natively with exact synchronized frames, deterministic role and timeline handling, bounded memory and queues, no external codec process, stable seeks, and complete diagnostics for unsupported or ambiguous sources.
