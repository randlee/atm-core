# ADR-050 — Shared Publish-Kit Ownership

| Field | Value |
| --- | --- |
| Status | Accepted |
| Scope | Release workflows, release helpers, publish agents, and consumer release data |
| Supersedes | REQ-P-RELEASE-004 local-control-surface wording |
| Relates to | REQ-P-RELEASE-004; Phase AT; `sc-publish` |

## Context

ATM is one of a growing set of repositories that publish through the same
release infrastructure. Per-repository copies that are locally edited drift in
workflow behavior, tool versions, validation, and agent instructions. That
drift is already a release risk and grows with every additional consumer.

The earlier REQ-P-RELEASE-004 wording required ATM to own the whole release
control surface. That was appropriate for a one-repository port, but conflicts
with the shared publish-kit model and would require every consumer to maintain
the same logic independently.

## Decision

`sc-publish` exclusively owns shared release implementation:

- workflows, actions, helpers, release-manifest parser, and shared tests;
- publisher and channel-agent prompts; and
- common bootstrap, tool-resolution, receipt, and retry behavior.

ATM consumes those files only through the canonical `sc-publish` package
installer from a recorded upstream commit. ATM never locally edits a synced
shared file. A shared defect is fixed upstream first and then adopted through
an unchanged installer run.

ATM owns only consumer data and non-shared extension data:

- its complete installer input / release manifest declarations;
- enabled destinations, artifacts, explicit crate dependency/publish order, and
  release-specific validation data; and
- explicitly namespaced ATM-only validations that the shared manifest contract
  executes without knowing ATM-specific behavior.

The consumer must prove the installation by validating package-byte parity,
rendered-manifest semantics, and a clean second dry-run. A shared consumer-CI
verification command must fail if a synced file drifts or if a workflow adds a
tool install outside the common bootstrap.

## Consequences

- ATM no longer owns local copies of generic release behavior; it owns correct
  release data and evidence for its declared artifacts.
- A shared-kit improvement has one upstream implementation and one test suite,
  then becomes available unchanged to every consumer.
- Release preflight and publication converge on the same resolved plan and
  toolchain instead of accumulating repository-local exceptions.
- A consumer cannot silently hotfix a shared workflow under release pressure;
  it must use an upstream fix or defer the operation.

## Rejected Alternative

Retain local ownership of copied workflows, scripts, and publisher prompts.
This would make every consuming repository a competing source of truth and
guarantee drift as the number of repositories grows.

## History

This ADR originally landed via the reverted phase-AS merge (PR #939/#958,
reverted by PR #960, which removed the file from the repo). It is deliberately
re-authored with its reviewed text unchanged and re-accepted as part of Phase
AT planning (PR #963), restoring the traceability anchor the Phase AT plan
cites.
