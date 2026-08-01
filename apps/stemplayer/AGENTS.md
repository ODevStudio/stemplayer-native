# Application and UI constraints

Applies under `apps/stemplayer/`.

- Keep product behavior in Rust controller/domain layers where practical; Slint files define presentation and event wiring.
- Keep the UI event-driven. Do not add a permanent idle repaint or polling loop.
- Do not block the UI thread on decode, analysis, network, filesystem, or device work.
- Preserve bounded playlist size, selection/focus/active-item remapping, settings persistence, generation cancellation, and sanitized user-visible diagnostics.
- A source must not autoplay merely because settings or a restored playlist were loaded unless the approved policy explicitly says so.
- Test controller and playlist logic with deterministic Rust tests. Use Windows UI/NVDA qualification only for behavior that cannot be proven below the native accessibility/window layer.

Read `docs/agent/verification.md`; load UI and architecture decisions only for the affected behavior.
