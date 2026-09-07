---
id: AY.11
phase: AY
sprint: AY.11
title: Daemon consumes the status stream in shadow mode and records parity with the poll
branch: feature/ay11-herdr-stream-shadow-parity
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/ay11-herdr-stream-shadow-parity
integration_branch: integrate/phase-ay
stack_parent: none
pr_target: integrate/phase-ay
target: integrate/phase-ay
status: draft
recommended_agent: arch-ctm
recommended_model: deep-reasoning
added: 2026-09-06 (subscription socket; see phase-ay-plan.md "Rework record" item 7)
execution_track: join
parallel_with: []
dependency_relations:
  - prerequisite: AY.9
    dependent: AY.11
    relation: must_follow
    rationale: the socket transport must be the selected production composition before the daemon can hold a stream.
  - prerequisite: AY.10
    dependent: AY.11
    relation: must_follow
    rationale: consumes the HerdrStatusStream trait.
  - prerequisite: AY.11
    dependent: AY.12
    relation: must_follow
    rationale: AY.12 removes the poll only on AY.11's recorded parity evidence and Rand's decision.
---

# AY.11 — Daemon consumes the status stream in shadow mode and records parity with the poll

Run both. The existing 5 s queue-wake poll stays the authority for every
decision the daemon makes. Alongside it, one held stream per configured
session (AY.10) maintains a second, stream-derived agent-state map. Every
tick compares the two and counts disagreements. Nothing the daemon does
changes on the basis of the stream in this sprint.

Rand, 2026-09-06: "are you proposing we do both and verify they match.
once they match we can drop the polling?"; "this is a prudent solution.";
"and probably lowest risk".

## Dispatch and PR topology

Standalone from `integrate/phase-ay` after AY.9 and AY.10 have merged.
Ordinary PR, target `integrate/phase-ay`, no `gh stack link`.

## Deliverables

- [ ] D1 — `crates/atm-http-runtime/src/herdr_status_shadow.rs` (new): a
  runtime-owned task per configured session that calls
  `HerdrStatusStream::subscribe`, applies `Baseline`/`Changed`/`Gone` to a
  RAM map `{AgentName -> (HerdrAgentStatus, observed_at)}`, and on `Closed`
  waits the queue-wake interval and subscribes again. Started only when the
  selected transport is socket (AY.9 D1); on CLI the task is not spawned.
  Follows the RAM write-through rule already in force for the roster.
- [ ] D2 — parity check inside `tick_once` in
  `crates/atm-http-runtime/src/herdr_queue_wake.rs`: after the tick's
  `list`, compare each agent's polled status with the shadow map. Count
  `match`, `stream_stale` (stream older than one interval), `mismatch`,
  `stream_missing` (agent absent from stream map), `stream_extra`. Emit one
  structured log line per tick with the five counters and, for each
  mismatch, agent name plus both statuses. Never a message body, never a
  pane title.
- [ ] D3 — `HerdrQueueWakeStats` gains the five counters; the existing
  stats sink and the doctor's Herdr section (AY.3/AY.9) project them as
  `herdr.stream_parity.*`.
- [ ] D4 — tests in `crates/atm-http-runtime/tests/herdr_stream_parity.rs`
  against the AY.10 fake server streaming mode and the existing fake
  process: every counter, the CLI-selected no-spawn case, resubscribe after
  `Closed`, and drain (the shadow task stops within the runtime shutdown
  deadline, no detached task).
- [ ] D5 — `docs/atm-herdr/operations.md` (or the AY.9 doc that owns the
  Herdr operator section): what the parity counters mean and the exit
  condition for AY.12 below.

### Paths to delete

None.

## Contracts

No public API change. No config key: shadow mode is on whenever the socket
transport is selected. `HERDR_POLL_INTERVAL_MS` and every prompt/reminder
decision are untouched (`git diff` on the decision functions is empty;
only the parity block is added to `tick_once`).

### Changed-file allowlist

- `crates/atm-http-runtime/src/herdr_status_shadow.rs` (new)
- `crates/atm-http-runtime/src/herdr_queue_wake.rs` (parity block, stats)
- `crates/atm-http-runtime/src/lib.rs` (module and spawn at composition)
- `crates/atm-http-runtime/tests/herdr_stream_parity.rs` (new)
- `crates/atm-daemon-bootstrap/src/replacement_handler.rs` (pass the
  stream handle next to the invoker; one site)
- doctor projection file owned by AY.9 D4 (five counters)
- `docs/atm-herdr/operations.md`

### Size

One new module, one parity block, one test file: one context window.

## Acceptance criteria

1. With socket selected, exactly one stream per session is held for the
   daemon's lifetime (fake server asserts connection count with 50 agents
   scripted).
2. Every decision path in `tick_once` is byte-identical to its AY.9 state
   except the parity block; a test proves prompts and reminders are
   unaffected by any shadow-map content, including a fully wrong map.
3. All five counters are exercised by tests and projected by doctor.
4. CLI selected: no shadow task, counters absent from doctor.
5. Drain: shadow task and stream receiver stop within the runtime shutdown
   deadline; no detached task.
6. Common phase merge gate.

## Exit condition this sprint records (input to AY.12)

Parity evidence is the daemon's own log over a dogfood run on rand-m4
with the normal team active: consecutive ticks with `mismatch == 0` and
`stream_missing == 0` (`stream_stale` is expected around a change and is
not a failure). The number of ticks and the run length are Rand's call
when he reads the evidence; this sprint does not fix a threshold.

## Required validation

- `just validate` on all three CI lanes.
- `cargo test -p atm-http-runtime`.
- `python3 .just/check_line_counts.py`.

## Out of scope

- Using the stream for any decision; removing or slowing the poll (AY.12).
- Any `atm-herdr` change (AY.10 owns the crate).
- The legacy synchronous daemon.
