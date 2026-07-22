# Fixture And Provenance Policy

## Classes

1. Public synthetic fixtures are small deterministic files committed with source generator, license, expected SHA-256, and expected parse/decode results.
2. Generated qualification fixtures are created during tests into a temporary directory from committed generators and seeds. The output is not committed unless needed for a stable regression.
3. Private listening fixtures stay outside version control and are referenced by explicit environment variables. Reports store a fixture alias, size, and approved hash, never a path, filename, title, or tags.

No class may contain unlicensed commercial audio. A fixture author records authorship/source, generator version, license, purpose, sample rate, channel layout, duration, and SHA-256 in `fixtures/manifest.json`.

## Determinism

Generators use pinned inputs, integer phase/counter sources where possible, fixed seeds, explicit byte order, and no wall clock, locale, host path, or random device. A regeneration check hashes every public output and fails on drift.

## Initial Public Matrix

- Valid 44.1/48 kHz PCM16, PCM24, PCM32, and Float32 stereo WAVs.
- Negative/full-scale/out-of-range finite Float32 values and non-finite rejection vectors.
- Odd unknown RIFF chunk padding, chunk-order variants, truncated headers/chunks/data, multiple data chunks, invalid byte rate/alignment, RF64, mono, multichannel, unsupported rates, and compressed WAV.
- Studio manifests for schemas 1/2, optional master/hash, malformed UTF-8/JSON, byte/depth/node caps, forbidden keys, missing/duplicate roles, unsafe names, Unicode NFC/case collisions, missing/ambiguous files, hash/rate/frame mismatch.
- MP4 with no edit, leading empty edit, media-time priming, explicit end padding, multiple non-overlapping edits, invalid rate, overlap, range overflow, AAC/ALAC packet-boundary cases, malformed boxes, role ambiguity, and optional master.
- HLS manifest/playlist trees for aligned routes, no master, 1,024-frame preroll, malformed UTF-8/JSON, missing roles, duration/boundary mismatch, traversal, encoded separators, origin escape, redirects, timeout, oversize, duplicate preroll, corrupt segment, and hash mismatch.
- Waveform level boundaries, partial final peak, values outside `[-1, 1]`, non-finite input, cache corruption/version/eviction/privacy, and exact-end vectors.

## Private Variables

- `STEMPLAYER_NATIVE_AAC_FIXTURE`
- `STEMPLAYER_NATIVE_ALAC_FIXTURE`
- `STEMPLAYER_NATIVE_STUDIO_BUNDLE`
- `STEMPLAYER_NATIVE_HLS_URL`

Tests skip private listening checks with an explicit `not_tested` record when a variable is absent. A missing private fixture never weakens public structural gates.

## Handling

Private media is opened read-only and never copied into repository, build output, crash records, caches, CI artifacts, screenshots, or logs. Tests do not enumerate parent directories. Hashing streams bounded blocks. Temporary decoded data is process-local and deleted on close; full PCM is never written.

## Review Gate

A public fixture change requires the manifest entry, source/generator, license, expected hash, expected behavior, and regeneration check in one review. A private fixture result requires operator confirmation that the media may be used locally for qualification.
