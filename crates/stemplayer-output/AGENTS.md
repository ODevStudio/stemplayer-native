# Real-time output constraints

Applies under `crates/stemplayer-output/`.

The device callback is a strict real-time boundary. It may copy prepared stereo samples, perform unavoidable sample-format conversion, advance counters, and emit silence on underrun.

It must not:

- allocate or free memory;
- decode, resample large windows, or parse data;
- perform file, device-discovery, or network I/O;
- log, format strings, update UI, or publish unbounded diagnostics;
- take a normal blocking mutex or wait on another thread.

Keep device lifecycle and expensive work outside the callback. Preserve bounded rings, deterministic underrun behavior, and post-mix-only resampling.

Run output package tests and the callback audit for callback-adjacent changes. Real hardware and long-soak claims require the approved Windows qualification environment.
