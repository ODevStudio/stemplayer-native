# Format parsing constraints

Applies under `crates/stemplayer-format/`.

- Treat every byte, path component, manifest field, declared size, role, hash, and filename as untrusted.
- Inspect content rather than trusting extensions.
- Keep reads, open files, allocations, metadata, tracks, frames, names, and diagnostics bounded.
- Normalize names consistently and reject ambiguity, traversal, duplicate roles/files, mutation, and contract mismatch.
- Fail closed with stable domain errors; do not leak full private paths in public diagnostics.
- Changes to accepted input require the relevant `docs/formats/` update and malformed or boundary regression tests.

Run the package tests first, then the shared verification tier for public contract changes.
