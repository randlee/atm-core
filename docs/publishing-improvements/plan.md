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

### Canonical preflight

- `just validate` is the single local command that represents release
  preflight
- phase-ending review should require `just validate` so release-surface drift
  is found before publisher starts a release
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

Acceptance:
- `just validate` runs the full local retained release preflight
- `just validate` includes:
  - `just lint`
  - manifest coverage / preflight-mode / publish-order checks
  - publish-surface package / dry-run checks
  - release support-file checks
  - release inventory generation/shape check
  - retained release-binary validation
- release manifest declares both `atm` and `atm-daemon`
- release archives are built from the manifest binary list and verified for
  expected membership

### Sprint PI.2 — Publisher Prompt and Release-Branch Discipline

Goal:
- make publisher operationally self-sufficient and remove user babysitting from
  routine release work

Deliverables:
- `.claude/agents/publisher.md`
  - explicit `develop` and `main` launch modes
  - release execution converges on `release/vX.Y.Z`
  - routine missing inputs sourced from `team-lead`
  - `just validate` mandated before release workflow execution
  - structured `STATE:` status report format
  - explicit instruction to batch blocker fixes rather than serial PR churn

Acceptance:
- publisher no longer relies on `develop` ancestry as the live release gate
- publisher documents that all release-window fixes happen on `release/vX.Y.Z`
- publisher asks `team-lead`, not the user, for release notes / changelist /
  missing coordination inputs
- publisher status reports include a structured `STATE:` block

### Sprint PI.3 — Failure Ratchet and Remaining Process Hardening

Goal:
- ensure every future release surprise turns into a tracked preflight or prompt
  improvement

Deliverables:
- `.claude/agents/publisher.md`
  - automatic GitHub issue requirement for any publish-time failure that should
    have been caught by preflight
- follow-on automation/documentation plan for:
  - stale dependency / version-currency reporting
  - winget bootstrap/manual handoff clarity
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
- remaining non-blocking release-process gaps are enumerated explicitly, not
  left implicit

## Planning Branch Scope

This branch is planning-only.

Allowed closeout artifacts on this branch:
- `docs/publishing-improvements/plan.md`
- `docs/project-plan.md`

Everything else described in `PI.1` through `PI.3` is implementation work for
later sprint branches after publisher reviews the plan.
