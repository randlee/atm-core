---
id: AE.4
title: Smoke Closeout And Release Readiness
status: planned
branch: feature/pAE-s4-smoke-closeout
worktree: ../atm-core-worktrees/feature/pAE-s4-smoke-closeout
target: integrate/phase-AE
---

# Sprint AE.4 — Smoke Closeout And Release Readiness

## Goal

- verify all Phase AE deliverables on the accepted line
- update project documentation for AE changes
- tag v1.3.0 release candidate

## Hard Dependencies

- `AE.3` complete
- `docs/plans/phase-AE/plan-phase-AE.md`
- `reports/smoke/smoke.md`
- `reports/smoke/smoke-thorough.md`
- `release/release-notes.md`
- `CHANGELOG.md`

## Exact Targets

- `reports/smoke/smoke.md` (update for AE surfaces)
- `reports/smoke/smoke-thorough.md` (update for AE surfaces)
- `release/release-notes.md`
- `CHANGELOG.md`
- `docs/requirements.md` (update for remove-member, send pipeline)
- `docs/atm/requirements.md`
- `docs/atm-core/requirements.md`
- `docs/project-plan.md` (add Phase AE record)

## Smoke Coverage

Add smoke tests for all AE surfaces:

- `atm teams remove-member <team> <name>` — success path
- `atm teams remove-member <team> <name>` — member not found
- `atm teams remove-member <team> <name>` — team not found
- `atm send --stdin <agent>` — stdin payload delivery
- `atm send --file <path> <agent>` — file payload delivery
- `atm send --stdin --file <path>` — mutual exclusion error
- `atm send --stdin` with >1MB payload — warning behavior
- pi-agent-atm nudge delivery after `atm send`
- codex-atm nudge delivery after `atm send`
- `python3 .just/preflight.py` exit 0

## Thorough Smoke

Add thorough smoke coverage:

- remove-member caller-context enforcement (missing ATM_IDENTITY, missing ATM_TEAM)
- remove-member idempotency (removing already-removed member)
- send pipeline with empty stdin
- send pipeline with binary file
- graft emitter failure mode (socket not available)
- concurrent send + remove-member (no race condition)

## Documentation Updates

- `CHANGELOG.md`: Phase AE entry with all four sprint summaries
- `release/release-notes.md`: v1.3.0 summary
- `docs/requirements.md`: add remove-member to retained command surface
- `docs/project-plan.md`: add Phase AE planning note

## Deliverables

- smoke report passes on the AE integration branch
- thorough smoke report passes
- release notes drafted for v1.3.0
- CHANGELOG updated
- requirements docs reflect new surfaces

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- smoke report: all AE checks pass
- thorough smoke report: all AE checks pass
- `cargo publish --dry-run` succeeds for every publishable crate
- `python3 .just/preflight.py` exits 0
