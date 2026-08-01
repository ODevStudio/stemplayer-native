# CI and GitHub workflow design

Workflow YAML, action SHAs, runner labels, and repository feature availability must be verified in the live repository.

## Required CI properties

- All Cargo commands that are intended to cover the repository use `--workspace` or name every intended package explicitly.
- Dependency policy uses a reviewed `deny.toml` and `cargo deny check` rather than an inventory command.
- Every third-party action is pinned to a full commit SHA with its release tag retained in a comment.
- The pinned Rust toolchain is read from `rust-toolchain.toml`, not repeated in workflow YAML.
- Pull request runs use concurrency cancellation, explicit least-privilege permissions, and job timeouts.
- Windows CI includes an unsigned release-profile build of `stemplayer`.
- Documentation and workflow integrity checks run before the Rust gates.

## Pull request workflows

- Run documentation and workflow checks, formatting, workspace clippy, workspace tests, and explicit feature checks.
- Use Ubuntu and Windows where platform compilation or behavior differs.
- Set matrix `fail-fast: false`, safe Cargo caching, job timeouts, and `contents: read`.
- Push CI should normally target `master` and deliberate release tags, avoiding duplicate feature-branch push and pull-request runs.

## Dependency policy

- Review and commit the repository-specific `deny.toml` before making cargo-deny required.
- Run `cargo deny check`.
- Keep a second advisory tool only when it provides distinct coverage.
- Cache or install pinned tools deterministically.

The accepted license list matches the locked workspace graph, including Slint's declared GPL and royalty-free alternatives. Registry dependencies are limited to crates.io and Git dependencies are denied. Duplicate versions warn because the cross-platform Slint graph currently requires parallel major and minor lines. cargo-deny 0.20 also reports `workspace = true` path dependencies as wildcard dependencies, so that warning remains visible without rejecting the workspace's exact local dependencies. The two current quick-xml advisories are narrowly ignored because the affected crate is only a Wayland protocol build dependency, the product does not parse XML, and the current wayland-scanner requirement blocks the fixed release; remove both exceptions when that dependency can move to quick-xml 0.41 or newer. Three unmaintained transitive font or macro crates are also explicit exceptions because their advisories report no safe upgrade in the locked Slint graph; revisit them with every Slint update.

## Optional GitHub security automation

Enable dependency review, CodeQL, and required security rules only when they are available for this private repository and account. Do not add an unavailable service as a required gate.

## Manual qualification workflow

Use `workflow_dispatch` or an approved self-hosted Windows environment for hardware audio, NVDA/UI automation, long soaks, waveform/cache qualification, HLS/TLS qualification, installer, signing, and clean-system work.

Ordinary pull requests must not depend on audio hardware, interactive desktop state, signing credentials, private fixtures, or clean-machine infrastructure.

## Release boundary

`STATUS.md` currently blocks packaging and signing. A workflow may compile an unsigned release binary for regression detection, but it must not publish, sign, create an installer, enable automatic updates, or represent the artifact as qualified.
