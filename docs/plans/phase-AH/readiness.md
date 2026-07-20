# Phase AH Readiness Record

## Purpose

Readiness gate for Phase AH: the Hermes + atm-graft integration line that
adds Python-consumable atm-graft bindings, a durable `session_id` field on
the ATM message model, session-scoped `atm read` defaults, and end-to-end
closure of the four motivating Hermes user stories.

Phase AH is not ready until:

- AH.1 closes the session_id protocol and query-surface gaps
- AH.2 closes the Python bindings
- AH.3 closes the Hermes gateway graft integration (including the
  `X-Session-ID` webhook surface)
- AH.4 closes the launchd deployment story
- AH.5 closes the four-motivating-story validation lane
- every failed validation row in any lane is linked to a finding in the
  Phase AH findings ledger
- no regression exists on the existing Hermes gateway channel behavior
- no regression exists on the atm-core CLI / atm-daemon behavior

## Per-Sprint Closure Results

| Sprint | Closure Result | Candidate Commit | Notes |
|---|---|---|---|
| AH.1 | `PENDING` | TBD | session_id protocol + query surface not yet implemented |
| AH.2 | `PENDING` | TBD | atm-graft PyO3 Python bindings not yet implemented |
| AH.3 | `PENDING` | TBD | Hermes gateway graft integration + X-Session-ID routing not yet implemented |
| AH.4 | `PENDING` | TBD | Hermes launchd bridge process deployment not yet implemented |
| AH.5 | `PENDING` | TBD | Four-story closure validation not yet executed |

Allowed closure-result values:

- `PENDING`
- `PASS`
- `FAIL`
- `BLOCKED`
- `PARTIAL` (a sprint produced partial evidence but did not close all rows;
  requires explicit follow-on sprint to close the remainder)

## Lane Closure

| Lane | Closure Result | Notes |
|---|---|---|
| A — Session-ID Protocol Closure | `PENDING` | gated on AH.1 |
| B — Python Bindings + Hermes Integration (macOS) | `PENDING` | gated on AH.1, AH.2, AH.3 |
| C — Cross-Profile Hermes Validation | `PENDING` | gated on AH.4 |
| D — Four-Story Closure Validation | `PENDING` | gated on B, C green |

## Required Gate Criteria

Phase AH must remain not-ready until all of the following are true:

- AH.1 closes before AH.2 begins; the session_id field must already be on
  the durable message model before the Python binding starts work; the
  binding must carry the field through the Python→Rust boundary verbatim
- AH.2 closes before AH.3 begins
- AH.3 closes before AH.4 and AH.5 can execute; `X-Session-ID` routing on
  the webhook adapter is the prerequisite for both
- AH.4 closes before AH.5
- Lane A must close before Lane B
- Lanes B and C must close before Lane D
- the `atm read` default-mode change (session-scoped) must land before AH.5
  story-1/4 closure, otherwise multi-turn evidence cannot be captured through
  session-scoped reads
- the `atm read --agent <name>` query must land before AH.5 story-1/4
  closure, otherwise "where did we discuss something" behavior cannot be
  probed
- every failed row is linked to a finding in the Phase AH findings ledger
  with an owner and a classification from the enum in `plan-phase-AH.md`
- the Hermes Telegram session is demonstrated isolated from ATM nudge
  delivery at least once before AH.5 closes
- every story in AH.5 carries:
  - full command transcript
  - Hermes session ID
  - evidence of session persistence across 3+ turns (for stories 1 and 4)
  - latency measurement (for story 3)
  - a per-story verdict

## Initial Verdict

- readiness status: `NOT READY`
- gate status: `BLOCKED`
- notes: Phase AH is green-field; no session_id field, no Python bindings,
  no Hermes graft integration, no launchd bridge, and no story-closure
  evidence exist yet. Phase AH cannot proceed without first landing the
  session_id protocol on the durable message model because every downstream
  sprint routes by `atm:{from_agent}:{session_id}`.
