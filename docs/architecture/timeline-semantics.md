# Timeline Semantics v1

## Coordinate System

One session uses one absolute signed 64-bit output-frame timeline at the common source rate. Valid ranges are half-open `[start_frame, end_frame_exclusive)`. Duration is their checked difference. Frame counters must not exceed `i64::MAX`.

Time conversion uses checked integer rational arithmetic. For non-negative `value * destination_rate / source_rate`, rounding is floor unless a format rule explicitly says otherwise. Floating-point seconds never drive seek, alignment, end, or buffer publication.

## Track Mapping

Each routed track provides media timescale, media duration, source rate, and zero or more edits.

- Positive integer timescales and duration are required.
- No edit maps media frame 0 through the scaled media end.
- Edit rate must be exactly 1.
- Edit durations are positive.
- Media time `-1` maps explicit presentation silence; values below `-1` are unsupported.
- Media ranges are checked, non-overlapping, and inside media duration.
- Priming is the first mapped media start; padding is the unused media tail after the final mapped range.
- Silence exists only for explicit empty edits.

Every required role appears exactly once. Four or five track maps have one sample rate and identical start/exclusive-end ranges. Mismatch fails; no track is stretched, resampled, trimmed, or padded to match another.

## PCM Publication

Decoded blocks carry generation, role, absolute start frame, and frame count. Per-role normalizers reject gaps, overlaps, discontinuities, non-finite values, and out-of-range blocks. A synchronized publisher emits only a common range available for all roles, in fixed order `master?`, `drums L/R`, `bass L/R`, `other L/R`, `vocals L/R`.

Blocks are bounded to 4,096 frames initially. Queues have explicit capacities. Complete-track PCM is never retained.

## Seek And End

Seek accepts an absolute frame or an exactly converted microsecond value, clamps before dispatch, increments generation, flushes all source/output rings, decodes from the nearest safe packet boundary, discards until the target frame, and publishes only the target generation. Repeated seek to one frame is deterministic.

End occurs only after every routed track has published through the common exclusive end and the device callback has consumed it. Missing or extra decoded frames return `E_PLAYBACK_END_MISMATCH`.

## Errors

`E_TIMESCALE_INVALID`, `E_EDIT_LIST_UNSUPPORTED`, `E_EDIT_LIST_CONFLICT`, `E_PRESENTATION_RANGE_INVALID`, `E_PCM_GAP`, `E_PCM_OVERLAP`, `E_PCM_ALIGNMENT_MISMATCH`, `E_PCM_LENGTH_MISMATCH`, `E_ASSEMBLER_BUFFER_LIMIT`, `E_GENERATION_MISMATCH`, and `E_PLAYBACK_END_MISMATCH`.

## Proof

Tests use rational vectors with non-divisible timescales, empty edits, multiple edits, boundary seeks, maximum safe counters, generation races, cross-track mismatch, exact final blocks, and complete sequential-versus-seeked PCM hashes.
