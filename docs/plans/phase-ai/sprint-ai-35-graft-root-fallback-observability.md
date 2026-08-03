---
title: AI.35 graft-root fallback observability
status: in_progress
branch: feature/pAI-s35-graft-root-fallback-observability
target: integrate/phase-AI
depends_on: AI.34
requires_merged_pr: PR #681 (Hermes graft nudge-endpoint reconciliation)
---

# AI.35 — graft-root fallback observability

## Closure

`canonical_graft_root()` and the deprecated `compatible_home_dir()` no longer
silently pick between `workspace_root`/`home_dir`/`legacy_cwd` with zero
observability. Whichever branch fires is logged, and the operational gap that
lets `workspace_root` drift out of sync with a live graft session's actual
root is either closed or explicitly documented as an accepted manual step.

## Background

Found by quality-mgr's AI34-QA-1 review as `RBQA-F002-AI34` (Important,
non-gating on PR #681):

`canonical_graft_root()` (`crates/atm-core/src/schema/agent_member.rs:53-56`)
silently falls back from roster `workspace_root` to `home_dir` (via
`canonical_home_dir()`) with no logging distinguishing which branch fired.
`workspace_root` is populated only via a manual operator step
(`atm team update-member --workspace-root`,
`crates/atm-core/src/team_admin/member_mutation.rs:384-389`) with no
automatic reconciliation to the value an atm-graft session actually runs
with. `docs/plans/phase-ai/hermes-graft-runbook.md` has zero references to
`update-member`/`workspace-root`, so there's no documented operational step
keeping the two in sync.

This is the same "two modules independently answer the same architectural
question" pattern that caused the original AI.34 bug, just pushed from code
to config, with a silent compatibility fallback and no mechanical lint guard.
The deprecated `compatible_home_dir()` (line 62-65) exhibits the identical
silent-fallback pattern (`home_dir` → `legacy_cwd`), even though marked
deprecated.

Triage record: `.triage/phase-AI/findings/RBQA-F002-AI34.ttl` (severity:
important, repeatable: true, sweepScope: workspace).

## Required fixes

1. Add observability (structured log at minimum) to `canonical_graft_root()`
   distinguishing which source resolved (`workspace_root` vs `home_dir`
   fallback), so a future endpoint mismatch is diagnosable without re-running
   this investigation from scratch.
2. Do the same for `compatible_home_dir()`'s `home_dir` → `legacy_cwd`
   fallback, or confirm it's unreachable in practice and remove it if so
   (it's already marked deprecated).
3. Document the `update-member --workspace-root` operational step in
   `docs/plans/phase-ai/hermes-graft-runbook.md`, including when it must be
   re-run to keep the roster's `workspace_root` in sync with a live graft
   session's actual root.
4. Judgement call, confirm before large scope: if reconciling `workspace_root`
   automatically (rather than just documenting the manual step) is small,
   prefer it — it structurally prevents the class of drift that caused
   AI.34's original bug. If it's a bigger lift, document-only is acceptable
   for this sprint; file a follow-up finding instead of expanding scope here.

## Required validation

- A test proves `canonical_graft_root()`'s log output correctly identifies
  which source resolved for both the `workspace_root`-present and
  `workspace_root`-absent cases.
- `just lint`, `just test` pass.

## Non-goals

`RBQA-F001-AI34` (CLI's `GraftNudgeSink::deliver` vs the daemon's canonical
resolver — reviewer disagreement on dead-code reachability) is a separate
finding routed to arch-ctm, not part of this sprint.
