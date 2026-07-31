# Native Player Agent Rules

- Keep this repository independent from the web player and converter.
- Preserve the locked Rust toolchain and dependency policy.
- Do not commit private media, generated qualification output, machine paths, or credentials.
- Keep security validation at input and filesystem boundaries.
- Run formatting, clippy, locked workspace tests, and the relevant release check before merging.
- Record current phase status only in `STATUS.md`.
