# Agent context index

This directory is a progressive-disclosure layer for any coding agent. It does not replace source code, tests, format specifications, or human review.

## Loading order

1. Read root `AGENTS.md`.
2. Read the nearest scoped `AGENTS.md` for files being changed.
3. Load only the task reference below.
4. Open detailed architecture, format, decision, or qualification files only when the change touches that contract.

| Need | Reference |
| --- | --- |
| Locate ownership and dependencies | [Repository map](repository-map.md) |
| Select commands and evidence | [Verification](verification.md) |
| Edit CI, actions, Cargo policy, or GitHub security automation | [CI workflows](ci-workflows.md) |
| Change parsing, URLs, caches, real-time output, or release inputs | [Security boundaries](security-boundaries.md) |
| Edit docs or decide what to delete | [Documentation policy](../maintenance/documentation-policy.md) |
| Add tests or qualification coverage | [Test strategy](../maintenance/test-strategy.md) |

## Context budget rules

- Do not read every file under `docs/` for routine code changes.
- Prefer current code and executable tests over narrative implementation history.
- Follow links instead of duplicating rules into prompts or new Markdown.
- Load `docs/qualification/` only when validating a qualification claim or release gate.
- Load the Phase 11 plan only for packaging, signing, installer, OS integration, provenance, or release work.
