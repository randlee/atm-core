---
title: Winget Token Wiring Fix
status: complete
branch: fix/winget-token-wiring
worktree: /Users/randlee/Documents/github/atm-core-worktrees/fix/winget-token-wiring
---

# Sprint — Winget Token Wiring Fix

## Goal

Repair the `winget` release wiring so the automated `publish-winget` job uses
the dedicated fork-write PAT that already exists in repo secrets, and remove
stale documentation that still claims the default Actions token is sufficient.

## Root Cause

- GitHub issue `#520` documents `publish-winget` failing with
  `Resource not accessible by integration`
- the repo already has a `WINGET_GITHUB_TOKEN` secret provisioned
- `.github/workflows/release.yml` still wires the `publish-winget` job to
  `${{ secrets.GITHUB_TOKEN }}`
- the default Actions token cannot create branches / PRs against the
  `randlee/winget-pkgs` fork used by
  `vedantmgoyal2009/winget-releaser@v2`
- this is a workflow/doc wiring bug, not a code-signing or certificate issue

## Exact Targets

- `.github/workflows/release.yml`
- `docs/WINGET_SETUP.md`
- `.claude/agents/publisher.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/project-plan.md`

## Deliverables

1. `docs/plans/winget-token-wiring/sprint-winget-token-wiring.md`
   captures the authoritative scope for this standalone fix sprint
2. `.github/workflows/release.yml` switches the `publish-winget` job `token:`
   input from `${{ secrets.GITHUB_TOKEN }}` to
   `${{ secrets.WINGET_GITHUB_TOKEN }}`
3. `docs/WINGET_SETUP.md` states that a dedicated
   `WINGET_GITHUB_TOKEN` repo secret is required and that it must be a PAT
   capable of creating branches / PRs against the `randlee/winget-pkgs` fork
4. all other live workflow/doc references to the old default-token assumption
   are corrected in the same sprint
5. `docs/project-plan.md` includes an entry for this sprint

## Verification Boundary

- this sprint is code/doc wiring only
- do not trigger the release workflow from this sprint
- validation of the PAT’s real fork-write permission remains the repo owner’s
  responsibility outside this sprint

## Acceptance Criteria

- `publish-winget` reads `token: ${{ secrets.WINGET_GITHUB_TOKEN }}`
- no live doc still claims the default `GITHUB_TOKEN` is sufficient for
  `winget` publishing
- every corrected doc makes the PAT requirement and fork-write scope explicit
- `docs/project-plan.md` records this sprint before closeout
- `just lint` passes
- `just validate` passes

## Required Validation

- `rg -n "WINGET_GITHUB_TOKEN|GITHUB_TOKEN|winget-specific secret|default GitHub workflow token" .github/workflows/release.yml docs/WINGET_SETUP.md .claude/agents/publisher.md docs/requirements.md docs/architecture.md`
- `just lint`
- `just validate`
