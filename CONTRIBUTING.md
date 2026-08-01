# Contributing

Keep changes inside the native repository and preserve its standalone runtime boundary. Do not commit private media, generated qualification output, screenshots, machine-specific paths, signing material, tokens, or credentials.

Before editing, read the nearest applicable `AGENTS.md`. For ownership and architecture, start with the [repository map](docs/agent/repository-map.md). Security-sensitive changes must preserve the documented bounds in `SECURITY.md`, `docs/formats/`, and the relevant architecture or compatibility reference.

Use the narrowest test while iterating, then run the verification tier defined in the [verification guide](docs/agent/verification.md). Test placement, fixtures, and manual qualification boundaries are defined in the [test strategy](docs/maintenance/test-strategy.md).

Documentation follows the [documentation policy](docs/maintenance/documentation-policy.md): one authority per fact, short indexes, progressive disclosure, and deletion of dead material rather than an ever-growing archive.

Record user-visible completed behavior in `CHANGELOG.md`, current blockers only in `STATUS.md`, durable decisions in `docs/decisions/`, and concise redacted evidence in `docs/qualification/`.
