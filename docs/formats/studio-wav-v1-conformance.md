# Studio WAV v1 Conformance

Status: Phase 4 implementation report, 2026-07-22.

## Source boundary

The platform crate resolves a selected folder, its sibling `manifest.json`, or an explicit dropped-file group into a constrained `SourceGroup`. Local folder and dropped-file selection accepts at most 4,096 entries. The format crate accepts only that provider. The platform adapter indexes regular selected files, rejects symlinks and mixed-folder groups, and the format layer rejects normalized-name collisions before opening manifest-referenced basenames. Neither layer searches parent folders or resolves URLs.

Basenames must be NFC, contain no separator, colon, control character, trailing dot or space, dot traversal, absolute prefix, UNC/device path, or Windows reserved device name. Matching uses NFC plus Unicode lowercase consistently on every platform.

## Manifest and integrity

- Maximum manifest size: 1,048,576 bytes.
- Maximum JSON depth: 32.
- Maximum JSON values: 10,000.
- Accepted schemas: 1 and 2.
- Required unique routes: drums, bass, other, vocals.
- Optional unique route: master.
- Ignored non-routed outputs: instrumental and acapella.
- SHA-256 uses `sha2 0.10.9`, 64 KiB reads, cancellation checks, and a caller-provided bounded `SyncSender`.
- Progress identifies the manifest file index; a full channel drops progress rather than blocking source work.

## WAV and decode profile

The RIFF reader uses checked offsets, at most 4,096 chunks, one bounded `fmt ` chunk, one `data` chunk, and validated odd-byte padding. Qualified inputs are stereo PCM16, PCM24, PCM32, and finite IEEE Float32 at 44.1 or 48 kHz.

Decode output is planar Float32. A request is limited to 4,096 frames and one 64 KiB range read. Integer divisors are exact profile constants. Float32 values outside `[-1, 1]` are preserved; NaN and Infinity fail with `E_WAV_PCM_INVALID`.

## Consistency and mutation

Every routed WAV is inspected before session eligibility. Sample rate and frame count must match exactly. There is no trimming, padding, resampling, or repair.

Each validated track retains its original size and modification identity. Reopening or reading a changed track fails with `E_BUNDLE_INCONSISTENT`.

## Resource evidence

Hashing is synchronous and uses at most one open file. Playback uses one bounded decode producer and retains one validated reader per routed track, for a maximum of five open WAV files. Inspection reads only the RIFF header, chunk headers, and `fmt ` bytes unless hash verification is requested. Hashing reads 64 KiB at a time. Each track decoder reuses two 4,096-sample Float32 channel vectors and one bounded encoded range buffer.

The sparse-duration test compares a 4,096-frame bundle with a one-hour, 48 kHz bundle. Per-track inspection bytes are identical. A 4,096-frame PCM16 decode reads 16,384 bytes in both cases and returns 4,096 samples per channel.

## Qualification

| Check | Result |
| --- | --- |
| Qualified encodings and rates | PASS |
| Master present and absent | PASS |
| Unknown odd RIFF chunk and padding | PASS |
| Every structural truncation boundary | PASS |
| Declared RIFF overflow | PASS |
| Unsafe path and normalized collision | PASS |
| Hash success, malformed value, mismatch, bounded progress, cancellation | PASS |
| Cross-track mismatch | PASS |
| Exact PCM integer boundaries | PASS |
| Float32 out-of-range preservation and non-finite rejection | PASS |
| Post-validation mutation | PASS |
| Duration-independent inspection and decode reads | PASS |
| Windows locked workspace tests | PASS |
| Ubuntu 24.04 WSL2 headless tests and metadata | PASS |
| Dependency vulnerability and license audit | PASS |

Reproducible commands:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/qualify-wsl.ps1 -OutputDirectory artifacts/phase4/wsl-ubuntu
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/audit-dependencies.ps1 -OutputDirectory artifacts/phase4/dependencies
```

Artifacts:

- `artifacts/phase4/wsl-ubuntu/headless.json`
- `artifacts/phase4/dependencies/audit.json`
