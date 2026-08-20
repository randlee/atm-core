# Phase AT — Manifest-Driven Publishing Recovery

```yaml
plan_type: phase_index
phase: AT
status: proposed
branch: plan/phase-at-publish-recovery
worktree: plan/phase-at-publish-recovery
upstream_package: sc-publish
upstream_revision: 0fa5b05e44a655ec76ada8a6c2b24714d47acca1
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

Pinned revision: `sc-publish` `develop` @
`0fa5b05e44a655ec76ada8a6c2b24714d47acca1` (merge of sc-publish PR #45,
resolving upstream issues #39–#41 on top of PR #38's #30–#37 fixes). This
revision satisfies ATM's hard prerequisite from issue #39: `hermes-atm`
builds with `setuptools.build_meta`, which the kit publishes via the
manifest-declared build system. AT.1 records this commit hash and the package
digest in its receipt; adopting any newer revision restarts AT.1's proof.

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
