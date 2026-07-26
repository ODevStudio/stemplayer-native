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

## Implementation record

### 2026-07-24 - Work package 7.1 started

Status: in progress.

Implemented:

- Added a bounded ISO BMFF structural inspection boundary using checked 64-bit ranges.
- Enforced the 16 GiB file, 1,000,000 box, 32-level nesting, 16-track, and bounded file-type limits from the approved profile.
- Added stable not-MP4 and malformed-container errors.
- Extracted major and compatible brands without decoding media or retaining file contents.

Verification:

| Check | Evidence | Result |
| --- | --- | --- |
| Workspace tests | `rtk cargo test --workspace` | PASS, 105 passed and 2 ignored across 30 suites |
| Lints and format | `rtk cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check` | PASS |
| Structural inspection | Synthetic valid file type/movie/track boxes plus truncated range and non-MP4 rejection | PASS |

Pending:

- Movie and track descriptors, codec configuration, sample tables, edit lists, enabled flags, and stem metadata.
- Role resolution, timeline mapping, packet indexing, AAC-LC/ALAC adapters, synchronized assembly, seek integration, diagnostics, and qualification evidence.

### 2026-07-24 - Role, timeline, and codec foundation

Implemented:

- Added deterministic Native Instruments metadata, normalized track-name, and qualified five-track order role resolution.
- Enforced exactly four unique stem roles, at most one master, unique track identifiers, 256-byte retained values, complete metadata, and disagreement rejection.
- Added checked edit-list mapping for unit-rate media edits, explicit empty edits, exact source-frame alignment, decoded-media bounds, non-overlap, and common routed-track ranges.
- Added bounded AAC-LC and ALAC track inspection and per-track decoding through the Phase 0-approved Symphonia 0.6 dependency.
- Added exact beginning, middle, and deterministic randomized window seeks, generation cancellation, decoder reset, packet timestamp association, Float32 conversion, stereo/rate validation, finite-sample checks, and bounded packet/config/decode sizes.
- Added decoder diagnostics for packets, encoded bytes, discarded preroll, and actual/required seek frames.

Verification:

| Check | Evidence | Result |
| --- | --- | --- |
| Workspace regression | rtk cargo test --workspace | PASS, 110 passed and 4 ignored across 30 suites |
| Lints and format | rtk cargo clippy --workspace --all-targets -- -D warnings; cargo fmt --all -- --check | PASS |
| Local .stem.mp4 | Bounded structure inspection plus all-track 4,096-frame decode at beginning, midpoint, and 16 deterministic randomized seeks | PASS |
| Local .stem.m4a | Bounded structure inspection plus all-track 4,096-frame decode at beginning, midpoint, and 16 deterministic randomized seeks | PASS |
| Codec contract | AAC-LC profile, 44.1/48 kHz, stereo, packet/config limits, and unsupported-profile tests | PASS |
| Timeline contract | No-edit, leading-empty-edit, exact media trim, invalid rate/range/alignment, and cross-track conflict tests | PASS |

Pending:

- Extract track names, Native Instruments metadata, movie/media timescales, edit lists, enabled flags, and compact sample tables from real containers.
- Connect resolved container tracks and timelines to the synchronized source assembler and production playback session.
- Complete near-end, in-flight cancellation, drift, soak, resource, malformed-container, and listening qualification.

### 2026-07-24 - Bounded timing and sample-table descriptors

Implemented:

- Extended structural inspection with movie timescale, track identifier, enabled state, audio handler, media timescale/duration, edit entries, encoded sample count, and encoded-byte total.
- Enforced direct movie-child placement, unique headers and handlers, nonzero track identifiers and timescales, 100,000 aggregate edits, 100,000,000 samples per track, exact table ranges, and checked encoded-byte accumulation.
- Kept variable sample-size reads bounded to 64 KiB without retaining encoded media or per-sample sizes.
- Added synthetic coverage for version 0 and version 1 timing fields, edit extraction, variable sample sizes, and oversized sample-count rejection.

Verification:

| Check | Evidence | Result |
| --- | --- | --- |
| Workspace regression | `rtk cargo test --workspace` | PASS, 111 passed and 4 ignored across 30 suites |
| Lints and format | `rtk cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check` | PASS |
| Bounded MP4 descriptors | Synthetic timing, handler, edit-list, sample-size, encoded-byte, truncation, and limit cases | PASS |

Pending:

- Extract track names, Native Instruments metadata, codec configuration, and compact packet byte ranges from real containers.
- Connect parsed descriptors to role resolution, timeline mapping, synchronized assembly, and the production playback session.
- Complete near-end, in-flight cancellation, drift, soak, resource, malformed-container, and listening qualification.

### 2026-07-24 - Native AAC and ALAC container playback integrated

Status: in progress.

Implemented:

