---
id: PREFLIGHT
title: Release Preflight Documentation and Publisher Alignment
status: complete
branch: docs/preflight-documentation
worktree: /Users/randlee/Documents/github/atm-core-worktrees/docs/preflight-documentation
target: develop
---

# Sprint PREFLIGHT — Release Preflight Documentation and Publisher Alignment

## Goal

Create one authoritative operator-facing release-preflight checklist, align the
publisher prompt to the real repo-owned validation surface, add a `/preflight`
slash command, and delete stale live docs that conflict with the current
release flow.

## Research Inputs

- `Justfile`
- `.just/run_lint.py`
- `scripts/validate_release.py`
- `.github/workflows/release-preflight.yml`
- `.claude/agents/publisher.md`
- `release/publish-artifacts.toml`
- `docs/project-plan.md`

## Exact Targets

- `docs/release-preflight-checklist.md`
- `.claude/commands/preflight.md`
- `.claude/agents/publisher.md`
- `docs/project-plan.md`
- `docs/lock-release-gate.md`
- `docs/publishing-improvements/plan.md`

## Current Facts This Sprint Must Preserve

- `just validate` is the canonical local preflight entrypoint
- `scripts/validate_release.py` currently exposes these targets:
  - `all`
  - `validate` (alias of `all`)
  - `lint`
  - `support-files`
  - `manifest`
  - `publish-surface`
  - `release-binaries`
  - `inventory`
  - `cargo-lock-drift`
  - `dependency-currency`
  - the optional historical `phase-ad-readiness` diagnostic (thorough-smoke
    only; not part of the default release target)
- `just lint all` currently gates 21 subchecks:
  - `fmt`
  - `clippy`
  - `deny`
  - `shear`
  - `version`
  - `identities`
  - `lines`
  - `boundaries`
  - `unix-gating`
  - `same-host-portability`
  - `runtime-waits`
  - `manifests`
  - `silent-emit`
  - `function-length`
  - `legacy-mailbox-paths`
  - `capability-degradation`
  - `spell`
  - `fixed-sleep`
  - `ttl-triage`
  - `daemon-singleton`
  - `pytests`
- the repo also exposes three advisory/manual lint lanes that are not part of
  `just lint all`:
  - `modules`
  - `sc-boundary`
  - `sc-portability`
- the release-preflight workflow adds CI-only behavior that `just validate`
  does not perform locally:
  - `run_by_agent=publisher` ownership assertion
  - workflow checkout/tool installation/bootstrap
  - version normalization from workflow input
  - deterministic installed-doc staging
  - upload of `release-findings.json`

## Deliverables

- `docs/release-preflight-checklist.md` exists as the operator-facing checklist
  for release preflight
- the checklist enumerates every current `validate_release.py` target and every
  `just lint` subcheck, with each item traced to:
  - a concrete local command path, or
  - a CI-only workflow step, or
  - an agent-specific manual step that is intentionally not script-covered
- `.claude/agents/publisher.md` references the checklist and no longer points
  at stale preflight artifact names or stale crate-surface prose
- `.claude/commands/preflight.md` exists and defines:
  - default mode: run `just validate`, then agent-specific non-script checks
  - `--fix` mode: create `sc-git-worktree` from `develop`, apply fixes,
    commit/push, open PR to `develop`, and require normal QA with no auto-merge
- `docs/project-plan.md` contains an explicit entry for this sprint
- stale live release docs are removed:
  - `docs/lock-release-gate.md`
  - `docs/publishing-improvements/plan.md`
- all references to the deleted docs are repointed to authoritative surviving
  documents

## Deletion Policy

- delete stale/conflicting live docs instead of retaining parallel release
  narratives
- phase-plan history remains in `docs/plans/**`
- if a live doc is deleted, all repo references to it must be updated in the
  same sprint

## Acceptance Criteria

- the checklist is an operator layer only and points readers to `just validate`
  / repo scripts for implementation detail
- no live document still instructs release preflight through the deleted docs
- publisher prompt, checklist, and slash command agree on:
  - the canonical local command
  - CI-only workflow additions
  - agent-specific checks that remain manual
- `git diff --check` passes
- `just lint` passes
- `just validate` passes

## Required Validation

- `just lint`
- `just validate`
- `git diff --check`
- `rg -n "docs/publishing-improvements/plan.md|docs/lock-release-gate.md" docs .claude release --glob '!docs/plans/preflight-documentation/sprint-preflight.md'`
