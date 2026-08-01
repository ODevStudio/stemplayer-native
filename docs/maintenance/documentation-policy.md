# Documentation policy

The documentation tree is a set of interfaces, not a memory dump. Git history is the default archive.

## Authority map

| Fact | Canonical location |
| --- | --- |
| Current phase, blockers, latest qualification baseline | `STATUS.md` |
| Approved future scope | `ROADMAP.md` and current phase plan |
| Completed user-visible behavior | `CHANGELOG.md` |
| Vulnerability reporting and top-level security posture | `SECURITY.md` |
| Runtime and crate invariants | `docs/architecture/` |
| Accepted rationale | `docs/decisions/` |
| Error contracts | `docs/errors/` |
| Input and network format contracts | `docs/formats/` and `docs/compatibility.md` |
| Measurement method | `docs/measurement/` |
| Concise redacted evidence | `docs/qualification/` |
| Agent routing and task procedure | root and scoped `AGENTS.md`, `docs/agent/` |

Do not restate an authority in multiple files. Link to it.

## Progressive disclosure tiers

1. Root `AGENTS.md`: non-obvious invariants and task routing only.
2. Indexes: short navigation and authority maps.
3. Task references: verification, CI, security, testing, and maintenance procedure.
4. Deep references: architecture, formats, decisions, measurement, and qualification evidence.

A routine code task should not require loading Tier 4 unless it changes that contract.

## Keep, consolidate, delete

Keep a document when it is current authority, a durable contract or decision, required qualification evidence, legal or release material, or a task guide that prevents a real recurring error.

Consolidate when multiple files state the same rule, command, status, or ownership boundary.

Delete when content is superseded, duplicated, disconnected from current code, unrecoverably corrupted, or useful only as implementation history. Do not move dead content into a permanent archive by default.

## New document requirements

A new document must have a distinct audience and purpose, one canonical ownership location, links from the smallest relevant index, no private data or raw generated output, commands verified against the current repository, and a deletion or update path when its purpose expires.

Use concise headings and relative links. Prefer tables for routing and ownership; prefer executable tests, schemas, and code references for precise behavior.

## Review checks

Active Markdown must be checked for broken relative links, deleted archive references, mojibake, duplicate current-status claims, private paths or data, and root `AGENTS.md` size. Root `AGENTS.md` must remain below 4096 bytes and 80 lines. Documentation-only changes do not need to pretend to validate Rust behavior, but must run documentation checks and `git diff --check`.
