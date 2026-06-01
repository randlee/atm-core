---
status: complete
branch: feature/publishing-improvements
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/publishing-improvements
---

# Publishing Improvements Plan

## Goal

Make publisher-driven releases predictable, low-churn, and self-contained:

- publisher may start from `develop` or `main`
- release execution always converges onto a short-lived `release/vX.Y.Z` branch
  cut from `main`
- the canonical preflight is `just validate`
- all known publish failures must be caught before the release workflow starts
- routine missing inputs come from `team-lead`, not from the user
- any publish-time failure that should have been preflighted becomes an
  automatic GitHub issue so the same surprise does not recur

## Governing Decisions

### Release source model

- `develop` remains an allowed launch point only for publisher to shepherd the
  release PR into `main`
- `main` remains the source branch for cutting the release line
- actual release execution happens from `release/vX.Y.Z`
- any release-window fixes happen on `release/vX.Y.Z`, not directly on `main`
  and not on `develop`
- after the release branch merges to `main`, publisher must immediately ensure
  the resulting release commits flow back into `develop` via a `main ->
  develop` reconciliation PR if one is not already open

### Canonical preflight

- `just validate` is the single local command that represents release
  preflight
- phase-ending review should require `just validate` whenever a phase changes
  publishable crates, release workflows, release manifests, version SSOT,
  release notes templates, or other release-surface inputs; that gate blocks
  merge into `develop`, not just the later publish attempt
- release-preflight CI should use the same validation logic, not a different
  handwritten checklist

### No-surprises policy

- every known publish failure class must be represented in preflight
- publisher should get the full blocker set in one pass
- publisher should batch mechanical fixes and avoid one-blocker-per-PR churn

### Escalation policy

- publisher requests routine missing inputs from `team-lead`
- publisher does not ask the user for ordinary release coordination details
- user escalation is reserved for real policy ambiguity only

### Failure ratchet

- if a publish-time failure should have been caught by preflight, publisher
  must file a GitHub issue immediately with exact failure details and the
  missing gate

## Current Status Audit

### `GH #376` items

- add `just lint` to preflight:
  - `PLANNED TO LAND AS PART OF just validate`
- add manifest / preflight-check / publish-order validation:
  - `ALREADY PRESENT`
- add `cargo package -p agent-team-mail-core --locked`:
  - `ALREADY PRESENT` in workflow, but not yet unified under `just validate`
- add archive membership verification for `atm` + `atm-daemon`:
  - `MISSING` before this branch
- aggregate all blockers into one preflight result set:
  - `MISSING`; current release validation flow does not yet require a single
    `release-findings.json` artifact with all blockers collected before exit
- check Cargo.lock drift against `origin/main`:
  - `MISSING` as an explicit release-window gate
- check dependency currency and file stale-version issues automatically:
  - `MISSING`; this was only a follow-on idea before publisher review
- treat failures as hard stops:
  - `PARTIAL`; prompt already stops on gate failures, but the full local gate
    was missing

### `GH #380` items

- lineage gate friction:
  - `RESOLVED` in current [scripts/release_gate.sh](/Users/randlee/Documents/github/atm-core-worktrees/feature/publishing-improvements/scripts/release_gate.sh)
    because the old `develop ⊂ main` enforcement is already gone
- publish from `main`, not from live `develop` ancestry:
  - `NOT FULLY DOCUMENTED` before this branch
- Cargo.lock drift / dependency bump policy:
  - `PARTIAL`; lock/version checks exist, but the publisher prompt still needed
    stronger release-window guidance
- parallel comprehensive preflight:
  - `PARTIAL`; workflow had many checks, but no canonical `just validate`
    contract
- stale state communication:
  - `MISSING`; no structured `STATE:` block requirement
- release notes template / source of truth:
  - `MISSING`; the prompt referenced a template that was absent
- winget bootstrap handling:
  - `PARTIAL`; docs existed, but prompt handling was too thin
- develop fast-forward / divergence semantics:
  - `SUPERSEDED` by the release-branch model in this plan
