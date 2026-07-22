# Measurement Methodology v1

## Common Rules

Use a locked release build on AC power after one warm-up run. Record the exact commit, dirty state, Rust version, dependency lock hash, binary hash/size, OS/build, architecture, CPU, logical cores, RAM, GPU/renderer, display scale, audio host/device/config, fixture alias/hash, source rate, and UTC timestamp.

Do not compare debug builds. Disable unrelated foreground work without disabling security software. Report median and p95 from at least 20 iterations for open, first audio, seek, and mixer latency. Preserve every sample, not only aggregates.

Memory fields distinguish private bytes, resident/working set, proportional set size when available, shared mappings, filesystem cache, GPU process memory, and process-set total. CPU is both process CPU time divided by wall time and normalized by logical cores. A value that a platform cannot expose is `not_tested`, never zero.

Raw reports conform to `report.schema.json` and live below `artifacts/phase0/<platform>/`. Private fixture identity is an alias and approved SHA-256 only.

## Windows

Run from the native workspace:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/measure-windows.ps1
```

The script builds `--release --locked`, runs the dependency/audio/codec self-check, opens the UI proof, and samples `PrivateMemorySize64`, `WorkingSet64`, process CPU time, and handles. Use Task Manager or Process Explorer only as a cross-check. GPU process memory and wakeup/timer counts require an ETW/WPA capture and remain `not_tested` until the capture is attached.

For latency runs, the probe or later headless harness writes monotonic timestamps for command accepted, READY, first non-silent output frame, seek requested/completed, mixer command, first affected output frame, ring watermark, underrun, overflow, and exact end frame. Windows UI Automation plus NVDA/Accessibility Insights records accessibility separately from timing.

## macOS

Optional Phase 12 platform-expansion method.


Runtime commands on the target machine:

```text
/usr/bin/time -l target/release/stemplayer-phase0-probe --ui-smoke 20
ps -o pid,rss,vsz,time,command -p <pid>
```

Use Activity Monitor or `vmmap -summary <pid>` for private/shared regions, Instruments Allocations/Time Profiler for allocation and CPU evidence, and `powermetrics` only with explicit operator approval for wakeups/energy. Use Accessibility Inspector on an application bundle. Record CoreAudio device/rate/format and VoiceOver results.

## Linux

Optional Phase 12 platform-expansion method. WSL compilation and headless tests are development evidence only.


Runtime commands on Ubuntu and Fedora targets:

```text
/usr/bin/time -v target/release/stemplayer-phase0-probe --ui-smoke 20
cat /proc/<pid>/status
cat /proc/<pid>/smaps_rollup
pidstat -u -r -w -p <pid> 1
```

Record compositor/session, Wayland/X11, renderer, PipeWire/PulseAudio/ALSA selection, and priority promotion result. Use `perf stat` for CPU/context-switch evidence when available and Orca plus the platform accessibility inspector for screen-reader exposure. Do not treat a headless cross-compile as runtime qualification.

## Metrics

- Idle: wait 10 seconds after the window is visible, then sample for at least 30 seconds; repeat minimized for 30 seconds.
- Binary: stripped executable bytes and SHA-256; report required dynamic libraries separately.
- READY: source-open request to immutable READY snapshot.
- First audio: Play acceptance to first non-silent frame submitted to the device timeline.
- Seek: request acceptance to first target-generation frame submitted; median and p95 over beginning/middle/near-end/random targets.
- Mixer: command acceptance to first rendered frame with new automation.
- Ring: configured capacity, observed high-water, underrun/overflow counters.
- Soak: warm-up complete to eight-hour end, linear memory slope, peak private/resident, final-minus-initial, exact end, underruns, overflows.
- End: expected exclusive frame minus rendered exclusive frame; budget is zero.

## Acceptance

A budget claim cites the command/script version and raw report. Cross-platform medians are not pooled. Any security software exclusion, elevated priority, external device, VM, software renderer, or unavailable counter is explicit. Failed or interrupted runs remain in raw evidence with status `failed` or `incomplete`.
