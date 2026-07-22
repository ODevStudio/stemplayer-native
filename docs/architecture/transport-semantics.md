# Transport Semantics v1

## States

`EMPTY`, `INSPECTING`, `READY`, `PRIMING`, `PLAYING`, `PAUSED`, `SEEKING`, `ENDED`, `ERROR`, and `CLOSED`.

| From | Legal next states |
| --- | --- |
| EMPTY | INSPECTING |
| INSPECTING | READY, ERROR |
| READY | PRIMING, SEEKING |
| PRIMING | PLAYING, PAUSED, SEEKING, ERROR |
| PLAYING | PAUSED, SEEKING, ENDED, ERROR |
| PAUSED | PRIMING, SEEKING, READY, ERROR |
| SEEKING | PRIMING, PLAYING, PAUSED, ERROR |
| ENDED | PRIMING, SEEKING, READY |
| ERROR | EMPTY, CLOSED |
| CLOSED | none |

An illegal transition returns `E_TRANSPORT_STATE_INVALID` without changing state. An unrecoverable session failure enters ERROR. Close is idempotent and terminal.

## Commands

- Load cancels the active generation, releases the previous session, enters INSPECTING, and either publishes one READY snapshot or ERROR.
- Play from READY/ENDED enters PRIMING and does not enter PLAYING until the start watermark and output stream are ready. Play from PAUSED resumes the retained cursor through PRIMING.
- Pause stops frame consumption, retains source buffers within their caps, and enters PAUSED.
- Stop cancels current decode, resets the cursor to session start, retains the loaded source, and ends READY.
- Seek clamps to `[start_frame, end_frame_exclusive - 1]`, creates a new generation, and returns to PLAYING only if playback was requested; otherwise PAUSED.
- Reset is legal only from ERROR and returns EMPTY after resources are released.

Duplicate play while PLAYING/PRIMING returns `E_PLAYBACK_ALREADY_ACTIVE`. Commands needing a loaded session return `E_PLAYBACK_NOT_READY`. Commands after close return `E_PLAYBACK_SESSION_CLOSED`.

## Snapshot And Time

The authoritative position is an absolute source output-frame integer. Microseconds are a derived rational display value. Snapshots are immutable, coalesced, and rate-limited; no audio-buffer event crosses into the UI. Duration equals `end_frame_exclusive - start_frame` converted rationally.

ENDED occurs exactly when the callback has consumed through the common exclusive end frame. Pause cannot advance the authoritative cursor. Stop does not unload.

## Generation And Cancellation

Every load, seek, and stop owns a monotonically increasing 64-bit generation. Decoder, waveform, and network results include it. Mismatched results are dropped and counted; they cannot publish PCM, state, or diagnostics. Cancellation is bounded and never waits inside the audio callback.

## Buffering

Initial defaults are 1.5 seconds start, 3 seconds low, 6 seconds target, and 12 seconds capacity at source rate. The values are tunable after measurement. One ring contains all synchronized 8/10 channels and never overwrites unread frames. Full returns backpressure; empty writes silence and increments underrun. Capacities and overflow behavior are explicit.

## Security And Errors

Source inspection completes before output creation. User-visible errors have stable codes and redacted messages. Paths, URLs, decoder internals, and OS device identifiers appear only in opt-in local diagnostics. Panic, allocation, blocking locks, I/O, parsing, and logging are forbidden in the output callback.
