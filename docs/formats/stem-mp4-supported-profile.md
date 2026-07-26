# Stem MP4 Supported Profile

Status: Phase 0 profile. Decoder acceptance remains provisional until Phase 7 exact-frame qualification.

## Input And Limits

- Local regular file ending in `.stem.m4a` or `.stem.mp4`; extension is a hint, not trust evidence.
- ISO Base Media File Format parsed with checked 64-bit offsets.
- Initial file limit: 16 GiB.
- Initial structural limits: 1,000,000 boxes, nesting depth 32, 16 tracks, 1 MiB aggregate recognized stem metadata, 256-byte retained track names, and 100,000 edit entries total.
- Exactly four resolved stereo stem tracks and zero or one stereo master track.
- Every routed track uses one source sample rate, either 44,100 or 48,000 Hz.

Unknown boxes are skipped only after their ranges are validated. Unsupported video, metadata, or extra audio tracks never become routes.

## Role Resolution

Resolution order is Native Instruments metadata, normalized track names, then the established five-track order `master`, `drums`, `bass`, `other`, `vocals` only when all names are unknown. Names use Unicode NFKC, trim, and locale-independent lowercase comparison. Each required role must occur exactly once; master occurs at most once. Any malformed metadata, incompatible channel/rate layout, duplicate role, partial naming, or disagreement is ambiguous and fails closed.

## Codec Profile

Qualified targets are AAC-LC and 16, 20, 24, or 32-bit ALAC in MP4. ALAC requires an explicit bounded codec configuration. Exact accepted object types, decoder configuration fields, packet layouts, gapless metadata, and channel layouts are frozen from public synthetic vectors in Phase 7. A successful demux or first decoded packet is not sufficient qualification.

Required decoder evidence per codec is exact frame count, deterministic repeated seek, packet-boundary behavior, edit-list behavior, bounded working memory, no inter-track resampling, and identical output across complete sequential and seeked decode.

## Timeline

All arithmetic is checked integer/rational arithmetic converted once to the source output-frame timeline.

- No edit list maps media from frame 0 to its scaled media end and leaves priming/padding unresolved.
- Edit rate must be exactly 1.
- An edit duration is positive and within media/movie timescale limits.
- Media time `-1` creates explicit presentation silence; other negative media times are rejected.
- Media edits may not overlap or exceed decoded media.
- All five/four track maps must produce the same start and exclusive end frame.
- No inferred padding or silence is inserted outside an explicit supported edit.

## Numeric Behavior

Decoded PCM becomes planar stereo Float32 without clipping or normalization. Source tracks are never resampled against one another. The common session layout is 8 channels without master and 10 with master. Full-track PCM caching is forbidden.

## Rejection Codes

Container: `E_CONTAINER_NOT_MP4`, `E_CONTAINER_MALFORMED`, `E_NO_AUDIO_TRACKS`, `E_NOT_A_STEM_FILE`, `E_TRACK_COUNT_UNSUPPORTED`, `E_TRACK_ROLE_AMBIGUOUS`, `E_CHANNEL_LAYOUT_UNSUPPORTED`, `E_SAMPLE_RATE_MISMATCH`, `E_METADATA_MALFORMED`, `E_METADATA_TOO_LARGE`.

Codec: `E_CODEC_UNSUPPORTED`, `E_CODEC_CONFIG_INVALID`, `E_DEMUX_FAILED`, `E_ENCODED_SAMPLE_INVALID`, `E_DECODE_FAILED`, `E_DECODE_QUEUE_OVERFLOW`, `E_AUDIO_DATA_INVALID`, `E_TRACK_CONFIGURATION_MISMATCH`.

Timeline: `E_TIMESCALE_INVALID`, `E_EDIT_LIST_UNSUPPORTED`, `E_EDIT_LIST_CONFLICT`, `E_PRESENTATION_RANGE_INVALID`, `E_PCM_GAP`, `E_PCM_OVERLAP`, `E_PCM_ALIGNMENT_MISMATCH`, `E_PCM_LENGTH_MISMATCH`.

## Security And Compatibility

Inspection performs local bounded random reads and does not decode. Decoder errors are mapped at the adapter boundary. Diagnostics retain basenames only, never full paths. Supporting another codec, role layout, edit rate, or arbitrary MP4 audio requires a new profile revision.