- post-release `main -> develop` reconciliation after release-branch merges:
  - `MISSING` as an explicit publisher-owned operational step
- publishability / surface-expansion escalation example:
  - `MISSING`; the prompt did not yet include a concrete example such as
    `sc-lint-attributes` unexpectedly becoming a production publish blocker

## Sprint Breakdown

### Sprint PI.1 — Canonical Validation Infrastructure

Goal:
- create the single authoritative preflight command and align repo-owned
  release metadata around it

Deliverables:
- `Justfile`
  - add `just validate`
- `scripts/validate_release.py`
  - canonical local preflight runner
  - collect all blockers and emit `release-findings.json`
  - exit non-zero only after the full blocker set is recorded
- `scripts/release_artifacts.py`
  - release-binary validation helper(s)
- `release/publish-artifacts.toml`
  - retained release-binary SSOT includes both `atm` and `atm-daemon`
- `release/RELEASE-NOTES-TEMPLATE.md`
  - real file exists in repo
- `.github/workflows/release-preflight.yml`
  - canonical validation suite invoked from CI
- `.github/workflows/release.yml`
  - release packaging driven from manifest release binaries, with archive
    membership verification
- release validation logic
  - explicit `Cargo.lock` drift check against `origin/main`
  - dependency version-currency check using `cargo search`, gated by env var so
    CI can suppress issue-spam while local or release contexts can file issues

Acceptance:
- `just validate` runs the full local retained release preflight
- `just validate` includes:
  - `just lint`
  - manifest coverage / preflight-mode / publish-order checks
  - publish-surface package / dry-run checks
  - release support-file checks
  - release inventory generation/shape check
  - retained release-binary validation
- `just validate` emits `release-findings.json` and does not stop at the first
  blocker
- `just validate` checks whether `Cargo.lock` drifted from `origin/main` during
  the release window
- `just validate` includes a version-currency check path, env-gated for CI
  noise control and capable of auto-filing a GitHub issue on stale results
- release manifest declares both `atm` and `atm-daemon`
- release archives are built from the manifest binary list and verified for
  expected membership
- dependency version-pin × publish-flag mismatches are explicitly covered by the
  combined manifest validation plus package / dry-run checks; `cargo publish
  --dry-run` alone is not treated as sufficient proof

### Sprint PI.2 — Publisher Prompt and Release-Branch Discipline

Goal:
- make publisher operationally self-sufficient and remove user babysitting from
  routine release work

Deliverables:
- `.claude/agents/publisher.md`
  - explicit `develop` and `main` launch modes
  - release execution converges on `release/vX.Y.Z`
  - any release-window fixes land on `release/vX.Y.Z`, regardless of launch
    mode
  - routine missing inputs sourced from `team-lead`
  - `just validate` mandated before release workflow execution
  - structured `STATE:` status report format
  - explicit instruction to batch blocker fixes rather than serial PR churn
  - explicit post-release `main -> develop` reconciliation step after the
    release branch merges back to `main`
  - concrete winget handoff mechanics, including `komac` CLI steps and the
    expectation that a first-time manual Store handoff is not itself a workflow
    failure
  - concrete user-escalation examples, including the publishability /
    surface-expansion case where a dependency such as `sc-lint-attributes`
    becomes production-facing unexpectedly

Acceptance:
- publisher no longer relies on `develop` ancestry as the live release gate
- publisher documents that all release-window fixes happen on `release/vX.Y.Z`
- publisher documents the exact `main -> develop` reconciliation step required
  after release-branch merges so version-bump commits are not stranded on
  `main`
- publisher asks `team-lead`, not the user, for release notes / changelist /
  missing coordination inputs
- publisher status reports include a structured `STATE:` block
- publisher documents the expected winget / `komac` handoff path and treats a
  first-time manual handoff as operational follow-through, not as a workflow
  failure

Operational detail:
- after merge of `release/vX.Y.Z -> main`, publisher must:
  - verify whether a `main -> develop` PR already exists
  - create it immediately if missing
  - route any missing changelist or release-note follow-through to `team-lead`
