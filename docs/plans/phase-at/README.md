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

Each sprint doc's frontmatter `status:` flips to `in-progress` when its
worktree is created and to `complete` in the same PR that merges the sprint's
final commit; this README's sprint table is updated in that same PR.

No AS work, files, acceptance criteria, or release receipts carry forward.

## Branch Strategy

Per repo convention, phase AT uses a dedicated integration branch:

- `integrate/phase-at` is created off `develop` at phase start.
- Sprint PRs target `integrate/phase-at`, never `develop` directly:
  `feature/pat-s1-canonical-consumer-install`,
  `feature/pat-s2-pypi-143-retry`, and
  `feature/pat-s3-legacy-publish-deletion`.
- Each later sprint merges the latest `integrate/phase-at` into its feature
  branch before opening its PR.
- After AT.3 merges, one final PR merges `integrate/phase-at` → `develop`.
- All merges are merge commits (`gh pr merge --merge`); never squash.
- `quality-mgr` gates every PR.

## Receipt Convention

Each sprint appends its receipt at
`docs/plans/phase-at/receipts/AT.<n>-receipt.md` with a fixed shape:

- the pinned `sc-publish` revision and package digest;
- the consumer-input sha256 (`release/sc-publish-consumer-input.json`);
- the rendered-manifest sha256s;
- every executed command with its exit code;
- workflow run IDs/URLs where applicable;
- a findings list, including fail-closed results recorded as evidence (see
  Non-Blocking Outcomes below).

## Non-Blocking Outcomes

Phase AT's chief churn risk is not a broken workflow — it is a team treating
an expected fail-closed result as an emergency (a team once stopped work for
eight hours over an unverified stale-token fear). Classify every non-passing
result with this table before reacting to it:

| Result | Classification | Action |
| --- | --- | --- |
| Credential missing, expired, or rejected, named by channel | SUCCESS EVIDENCE — the fail-closed design working | Record the receipt; continue all other channels and work. Credential state is user-owned and asynchronous — never rotate, diagnose, or wait on tokens mid-sprint. |
| 404 for a not-yet-published version on a public registry | Expected | Proceed. |
| A probe returning indeterminate and hard-failing its leg | SUCCESS EVIDENCE — fail-closed per sc-publish #40/#41 | Retry the leg; escalate only if deterministic. |
| `install.py --dry-run` drift | Work item | Fix the consumer input or file an upstream `sc-publish` issue; never a stop, never a hand-edit. |
| Legacy `.just` tests failing after install, before AT.3 | Expected classification work | Route to AT.3's data-vs-behavior split. |
| Immutable-asset hash mismatch, or a wrong version visible on a public index | GENUINE STOP | Halt that channel; escalate to team-lead. |

Only the last row stops work. An explicitly identified fail-closed outcome is
the workflow succeeding at its job; treating it as an emergency is the
failure mode this section exists to prevent. Sprint dispatch assignments must
include this table.
