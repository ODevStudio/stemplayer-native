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

### Linux D-Bus Maintenance Exceptions

On 2026-10-21 the maintainer approved `RUSTSEC-2026-0370` (`zvariant 5.13.1`), `RUSTSEC-2026-0371` (`zbus_names 4.3.4`), and `RUSTSEC-2026-0372` (`zvariant_derive 5.13.1`). These are maintenance-only notices: the retired crates were incorporated into zbus 6. The locked Linux graph still needs zbus 5 through Slint's winit/accesskit backend and rfd's ashpd portal. No compatible complete upgrade is available through those parents. The additional [Linux preview packaging](../release/unix-preview.md) retains these exact exceptions and the enforcing workspace gate; no vulnerability exception is added.

These three exact IDs are ignored, not their packages or future vulnerabilities. The workspace `cargo deny check` gate remains required. The product integration test `windows_product_excludes_retired_linux_dbus_packages` rejects any of these packages reachable from the default Windows application graph. Review the exceptions at every Slint/rfd update; remove them once all parents migrate off the retired crates. Linux distribution requires a new dependency and qualification review. See the [security refresh evidence](../qualification/windows-preview-security-refresh.md).

## Optional GitHub security automation

Enable dependency review, CodeQL, and required security rules only when they are available for this private repository and account. Do not add an unavailable service as a required gate.

## Manual qualification workflow

Use `workflow_dispatch` or an approved self-hosted Windows environment for hardware audio, NVDA/UI automation, long soaks, waveform/cache qualification, HLS/TLS qualification, installer, signing, and clean-system work.

Ordinary pull requests must not depend on audio hardware, interactive desktop state, signing credentials, private fixtures, or clean-machine infrastructure.

## Release boundary

The [Windows hobby preview](../release/windows-preview.md) permits a manual-dispatch-only packaging job after all ordinary CI gates pass. Use a clean build directory, no restored build cache, read-only repository permissions, and pinned build tools/actions. Upload candidate artifacts only; do not publish, sign, create an installer, enable automatic updates, or represent the artifact as qualified. See `STATUS.md` for remaining publication and signed-release blockers.

The same workflow's `linux-and-macos` dispatch selection builds additional archives from the immutable published application commit with reviewed packaging code. It skips Windows packaging, uses system-only runtime dependency checks, and uploads uniquely named archive/checksum pairs. macOS uses local ad-hoc signing without publisher credentials or notarization. Publication remains a separate maintainer operation; see the [Unix packaging procedure](../release/unix-preview.md).
