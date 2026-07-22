# Studio WAV v1

Status: Phase 0 native client profile, version 1.

## Input

A selected local bundle contains exactly one case-insensitive, NFC-normalized `manifest.json`, four required WAV files for `drums`, `bass`, `other`, and `vocals`, and zero or one `master`. Unreferenced files are ignored and never executed.

The manifest is fatal UTF-8 JSON, at most 1,048,576 bytes, depth at most 32, and at most 10,000 values. Keys `__proto__`, `prototype`, and `constructor` are forbidden. `schema_version` is integer 1 or 2. `outputs` is an array.

Recognized output fields are:

- `logical_type`: required role string; `instrumental` and `acapella` entries are ignored.
- `filename`: required NFC basename.
- `sample_rate`: optional finite number that must match the WAV.
- `channels`: optional finite number that must equal 2 and match the WAV.
- `frames`: optional positive safe integer that must match the WAV.
- `subtype`: optional diagnostic string.
- `sha256`: optional 64 hexadecimal characters, compared case-insensitively and stored lowercase.

Unknown top-level fields are diagnostic only. Unknown fields cannot cause file or network access.

## Path And Name Rules

Filenames are basenames, not paths or URLs. Empty names, non-NFC names, absolute paths, separators, drive prefixes, URL schemes, `.`/`..`, control characters, and network paths are rejected. Matching within the explicitly selected group is NFC-normalized and case-insensitive. Duplicate normalized names are ambiguous and rejected.

## WAV Profile

- Little-endian RIFF/WAVE only; RF64 is rejected.
- File and declared RIFF sizes must fit unsigned 64-bit offsets.
- At most 4,096 chunks; exactly one usable `fmt ` and one `data` chunk.
- `fmt ` length is 16 through 65,536 bytes.
- Stereo at exactly 44,100 or 48,000 Hz.
- PCM integer format 1 at 16, 24, or 32 bits, or IEEE Float format 3 at 32 bits.
- Byte rate, block alignment, chunk padding, data alignment, and all range additions are checked.
- Unknown chunks are skipped by validated offsets.

All routed tracks have the same sample rate and exact frame count. There is no resampling, channel conversion, trimming, padding, or mismatch repair.

## Numeric Behavior

Blocks contain at most 4,096 frames and decode to planar stereo Float32:

- PCM16: signed value divided by 32,768.
- PCM24: sign-extended signed value divided by 8,388,608.
- PCM32: signed value divided by 2,147,483,648.
- Float32: copied unchanged, including finite values outside `[-1, 1]`.

Non-finite Float32 fails decoding. No normalization, clipping, dither, or full-track PCM cache is allowed. The timeline is `[0, common_frame_count)`.

## Rejection Codes

`E_MANIFEST_INVALID`, `E_MANIFEST_VERSION_UNSUPPORTED`, `E_MANIFEST_TOO_LARGE`, `E_MANIFEST_ROLE_MISSING`, `E_MANIFEST_ROLE_DUPLICATE`, `E_MANIFEST_FILE_MISSING`, `E_MANIFEST_FILE_AMBIGUOUS`, `E_MANIFEST_PATH_UNSAFE`, `E_MANIFEST_SHA256_MISMATCH`, `E_WAV_NOT_RIFF`, `E_WAV_MALFORMED`, `E_WAV_FORMAT_UNSUPPORTED`, `E_WAV_CHANNELS_UNSUPPORTED`, `E_WAV_SAMPLE_RATE_MISMATCH`, `E_WAV_FRAME_COUNT_MISMATCH`, `E_WAV_BLOCK_ALIGN_INVALID`, `E_WAV_DATA_TRUNCATED`, `E_WAV_PCM_INVALID`, `E_BUNDLE_INCONSISTENT`, and `E_BUNDLE_ABORTED`.

## Security And Compatibility

Files are opened only after manifest validation and only from the explicit local selection. SHA-256 is streamed with bounded buffers. Paths and manifest metadata are redacted from crash reports. Schema 1 and 2 are accepted because current published bundles use them; a new schema requires a new profile version or explicit compatible amendment.
