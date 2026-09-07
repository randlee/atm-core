---
id: AY.12
phase: AY
sprint: AY.12
title: Stream becomes the state source; poll removed
branch: feature/ay12-herdr-drop-poll
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay12-herdr-drop-poll
integration_branch: integrate/phase-ay
stack_parent: none
pr_target: integrate/phase-ay
target: integrate/phase-ay
status: gated (dispatch only after "Decision (Rand, YYYY-MM-DD): drop poll" is recorded in phase-ay-plan.md item 7)
recommended_agent: arch-ctm
recommended_model: deep-reasoning
added: 2026-09-06 (subscription socket; see phase-ay-plan.md "Rework record" item 7)
execution_track: join
parallel_with: []
dependency_relations:
  - prerequisite: AY.11
    dependent: AY.12
    relation: must_follow
    rationale: parity evidence from AY.11 and Rand's decision on it gate this sprint.
---

# AY.12 — Stream becomes the state source; poll removed

Once AY.11's parity evidence is accepted, the shadow map becomes the map.
The queue-wake tick no longer calls `list` or per-member `get`; agent state
comes from the stream, and the tick keeps only its existing duty of
sending due prompts and reminders from queued mail. On the CLI transport
(no stream) the poll remains exactly as today.

## Deliverables

- [ ] D1 — `tick_once` reads agent state from the AY.11 map when the
  socket transport is selected; the `list`/`get` calls run only when the
  CLI transport is selected. `HERDR_POLL_INTERVAL_MS` remains as the
  prompt/reminder cadence.
- [ ] D2 — one `agent.list` per session on every (re)subscribe is already
  the `Baseline` event (AY.10); D1 must not add another.
- [ ] D3 — the parity block and counters are deleted with the poll on the
  socket path; the CLI path never had them.
- [ ] D4 — lifecycle tests (AY.4 matrix rerun under stream-sourced state):
  Herdr restart, stream `Closed`, and breaker open all leave prompt
  decisions correct on the next `Baseline`.
- [ ] D5 — docs: the operator section drops the parity counters and states
  the two state sources by transport.

### Changed-file allowlist

- `crates/atm-http-runtime/src/herdr_queue_wake.rs`
- `crates/atm-http-runtime/src/herdr_status_shadow.rs` (rename to
  `herdr_status.rs` allowed)
- `crates/atm-http-runtime/tests/**` for the two files above
- doctor projection file (counter removal)
- `docs/atm-herdr/architecture.md` (operator section)

No file under `crates/atm-herdr` changes. Stream failures do not feed the
nudge breaker; AY.10 C2 and HR-CORE-011 settle that for the phase (a
stream fault surfaces as `Closed` and the map falls back to its last
`Baseline`, which D4 exercises). This sprint carries no open design
decision beyond the drop-poll line that gates it.

### Size

Two production files in one crate, their tests, one doctor projection
edit, one doc section: expected to fit one context window. No split is
pre-declared; if it does not fit, that is a plan amendment, not a
dispatch-time decision.

## Acceptance criteria

1. Fake server with 50 agents: after connect, zero `agent.list`/`agent.get`
   requests over ten ticks on the socket path; the CLI path issues the same
   requests it does today.
2. AY.4 lifecycle matrix green under stream-sourced state.
3. Common phase merge gate.

## Out of scope

- Any change to prompt/reminder policy.
- Any `atm-herdr` change; the breaker question is closed in AY.10 C2.
- The legacy synchronous daemon.
