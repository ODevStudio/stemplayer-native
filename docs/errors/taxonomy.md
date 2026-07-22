# Native Error Taxonomy v1

## Shape

Every failure has `code`, `category`, `message`, optional structured `details`, and `recoverability`. Codes are stable uppercase ASCII. Messages are actionable and path-redacted. Details use bounded scalar/list values and are never required for control flow.

Internal library errors are mapped once at crate boundaries. Panic is not a normal error. Unknown internal errors become `E_INTERNAL` with a local correlation identifier and no source text in the user message.

## Categories

| Prefix | Category | Initial codes |
| --- | --- | --- |
| `E_TRANSPORT_`, `E_PLAYBACK_` | Lifecycle and transport | `E_TRANSPORT_STATE_INVALID`, `E_PLAYBACK_SESSION_CLOSED`, `E_PLAYBACK_NOT_READY`, `E_PLAYBACK_ALREADY_ACTIVE`, `E_PLAYBACK_PRIMING_FAILED`, `E_PLAYBACK_SEEK_FAILED`, `E_PLAYBACK_END_MISMATCH`, `E_PLAYBACK_ABORTED`, `E_PLAYBACK_FEATURE_UNSUPPORTED` |
| `E_AUDIO_` | Device and stream | `E_AUDIO_DEVICE_MISSING`, `E_AUDIO_DEVICE_DISCONNECTED`, `E_AUDIO_CONFIG_UNSUPPORTED`, `E_AUDIO_STREAM_OPEN_FAILED`, `E_AUDIO_STREAM_START_FAILED`, `E_AUDIO_STREAM_PRIORITY_DENIED`, `E_AUDIO_SAMPLE_FORMAT_UNSUPPORTED` |
| `E_MANIFEST_`, `E_PATH_` | Manifest and local path | `E_MANIFEST_INVALID`, `E_MANIFEST_VERSION_UNSUPPORTED`, `E_MANIFEST_TOO_LARGE`, `E_MANIFEST_ROLE_MISSING`, `E_MANIFEST_ROLE_DUPLICATE`, `E_MANIFEST_FILE_MISSING`, `E_MANIFEST_FILE_AMBIGUOUS`, `E_MANIFEST_PATH_UNSAFE`, `E_MANIFEST_SHA256_MISMATCH`, `E_PATH_SYMLINK_REJECTED` |
| `E_WAV_`, `E_BUNDLE_` | WAV and Studio bundle | `E_WAV_NOT_RIFF`, `E_WAV_MALFORMED`, `E_WAV_FORMAT_UNSUPPORTED`, `E_WAV_CHANNELS_UNSUPPORTED`, `E_WAV_SAMPLE_RATE_MISMATCH`, `E_WAV_FRAME_COUNT_MISMATCH`, `E_WAV_BLOCK_ALIGN_INVALID`, `E_WAV_DATA_TRUNCATED`, `E_WAV_PCM_INVALID`, `E_BUNDLE_INCONSISTENT`, `E_BUNDLE_ABORTED` |
| `E_CONTAINER_`, `E_TRACK_`, `E_METADATA_` | Container and role resolution | `E_CONTAINER_NOT_MP4`, `E_CONTAINER_MALFORMED`, `E_NO_AUDIO_TRACKS`, `E_NOT_A_STEM_FILE`, `E_TRACK_COUNT_UNSUPPORTED`, `E_TRACK_ROLE_AMBIGUOUS`, `E_CHANNEL_LAYOUT_UNSUPPORTED`, `E_METADATA_MALFORMED`, `E_METADATA_TOO_LARGE` |
| `E_CODEC_`, `E_DECODE_` | Codec and decode | `E_CODEC_UNSUPPORTED`, `E_CODEC_CONFIG_INVALID`, `E_DEMUX_FAILED`, `E_ENCODED_SAMPLE_INVALID`, `E_DECODE_FAILED`, `E_DECODE_QUEUE_OVERFLOW`, `E_DECODE_ABORTED`, `E_AUDIO_DATA_INVALID`, `E_TRACK_CONFIGURATION_MISMATCH` |
| `E_TIMELINE_`, `E_EDIT_`, `E_PCM_` | Timeline and PCM | `E_TIMESCALE_INVALID`, `E_EDIT_LIST_UNSUPPORTED`, `E_EDIT_LIST_CONFLICT`, `E_PRESENTATION_RANGE_INVALID`, `E_PCM_GAP`, `E_PCM_OVERLAP`, `E_PCM_ALIGNMENT_MISMATCH`, `E_PCM_LENGTH_MISMATCH` |
| `E_BUFFER_`, `E_GENERATION_` | Bounded buffers and stale work | `E_BUFFER_UNDERRUN`, `E_BUFFER_OVERFLOW`, `E_BUFFER_CAPACITY_INVALID`, `E_GENERATION_MISMATCH`, `E_ASSEMBLER_BUFFER_LIMIT` |
| `E_MIXER_` | Mixer and routing | `E_MIXER_GAIN_INVALID`, `E_MIXER_ROLE_INVALID`, `E_MIXER_MASTER_UNAVAILABLE`, `E_MIXER_AUTOMATION_FAILED` |
| `E_CACHE_` | Waveform cache | `E_CACHE_READ_FAILED`, `E_CACHE_WRITE_FAILED`, `E_CACHE_QUOTA`, `E_CACHE_VERSION`, `E_CACHE_CORRUPT`, `E_CACHE_PATH_UNSAFE` |
| `E_HLS_`, `E_NETWORK_`, `E_TLS_` | HLS and network | `E_HLS_STEMSET_INVALID`, `E_HLS_ROLE_MISSING`, `E_HLS_TIMELINE_MISMATCH`, `E_HLS_SEGMENT_BOUNDARY_MISMATCH`, `E_HLS_URL_UNSAFE`, `E_HLS_RESPONSE_TOO_LARGE`, `E_HLS_HTTP_FAILED`, `E_HLS_REDIRECT_REJECTED`, `E_HLS_GROUP_BUFFERING_TIMEOUT`, `E_HLS_HASH_MISMATCH`, `E_HLS_CODEC_UNSUPPORTED`, `E_NETWORK_TIMEOUT`, `E_TLS_FAILED` |
| `E_PLATFORM_`, `E_UPDATE_` | OS integration and updates | `E_PLATFORM_DIALOG_FAILED`, `E_PLATFORM_MEDIA_CONTROL_FAILED`, `E_PLATFORM_POWER_EVENT_FAILED`, `E_UPDATE_DISABLED`, `E_UPDATE_SIGNATURE_INVALID`, `E_UPDATE_ROLLBACK_FAILED` |
| `E_INTERNAL` | Unexpected native failure | `E_INTERNAL` |

## Recoverability

- `retry`: device/network transient and cache I/O errors.
- `choose-another-source`: malformed, unsupported, ambiguous, or unsafe media.
- `reload-session`: decode generation or device-change failures after resources are released.
- `restart-application`: isolated platform initialization failure.
- `fatal-release`: update signature or repeated invariant violation; automatic action stops.

Underruns and priority denial are diagnostics until policy thresholds are exceeded. Overflow, generation mismatch reaching the callback, corrupt update signatures, and unsafe paths fail the affected operation.

## Redaction

Default details may include role, track index, codec identifier, sample rate, frame range, HTTP status, and size limit. They exclude full paths, URLs, hostnames, query strings, selected device identifiers, manifest bodies, media tags, decoder backtraces, and raw library errors. An opt-in local diagnostic export applies the same media redaction.

## Verification

Each public error variant has one construction test, a user-message snapshot, recoverability, and a redaction test. Boundary adapters must prove that known dependency errors map to one native code and that unknown dependency text is not exposed.
