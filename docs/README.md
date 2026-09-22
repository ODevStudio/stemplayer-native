# Documentation

Start with the smallest document that answers the task. This index is for contributors and users; coding agents should begin with [`AGENTS.md`](../AGENTS.md) and follow its task routing.

## Current product state

- [Status](../STATUS.md): current phase and blockers.
- [Roadmap](../ROADMAP.md): approved future scope.
- [Phase 11 plan](plans/PLAN_11.md): packaging, signing, installation, and release exit criteria.
- [Changelog](../CHANGELOG.md): completed user-visible changes.

## User and contributor policies

- [Security](../SECURITY.md)
- [Compatibility](compatibility.md)
- [Contributing](../CONTRIBUTING.md)
- [Documentation policy](maintenance/documentation-policy.md)
- [Test strategy](maintenance/test-strategy.md)
- [Hostile-input fuzzing](maintenance/fuzzing.md)
- [Build evidence and local support](release/README.md)

## Durable technical references

- `architecture/`: crate responsibilities and runtime invariants.
- `decisions/`: accepted rationale that still explains the current design.
- `errors/`: error taxonomy and reporting boundaries.
- `formats/`: native format profiles and conformance rules.
- `measurement/`: measurement methods and schemas.
- `qualification/`: concise, redacted evidence summaries; no raw reports or private fixtures.
- `reference/`: compatibility notes and source behavior maps.

## Agent task references

- [Agent context index](agent/README.md)
- [Repository map](agent/repository-map.md)
- [Verification guide](agent/verification.md)
- [CI and workflow design](agent/ci-workflows.md)
- [Security boundaries](agent/security-boundaries.md)

Historical implementation detail belongs in Git history unless a current decision, contract, qualification result, legal obligation, or release requirement still depends on it.
