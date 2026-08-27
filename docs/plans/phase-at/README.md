# Phase AT — Manifest-Driven Publishing Recovery

```yaml
plan_type: phase_index
phase: AT
status: proposed
branch: plan/phase-at-publish-recovery
worktree: plan/phase-at-publish-recovery
upstream_package: sc-publish
upstream_revision: 42e0fcea23f730fae0ef3d08b060cd4df6a2602e
```

## Goal

Replace no product code and add no ATM-specific publishing framework. Install
the shared `sc-publish` package from one immutable upstream revision using
ATM's complete JSON input and prove install parity (AT.1), then delete the
deprecated legacy publish surface the shared kit demonstrably covers (AT.2).

**Amendment (2026-08-27, owner decision — forward-only publishing).** The
original goal also included proving a retryable 1.4.3 Python release from
immutable `main` reaches TestPyPI and PyPI. Rand ruled re-publishing tags
that predate the kit installation out of scope ("I don't see a reason to try
to re-publish anything before the first kit install"). `v1.4.3` is
unpublishable via the kit — the kit action is absent at that tag and the
legacy manifest fails the kit schema (see the AT.1 receipt's TestPyPI
amendment). The publish-proof leg is retargeted to the **first kit-era tag
(workspace version `1.4.4`)**, cut after phase AT merges to `develop`;
TestPyPI authorization carries over, production remains
`pending-contemporaneous-authorization`.

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
  byte-for-byte through the pinned checkout's
  `plugins/sc-publish/install.py` (see Authoritative Upstream Contract); ATM
  never patches a copied file. A shared-file correction is an `sc-publish`
  issue/PR, adopted here through an unchanged installer run.
- ATM owns only the complete consumer JSON input and the manifests rendered
  from it. The input explicitly names every crate, Python distribution,
  binary, channel, publish order, and version source. Nothing is inferred.
- The consumer input must also preserve ATM's release-identity requirements:
  crate identity continuity under the legacy names `agent-team-mail` and
  `agent-team-mail-core` with the installed binary name `atm`
  (REQ-P-RELEASE-001/REQ-P-RELEASE-003), parity with the historical channels
  crates.io / GitHub Releases / Homebrew (REQ-P-RELEASE-002), and `winget` as
  a required additional channel (REQ-P-RELEASE-005). AT.1's acceptance
  criteria enforce these against the rendered manifests.
- GitHub environments own publishing tokens. A missing, expired, or rejected
  credential is a fail-closed, channel-scoped workflow result; it never blocks
  planning, asset verification, or other channels.
- Production upload remains explicitly authorized by the **release
  authorizer**: the human repository owner (Rand), acting outside any agent
  role. Wherever this plan says "operator", it means exactly this person; no
  agent, coordinator, or teammate can grant publication authority, and each
  authorization is quoted verbatim in the sprint receipt (see AT.1). This
  plan grants no publication authority.

## Authoritative Upstream Contract

Pinned revision: `sc-publish` `develop` @
`42e0fcea23f730fae0ef3d08b060cd4df6a2602e` (including PR #48 release-candidate
provenance gating and PR #50 stale bootstrap-wheel rejection on top of the
previous #45/#39–#41 fixes). This revision satisfies ATM's hard prerequisite
from issue #39: `hermes-atm` builds with `setuptools.build_meta`, which the kit
publishes via the manifest-declared build system. The AT.1 receipt records this
commit and its verified package identity: digest
`75da9d18426eacb92bab3e02bb6655a35e14f69deafe1ba2d00f4e93aabc0a5b` over 49
installable files (byte parity 49/49, dry-run clean, kit tests 81 passed/8
skipped, and manifest validation passed). The complete verification is
retained at `recovered/repin-verify-42e0fce-receipt.md`; adopting any newer
revision restarts AT.1's proof.

Installer contract at the pinned revision. Every command runs FROM the
pinned, read-only `sc-publish` checkout (`<sc-publish-checkout>` below, at
`42e0fcea23f730fae0ef3d08b060cd4df6a2602e`), with the ATM worktree as the
install target:

```bash
python <sc-publish-checkout>/plugins/sc-publish/.github/scripts/bootstrap_sc_compose.py --venv <venv>
<venv>/bin/python <sc-publish-checkout>/plugins/sc-publish/install.py --input release/sc-publish-consumer-input.json <atm-core-worktree>
<venv>/bin/python <sc-publish-checkout>/plugins/sc-publish/install.py --dry-run --input release/sc-publish-consumer-input.json <atm-core-worktree>
```

**Watchout (upstream issue sc-publish#46):** the installer copies a vendored
`install.py` to the consumer repo root; that vendored copy resolves its
`PACKAGE_ROOT` to the consumer repo itself and must **never** be run — it
would treat the whole ATM repo (including `.git`) as the package. At first
install the consumer also does not yet contain any kit files (chicken-and-egg),
so the checkout-hosted commands above are the only valid form, before and
after install.

The final dry-run must print no drift. A nonzero dry-run that lists changes is
evidence to fix the consumer input or the upstream package, never permission
to hand-edit generated or copied assets.

The new release-candidate provenance gate (sc-publish PR #48) runs in
`release-candidate.yml`/`release.yml` before release creation. The
`pypi-publish.yml` dispatch accepts only `tag` and `target`, and then requires
the named GitHub Release to be published, non-draft, and to have all required
assets. An immutable v1.4.3 retry would therefore have used the post-release
path without re-running provenance or rebuilding assets — but per the
2026-08-27 forward-only ruling (see Goal amendment) the v1.4.3 retry is
withdrawn: `pypi-publish.yml` checks out the release tag's tree for its
action and manifest, so only tags cut from kit-installed trees are
publishable. The publish proof runs against the first kit-era tag (1.4.4);
this is recorded in the AT.1 receipt and the recovered re-pin verification
receipt.

## Sprint Sequence

| Sprint | Purpose | Dependency |
| --- | --- | --- |
| [AT.1](AT.1-install-parity-and-publish.md) | Install the pinned shared package and prove parity. The 1.4.3 publish leg was retargeted to the first kit-era tag (1.4.4) per the forward-only ruling (see Goal amendment). | Start point |
| [AT.2](AT.2-legacy-deletion.md) | Verify coverage, then delete the deprecated legacy publish surface. | must_follow AT.1 |

Each sprint doc's frontmatter `status:` flips to `in-progress` when its
worktree is created and to `complete` in the same PR that merges the sprint's
final commit; this README's sprint table is updated in that same PR.

No AS work, files, acceptance criteria, or release receipts carry forward.

## Recovered Rehearsal Evidence

The files under `recovered/` are byte-identical copies imported from
`origin/smoke/phase-at-at1-rehearsal`: the consumer input and its rehearsal
receipt produced at the previous pin `0fa5b05e44a655ec76ada8a6c2b24714d47acca1`.
AT.1 promotes the recovered consumer input only after diff review against the
Publish Surface Ground Truth and a complete install/dry-run/parity/test
re-validation at the new pin. The re-pin verification receipt is also retained
from `origin/smoke/phase-at-repin-verify` at `1803451638`, recording the
compatible new pin, digest, 49-file parity, clean dry-run, kit test result, and
manifest validation. These recovered copies are now the evidence of record;
the two scratch smoke branches are eligible for cleanup after this import.

## Branch Hygiene

The first-attempt cleanup is recorded here: PR #935 was closed and
`feature/vendor-sc-compose-publishing-skill` was deleted;
`fix/publish-manifest-complete` was fully merged or superseded and deleted;
`integrate/publish-release-readiness` was fully merged by PR #425 with zero
commits ahead of `develop`, but remote deletion was rejected by the
`integrate/*` repository ruleset and is pending manual deletion by the
repository owner. The two scratch smoke branches
(`smoke/phase-at-at1-rehearsal` and `smoke/phase-at-repin-verify`) are eligible
for deletion after the artifact recovery above. No branch cleanup removes
evidence because the recovered copies are retained in this plan branch.

## Upstream Extension-Point Doctrine

When a defect is found in the shared package, file a generic extension-point
request upstream in `sc-publish`. Never add ATM-specific customization to the
general publish process or patch a copied kit file locally (ADR-050).

## Branch Strategy

Per repo convention, phase AT uses a dedicated integration branch:

- `integrate/phase-at` is created off `develop` at phase start.
- Sprint PRs target `integrate/phase-at`, never `develop` directly:
  `feature/pat-s1-install-and-publish` and
  `feature/pat-s2-legacy-publish-deletion`.
- Each later sprint merges the latest `integrate/phase-at` into its feature
  branch before opening its PR.
- After AT.2 merges, one final PR merges `integrate/phase-at` → `develop`.
- All merges are merge commits (`gh pr merge --merge`); never squash.
- `quality-mgr` gates every PR.

## Receipt Convention

Each sprint appends its receipt at
`docs/plans/phase-at/receipts/AT.<n>-receipt.md` with a fixed shape:

- the pinned `sc-publish` revision and package digest. **Package digest
  algorithm (fixed for all AT receipts):** sha256 over the newline-joined,
  path-sorted list of `"<sha256(file)>  <relative-path>"` lines for all
  copied installable package files (excluding `.sc-publish-source-root`,
  `__pycache__`, `*.pyc`, and any pre-rendered `release/publish-*.toml`),
  matching the recovered rehearsal receipt;
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
| Legacy `.just` tests failing after install, before AT.2 | Expected, split by kind | Expectation updates for installer-overwritten workflows (`.just/lint-config.toml` release-wiring fragments; assertions in `.just/tests/test_release_preflight.py` and `test_release_homebrew_workflow.py`) are an **AT.1 work item** — consumer-owned validation data per ADR-050, updated so the AT.1 branch CI passes. Retention-vs-deletion of those test files remains **AT.2's** data-vs-behavior split. |
| Immutable-asset hash mismatch, or a wrong version visible on a public index | GENUINE STOP | Halt that channel; escalate to team-lead. |

Only the last row stops work. An explicitly identified fail-closed outcome is
the workflow succeeding at its job; treating it as an emergency is the
failure mode this section exists to prevent. Sprint dispatch assignments must
include this table.

SUCCESS EVIDENCE here means evidence of correct fail-closed behavior — never
evidence of publication. A fail-closed credential result does not close
AT.1's publication acceptance criteria or open any AT.2 deletion gate; those
require receipts showing the public indexes actually contain the released
distributions (see AT.1 Acceptance Criteria and AT.2 Prerequisites).