- winget handoff text in the prompt should include the expected `komac` CLI
  path, for example:
  - `komac update <package-id> --version <X.Y.Z> --urls <artifact-url> ...`
  - if the first submission still requires manual Store-side approval or repo
    bootstrapping, publisher records that as handoff status, not as a failed
    release workflow

### Sprint PI.3 — Failure Ratchet and Remaining Process Hardening

Goal:
- ensure every future release surprise turns into a tracked preflight or prompt
  improvement

Deliverables:
- `.claude/agents/publisher.md`
  - automatic GitHub issue requirement for any publish-time failure that should
    have been caught by preflight
- `docs/publishing-improvements/plan.md`
  - closure proof matrix mapping every known `v1.2.0` incident category to the
    preflight or prompt control that prevents recurrence
- follow-on automation/documentation plan for:
  - release incident reporting templates if needed

Publisher review checkpoint:
- publisher must sign off on:
  - whether `just validate` covers all failure classes encountered in the last
    release
  - whether any remaining late-release surprises still lack a gate
  - whether the `STATE:` block and release-branch workflow are operationally
    sufficient

Acceptance:
- publisher prompt requires issue filing for missed preflight failures
- the plan contains a closure proof matrix for `v1.2.0` incident categories and
  confirms the intended preflight coverage for each
- remaining non-blocking release-process gaps are enumerated explicitly, not
  left implicit

## Coverage Notes

### Dependency version-pin × publish-flag cross-check

- this plan treats the cross-check as the combined responsibility of:
  - manifest/version SSOT validation
  - publish-surface manifest validation
  - `cargo package --locked`
  - `cargo publish --dry-run` where applicable
- `cargo publish --dry-run` by itself is not enough to prove internal
  path-version correctness or publish-surface completeness

### User escalation examples

- ordinary missing inputs are not user blockers:
  - release notes not refreshed
  - changelist not refreshed
  - missing `main -> develop` reconciliation PR
  - missing release-branch follow-through
- true escalation examples are policy decisions:
  - a publishability / surface-expansion change such as
    `sc-lint-attributes` unexpectedly becoming a production dependency in a
    publishable crate
  - a disputed decision about whether a crate must ship publicly or may remain
    internal

### `v1.2.0` closure proof matrix

| Incident category | Required preflight / prompt control |
|---|---|
| internal dev-dependency version-pin leakage (`atm-runtime-test-support`) | `just lint` plus manifest/version SSOT validation plus `cargo package --locked` |
| publish-surface manifest drift or wrong publish order | `validate-manifest`, `validate-preflight-checks`, `validate-publish-order` inside `just validate` |
| release archives missing `atm-daemon` | manifest-declared release binaries plus archive membership verification inside `just validate` |
| `Cargo.lock` drift or silent release-window dependency bump | explicit `Cargo.lock` drift check against `origin/main` in `just validate` |
| stale dependency currency discovered late | env-gated version-currency check plus auto-filed GitHub issue when stale |
| publisher discovers blockers serially and opens multiple PRs | `release-findings.json` aggregation requirement so preflight returns the full blocker set in one pass |
| release notes template / changelist source missing | release support-file checks plus publisher requirement to obtain missing inputs from `team-lead` |
| winget handoff confusion or first-submission bootstrap surprise | explicit `komac` handoff steps in prompt plus non-failure status treatment for manual Store follow-through |
| publishability / surface-expansion ambiguity (for example `sc-lint-attributes`) | publisher prompt escalation example routes the policy question to `team-lead` / user instead of treating it as a mechanical blocker |
| post-release divergence between `main` and `develop` | explicit publisher-owned `main -> develop` reconciliation step after release-branch merge |

## Planning Branch Scope

This branch is planning-only.

Allowed closeout artifacts on this branch:
- `docs/publishing-improvements/plan.md`
- `docs/project-plan.md`

Everything else described in `PI.1` through `PI.3` is implementation work for
later sprint branches after publisher reviews the plan.
