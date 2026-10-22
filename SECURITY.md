# Security

Report suspected vulnerabilities privately to the repository owner before public disclosure. Include the affected commit, a minimal reproduction, impact, and any proposed mitigation. Do not include private media, credentials, or personal paths in the report.

The player treats local sources, manifests, container metadata, URLs, HTTP responses, cache records, and installer inputs as untrusted. Validation is bounded and fails closed. The player does not use browser cookies, generic file associations, startup network access, or unsigned automatic updates.

Security-sensitive changes must include a regression check and must preserve the documented limits in `docs/formats/`, `docs/architecture/`, and the HLS client profile.

The [Windows hobby preview](docs/release/windows-preview.md) is explicitly unsigned and manually distributed. Its SHA-256 checksum detects changed downloads but does not authenticate a publisher. Never disable Defender, SmartScreen, or other protections to run it. Automatic updates remain disabled. Published release assets are immutable; withdraw an unsafe preview with an advisory and issue a new version rather than replacing its binaries.
