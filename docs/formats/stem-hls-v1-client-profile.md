# Stem HLS v1 Client Profile

Status: Phase 0 client contract for later Phase 10 implementation.

## Entry URL

Network access occurs only after an explicit user open action. Production uses HTTPS. HTTP is allowed only for an explicit loopback development URL. `file`, `data`, `blob`, credentials in URLs, fragments, and unsupported schemes are rejected. Requests omit cookies and authentication by default, do not follow redirects, and time out after 15 seconds.

## Stemset JSON

The response is fatal UTF-8 JSON and at most 256 KiB. Root keys are closed to the versioned schema. Required values are:

- `schema` and `format_version`: `stem-hls-v1`.
- `manifest_version`: integer 1.
- `title`: non-empty string, retained to 512 Unicode scalar values.
- `duration_us`: positive safe integer.
- `sample_rate`: 44,100 or 48,000.
- `channels`: 2.
- `segment_duration_us` and `segment_count`: positive safe integers.
- `master_present`: boolean matching the presence of `master`.
- `codec`: AAC-LC, codec string `mp4a.40.2`, segment format `M4A`, and positive `bitrate_bps`.
- `stems`: exactly `drums`, `bass`, `other`, and `vocals`.
- Each route: `playlist` and `waveform` relative references.
- Optional `technical.segment_preroll_frames`: absent/0 or exactly 1,024.

At most five routes and 100,000 segments are accepted. Integer additions, duration sums, and frame conversion are checked.

## Child References

Playlist, waveform, and segment references are NFC relative URL paths on the stemset origin. Reject absolute paths, schemes, backslashes, empty components, `.` or `..`, decoded separators, NUL/control characters, queries, fragments, invalid percent encoding, origin changes, and duplicate normalized references.

## Playlists

Each playlist is fatal UTF-8 and at most 1 MiB. It begins with `#EXTM3U`, contains `#EXT-X-ENDLIST`, has no `#EXT-X-STREAM-INF`, and pairs every `#EXTINF` with one following segment URI. Duration has decimal microsecond precision of at most six digits and is positive. Media sequence is a non-negative safe integer, default 0.

Every route has identical media sequence, segment count, per-index duration, and total duration. Playlist duration matches `duration_us` within 1,000 microseconds. Adaptive and live HLS are not supported.

## Waveforms

Each waveform response is at most 2 MiB and schema `stem-waveform-v1`. `samples_per_peak` is positive. `min` and `max` arrays have equal length, at most 1,000,000 entries, contain finite numbers, and satisfy `min <= max`. All four stem waveforms have identical `samples_per_peak` and array length. The native client may later support multiple levels, but this input level is validated before allocation.

## Segments And Preroll

Encoded segments are fetched in bounded groups with explicit maximum response bytes derived from declared bitrate, duration, and a capped tolerance. Group publication waits for every active role. A non-first segment with declared preroll discards exactly 1,024 decoded frames before publication. The first segment discards none. Group boundaries must remain exact across all roles and produce zero inter-stem offset.

No segment bytes or decoded PCM are retained beyond the bounded playback/cache policy. HTTP range support is optional for complete segment files and mandatory if a future profile performs ranged media reads.

## Rejection Codes

`E_HLS_STEMSET_INVALID`, `E_HLS_ROLE_MISSING`, `E_HLS_TIMELINE_MISMATCH`, `E_HLS_SEGMENT_BOUNDARY_MISMATCH`, `E_HLS_URL_UNSAFE`, `E_HLS_RESPONSE_TOO_LARGE`, `E_HLS_HTTP_FAILED`, `E_HLS_TLS_FAILED`, `E_HLS_REDIRECT_REJECTED`, `E_HLS_GROUP_BUFFERING_TIMEOUT`, `E_HLS_HASH_MISMATCH`, and `E_HLS_CODEC_UNSUPPORTED`.

## Privacy And Compatibility

No request is made at startup or for local sources. URLs in user-facing diagnostics are reduced to origin plus redacted path shape unless explicitly requested. TLS uses a maintained Rust implementation and a documented trust-root policy. Live HLS, adaptive variants, cookies, DRM, arbitrary codecs, and uploads require a new profile and are not compatible extensions.
