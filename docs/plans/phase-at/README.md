# Phase AT — Manifest-Driven Publishing Recovery

```yaml
plan_type: phase_index
phase: AT
status: proposed
branch: plan/phase-at-publish-recovery
worktree: plan/phase-at-publish-recovery
upstream_package: sc-publish
upstream_revision: pin-pending (first revision resolving sc-publish #39-#41; ce85b4d is the floor)
```

## Goal

Replace no product code and add no ATM-specific publishing framework. Install
the shared `sc-publish` package from one immutable upstream revision using
ATM's complete JSON input (AT.1), prove a retryable 1.4.3 Python release from
immutable `main` reaches TestPyPI and PyPI (AT.2), then delete the deprecated
legacy publish surface the shared kit demonstrably covers (AT.3).

## Baseline

`develop` @ `d610b4c07` (merge of PR #960), which reverted the unauthorized
phase-AS redraft (PR #958) and its plan merge (PR #939). The baseline carries
**no** shared-kit files: publishing runs on the legacy `release.yml`,
`release-preflight.yml`, `hermes-atm-pypi-publish.yml`, root `scripts/`
release tooling, and the pre-kit `.claude/agents/publisher.md`. The kit is
adopted deliberately through the installer in AT.1 — never inherited from the
reverted merges.

## Ownership And Boundaries

Per **ADR-050 (Shared Publish-Kit Ownership)** and **REQ-P-RELEASE-004
(superseded in part by ADR-050)**:

- `sc-publish` owns shared workflows, composite actions, scripts, agent
  prompts, tool bootstrap, and their tests. ATM copies those package files
  byte-for-byte through `plugins/sc-publish/install.py`; ATM never patches a
  copied file. A shared-file correction is an `sc-publish` issue/PR, adopted
  here through an unchanged installer run.
- ATM owns only the complete consumer JSON input and the manifests rendered
  from it. The input explicitly names every crate, Python distribution,
  binary, channel, publish order, and version source. Nothing is inferred.
- GitHub environments own publishing tokens. A missing, expired, or rejected
  credential is a fail-closed, channel-scoped workflow result; it never blocks
  planning, asset verification, or other channels.
- Production upload remains explicitly user-authorized. This plan grants no
  publication authority.

## Authoritative Upstream Contract

The pin floor is `sc-publish` `develop` @
`ce85b4d284e563bce8be5bf632a97f200a90c0d2` (merge of sc-publish PR #38,
resolving upstream issues #30–#37). **The executable pin is the first
revision that additionally resolves upstream issues #39–#41** (fail-open
GH Release/winget idempotency probes; missing setuptools publish path).
Issue #39 is a hard ATM prerequisite: `hermes-atm` builds with
`setuptools.build_meta`, which the kit cannot publish until #39 lands. AT.1
records the final commit hash in its receipt and this document is updated to
it before the plan PR merges.

Installer contract at the pinned revision:

```bash
python plugins/sc-publish/.github/scripts/bootstrap_sc_compose.py --venv <venv>
<venv>/bin/python plugins/sc-publish/install.py --input release/sc-publish-consumer-input.json <atm-core-worktree>
<venv>/bin/python plugins/sc-publish/install.py --dry-run --input release/sc-publish-consumer-input.json <atm-core-worktree>
```

The final dry-run must print no drift. A nonzero dry-run that lists changes is
evidence to fix the consumer input or the upstream package, never permission
to hand-edit generated or copied assets.

## Sprint Sequence

| Sprint | Purpose | Dependency |
| --- | --- | --- |
| [AT.1](AT.1-canonical-consumer-install.md) | Install the pinned shared package with ATM's complete JSON input; prove parity. | Start point |
| [AT.2](AT.2-pypi-1.4.3-retry.md) | Authorized TestPyPI then PyPI 1.4.3 retry from immutable `main` assets. | must_follow AT.1 |
| [AT.3](AT.3-legacy-deletion.md) | Verify coverage, then delete the deprecated legacy publish surface. | must_follow AT.2 |

No AS work, files, acceptance criteria, or release receipts carry forward.
