# Phase 3 Callback Allocation Audit

The callback allocation audit is a dedicated development-only binary. It is not linked into the player or probe.

Its only unsafe code implements `GlobalAlloc` by forwarding every operation unchanged to `std::alloc::System`. While tracking is enabled, allocation and reallocation calls increment an atomic counter. Deallocation is forwarded without counting. The pointer, layout, and size values are passed to the same system allocator operation without inspection, storage, or modification.

The audit preallocates the SPSC ring and output buffer, enables tracking, executes 100,000 callback-kernel iterations, disables tracking, and then serializes its report. A nonzero allocation count exits unsuccessfully.

This exception exists only because stable Rust does not expose allocation counts without a global allocator wrapper. It can be deleted when a stable standard-library allocation observer or an external platform trace provides equivalent callback-scoped evidence.
