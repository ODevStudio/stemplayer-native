# Phase 6 Post-Mix Resampler Decision

Date: 2026-07-24
Status: accepted for qualification

## Decision

Use rubato 4.0.0 with its synchronous FFT resampler for fixed source-to-device rate conversion. Keep exactly one stereo instance after the master/stem A/B bus and before the device ring.

## Rationale

The standard library has no band-limited sample-rate converter. Rubato provides fixed-ratio, anti-aliased stereo conversion, reports its output delay, and supports preallocated input and output buffers through `process_into_buffer`. A local linear converter would be smaller but would lose high-frequency quality and omit anti-alias filtering. A custom sinc or FFT implementation would duplicate mature DSP code and expand the correctness burden.

The resampler is created and reset on the render worker. The device callback remains unchanged and only drains prepared stereo frames. Input, output, and conversion buffers have fixed capacities derived from the negotiated rates and bounded render chunk. Fractional phase, filter delay, frame totals, resets, and device-change discontinuities are exposed in snapshots.

The stem sum remains Float32. The graph adds only four stereo values, so Float64 accumulation would add conversion work without a measurable benefit for the Float32 source, resampler, ring, and device path. Peak diagnostics observe the unnormalized Float32 result; they do not clamp, limit, or alter it.

## Security and maintenance

The dependency is pinned exactly. Its default feature set enables the FFT implementation and leaves logging disabled. It performs no filesystem, network, device, or configuration access. Source selection and decoded Float32 validation remain outside the dependency. The direct crate declares MIT OR Apache-2.0 and requires Rust 1.85 or newer, below the workspace Rust 1.92 floor.

Version 4.0.0 is a recent major release from the established rubato project. The adapter migration is isolated inside `stemplayer-output`, so replacement does not affect domain, mixer, engine, or UI contracts. Qualification includes locked dependency audit, exact frame-count tests, channel-isolation tests, reset determinism, resource measurement, and listening checks.

## Dependency closure

The Windows output closure adds audioadapter, audioadapter-buffers, audioadapter-sample, audio-core, num-complex, num-integer, num-traits, realfft, rustfft, primal-check, strength_reduce, transpose, visibility, and windowfunctions. The locked audit and binary footprint are recorded with the Phase 6 evidence.