- Parsed bounded `stsz`, `stco`/`co64`, `stsc`, and `stts` tables into compact sample-size, chunk, and timing runs. Packet lookup uses checked byte ranges and binary timing lookup without retaining encoded media.
- Parsed bounded track names and Native Instruments JSON metadata. The parser limits bytes, nesting, node count, retained strings, tracks, edits, samples, chunks, timing runs, and aggregate table memory.
- Accepted bounded ISO BMFF handler names as qualified track metadata, with direct track names taking precedence.
- Joined structural inspection, role resolution, codec inspection, and edit-list mapping into one reusable compressed-container descriptor.
- Added file identity checks before inspection and every decode window. Symlinks, non-files, truncation, and replacement after inspection fail closed.
- Added persistent per-track AAC-LC/ALAC decoder instances, exact packet timestamp checks, bounded packet and decoded-frame limits, finite Float32 validation, cancellation checks, and retained partial packets for adjacent windows.
- Added 2,048 frames of bounded AAC seek preroll. Fresh seeks now reproduce uninterrupted adjacent-window samples before trimming to the requested absolute frame.
- Validated ALAC configuration data, stereo layout, 44.1/48 kHz rate, and 16, 20, 24, or 32-bit sample depth. The adapter reads depth from Symphonia parameters or the standard bounded 24/36-byte ALAC configuration. AAC-LC and ALAC share the same decoder interface and diagnostics.
- Added the compressed source to the production session and scheduler. The assembler publishes complete synchronized 8/10-channel blocks, applies edit-list segments, emits silence only for explicit empty edits, and rejects incomplete coverage.
- Added local container selection, source diagnostics in the native UI, structured container inspection and seek commands in the probe, and content-based rejection of renamed invalid files.
- Added metadata/media byte counters, queue maxima, packet/discard counters, and seek latency to snapshots without exposing full paths or raw metadata.
- Confirmed production playback contains no external codec or media-player process launch.

Verification:

| Check | Evidence | Result |
| --- | --- | --- |
| Workspace regression | `cargo test --workspace` | PASS, 115 passed and 4 environment-gated tests ignored across 30 suites |
| Lints and format | `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check` | PASS |
| Real AAC `.stem.mp4` structure | Five 44.1 kHz stereo AAC-LC tracks, Native Instruments master/drums/bass/other/vocals roles, edit lists, 0 through 22,583,610 common frames, 463,790 metadata bytes read | PASS |
| Real AAC `.stem.m4a` structure | Five 44.1 kHz stereo AAC-LC tracks, Native Instruments master/drums/bass/other/vocals roles, no edits, 0 through 22,590,464 common frames, 463,918 metadata bytes read | PASS |
| Synthetic ALAC, master present | Five 16-bit 44.1 kHz stereo tracks, standard-order roles, edit lists, 0 through 88,200 common frames | PASS, exact windows, 16 randomized seeks, and exact full playback to `ENDED` |
| Synthetic ALAC, master absent | Four 16-bit 44.1 kHz stereo tracks, handler-name roles, edit lists, 0 through 88,200 common frames | PASS, exact windows, 16 randomized seeks, and exact full playback to `ENDED` |
| Exact decoder windows | Beginning, midpoint, near-end, adjacent sequential/fresh seek equality, cancellation, and 16 deterministic randomized repeated seeks on every track of both AAC fixtures | PASS |
| Accelerated complete playback | Both AAC fixtures reached their exact exclusive end frame with finite first/last samples, one pending source block, and zero ring overflow or stale-generation rejection | PASS |
| Accelerated drift | Four complete `.stem.mp4` runs, 34.14 minutes and 90,334,440 rendered frames | PASS, zero end-frame error, overflow, or stale blocks |
| Resource measurement | 512.1-second `.stem.mp4`: 5.65625 CPU seconds, 1.105% equivalent single-core use, 17.31 MiB peak private memory, 22.26 MiB peak working set | PASS |
| ALAC resource measurement | 60-second, five-track synthetic ALAC: 0.7031 CPU seconds, 13.05 MiB peak private memory, 18.59 MiB peak working set, 2,646,000 exact rendered frames | PASS, zero overflow or stale blocks |
| Seek measurement | 19 release seeks per AAC fixture, 256-frame synchronized windows | PASS; `.stem.mp4` p95/max 25,250 us, max 340 packets and 15,355 discarded frames; `.stem.m4a` p95/max 32,478 us, max 340 packets and 15,290 discarded frames |
| ALAC seek measurement | 19 release seeks per synthetic fixture, 256-frame synchronized windows | PASS; five-track p95/max 24,354 us and max 85 packets; four-track p95/max 18,615 us and max 68 packets |
| Content mismatch | Invalid content renamed to `.stem.m4a` | PASS, `E_CONTAINER_NOT_MP4` |
| Bounded failure cases | Invalid box ranges, table counts, chunk overlaps, timing disagreement, metadata structure, unsupported codec/profile, non-finite PCM, cancellation, and local-file replacement | PASS |
| Encoded corruption and truncation | Temporary local copies with one AAC packet zeroed, one ALAC packet zeroed, and the AAC file shortened below declared chunk ranges | PASS, `E_DECODE_FAILED` and `E_CONTAINER_MALFORMED`; temporary copies removed after the checks |
| Native-device real-time soak and listening | Operator qualification on 2026-07-24 | PASS, including routing, clicks, priming, and end behavior |
| Slower-storage seek benchmark | Operator qualification on 2026-07-24 | PASS |

The ALAC qualification files were authored once with the locally installed FFmpeg executable, then inspected, decoded, sought, rendered, and rejected when corrupt through the native Rust binaries only. The application, probe, Cargo workspace, and tests do not invoke or depend on FFmpeg.

Exit gate: passed. Qualified AAC-LC and ALAC containers have exact synchronized playback and seeks, bounded memory and queues, deterministic role and timeline handling, complete diagnostics, native-device soak and listening evidence, slower-storage seek evidence, and no external runtime codec process.
