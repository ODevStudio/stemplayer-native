# Stemplayer Native Agent Rules

Read this file first. Load only the linked reference needed for the task; do not preload the whole documentation tree.

## Repository invariants

- This is a standalone Rust native player. Do not add a build/runtime dependency on the web player, converter, Node.js, Python, FFmpeg, a browser, or a WebView.
- Preserve `Cargo.lock`, `rust-toolchain.toml`, workspace boundaries, and declared format profiles unless the task explicitly changes their policy.
- Treat files, manifests, container metadata, URLs, HTTP responses, caches, shell inputs, and installer inputs as untrusted. Keep validation bounded and fail closed.
- Do not commit private media, raw qualification output, screenshots, machine-specific paths, tokens, signing material, or credentials.
- `STATUS.md` is the only authority for current phase status and blockers.

## Load context by task

| Task | Read next |
| --- | --- |
| Find code or understand ownership | `docs/agent/repository-map.md` |
| Choose or run checks | `docs/agent/verification.md` |
| Edit GitHub Actions or dependency policy | `docs/agent/ci-workflows.md` |
| Change input, network, cache, or release security | `docs/agent/security-boundaries.md` and `SECURITY.md` |
| Edit documentation | `docs/maintenance/documentation-policy.md` |
| Add or reorganize tests | `docs/maintenance/test-strategy.md` |
| Change a format contract | the relevant file under `docs/formats/` and `docs/compatibility.md` |
| Change architecture or accepted rationale | the relevant file under `docs/architecture/` or `docs/decisions/` |

More-specific `AGENTS.md` files apply under selected high-risk directories.

## Working method

1. Inspect the nearest applicable `AGENTS.md`, relevant code, and existing tests before editing.
2. Make the smallest coherent change and reuse existing interfaces and fixtures.
3. Run narrow checks while iterating, then the applicable verification tier.
4. Report exact commands, results, and anything not run. Never imply a hardware, signing, installer, or clean-system gate passed when it was not executed.
