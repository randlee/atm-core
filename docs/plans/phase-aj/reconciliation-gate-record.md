---
title: Phase AJ — Phase-AI reconciliation gate record
status: override-authorized
---

# Phase-AI reconciliation gate record

`plan-phase-aj.md`'s Phase-entry gate requires: Phase AI's PR merges to
`develop`; team-lead records the resulting `develop` SHA, creates
`integrate/phase-AJ` from it, and diffs the pinned AI baseline against that
SHA for every AJ exact target before AJ.1 implementation begins.

## Actual state at the time `integrate/phase-aj` was cut

- Phase AI is **not** formally closed. `docs/project-plan.md` §40 still reads
  `[ACTIVE — implementation through AI.38; readiness blocked]`.
  `docs/plans/phase-ai/readiness.md` is `status: blocked`, pending physical
  two-Mac and Mac↔Windows peer evidence (`AI10-TWOMAC-001`,
  `AI10-WINDOWS-001`) — a release-readiness gate, separate from code merge.
- `integrate/phase-aj` was cut directly from `develop @ 0a0c9bed`, the same
  commit as `develop`'s tip at cut time — not from a separately recorded
  post-Phase-AI-phase-closure SHA, because no such closure event has happened
  yet.

## Why implementation was authorized to proceed anyway

The pinned AJ planning baseline, `integrate/phase-ai-31-33 @
150391ecdf2e003185bff7d78427cd21509a7981` (the commit containing the unified
HTTP local transports over UDS and TCP that AJ's sprints are written against),
is confirmed an ancestor of `develop @ 0a0c9bed`. The HTTP transport code
this phase depends on is genuinely present on `develop` and is the code this
team's own live daemon currently runs (confirmed via `atm doctor` during an
unrelated daemon-stability check earlier in this session). Rand Lee, as
project owner, explicitly reviewed this evidence and directed team-lead to
create `integrate/phase-aj` and dispatch AJ.1 onward on 2026-08-04, ahead of
Phase AI's formal phase-closure event. This is a deliberate, informed
override of the gate's literal sequencing — not an accidental skip.

## Reconciliation diff performed

Diffed the pinned baseline (`150391ec`) against `develop @ 0a0c9bed` (the SHA
`integrate/phase-aj` was cut from) for every AJ exact target named across the
sprint docs:

| Exact target | Drift? |
| --- | --- |
| `crates/atm-core/src/ack/mod.rs` | none |
| `crates/atm-daemon-client/src/api.rs` | none |
| `crates/atm-core/src/send/mod.rs` | none |
| `crates/atm-core/src/read/mod.rs` | none |
| `crates/atm-daemon/src/runtime_status_cache.rs` | none |
| `crates/atm-core/src/delivery_policy.rs` | none |
| `crates/atm-daemon/src/runtime_health.rs` | **yes** — see below |

`runtime_health.rs`'s `dispatch_with_deadline()` gained a real
`require_dispatch_budget()` / `request_may_have_side_effects()` mechanism
(retry-safe `ATM_DAEMON_MAY_HAVE_EXECUTED` outcome after side-effecting
dispatch work may have started) that did not exist at the pinned baseline.
This is not new drift to react to: `sprint-AJ4.md`'s existing
deadline-interaction language, written during an earlier plan-hardening
round, already describes this exact mechanism in matching terms ("the
accepted local observation is retained: it describes the daemon-side event,
not a client-visible success"). No plan or sprint-doc change was required.

## Residual risk this override accepts

- If Phase AI's eventual formal closure (the physical peer-evidence gate)
  surfaces changes to `develop` beyond what's captured in the diff above, AJ's
  in-flight sprints will need a follow-up reconciliation pass against
  whatever new drift appears between `0a0c9bed` and the actual post-closure
  SHA. Re-run the same exact-target diff at that time.
- `integrate/phase-aj`'s branch name is lowercase, matching this repo's
  convention for other phase integration branches
  (`integrate/phase-ai-31-33`, `integrate/phase-ak`); AJ sprint doc frontmatter
  originally referenced mixed-case `integrate/phase-AJ` and was normalized to
  match during AJ.1.

Recorded by team-lead, authorized by Rand Lee, 2026-08-04.
