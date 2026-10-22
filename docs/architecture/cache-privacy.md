# Cache Privacy v1

## Scope

The cache is optional, local, disposable, and waveform-only. It stores no PCM, encoded audio, manifest body, playlist, segment, full path, filename, URL, title, tags, or commercial-audio metadata. Playback never depends on a cache hit.

## Key

Container keys combine exact byte size, last-modified time when available, SHA-256 of the first and last 64 KiB, ordered role/codec/rate/channel/edit descriptors, timeline version, analysis version, and peak format version.

Studio keys combine canonical manifest hash plus sorted per-role byte size, last-modified time, head/tail hashes, sample rate, frame count, bit depth, format code, and the same version fields. Raw manifest content and names are not retained.

HLS cache remains disabled until Phase 10 defines a signed/hash-addressed source identity. URLs are not acceptable persistent keys.

## Payload

Schema version 1 stores, per controllable role, four min/max Float32 peak levels at 256, 1,024, 4,096, and 16,384 source frames per peak. It also stores start frame, frame count, timestamps, and byte count. Optional measured master-minus-stem consistency contains aggregate numbers only.

The default total cap is 512 MiB. A per-record cap is 64 MiB. Cache reads and writes hold the same cross-process operation lock. Unix writes use a temporary file plus same-directory rename. Windows writes exclusively create the final record under that lock, flush it, and read it back for validation before reporting success. Directory protections remain held throughout; they are not relaxed to permit a rename. A crash can leave a partial Windows record, which must be rejected and rebuilt rather than published. Reads validate magic, schema, lengths, checksums, role order, sample rate, timeline, and all finite peak values before allocation/publication.

## Location And Permissions

Use the OS per-user cache directory, never the media directory. Cache directories and files use user-only permissions where supported. Symlinks/reparse points are rejected during creation and replacement. File names are a lowercase SHA-256 key plus fixed extension.

## Eviction And Failure

Least-recently-used metadata drives eviction before a write. Metadata enumeration is bounded and never loads payloads. Corrupt, stale, oversized, or unknown-version entries are deleted and treated as misses. Quota and I/O failures are recoverable cache errors and cannot fail playback or completed in-memory peaks.

Disable prevents future reads/writes but does not stop analysis. Clear-current and clear-all affect only this versioned application cache root and never follow links. Concurrent processes must acquire the operation lock or report a recoverable busy error. The Windows writer reuses existing records only after validation against the expected fingerprint, sample rate, and timeline. A failed write cleans up only a record created by that operation, without truncating a pre-existing record.

On Unix the operation guard explicitly unlocks before closing its descriptor. A duplicate inherited during process creation must not retain a completed operation's lock until that duplicate closes. Directory identity checks and nonblocking lock acquisition remain required.

## Diagnostics And Proof

Diagnostics expose enabled state, hit/miss/error, current and total bytes, and redacted error code. Privacy tests scan serialized records for fixture basenames, absolute path fragments, manifest text, URL hosts, and known encoded/PCM byte sequences. Corruption, truncation, oversized lengths, duplicate names, eviction, and interrupted-write tests must fail closed.
