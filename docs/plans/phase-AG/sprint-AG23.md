---
id: AG.23
title: Remove Synthetic Deferred Receipt Construction From Daemon Dispatch
status: complete
execution_status: not_started  # plan doc is complete/ready-for-review; code has not landed on any feature/pAG-sN branch yet
branch: feature/pAG-s23-remove-synthetic-deferred-receipts
worktree: ../atm-core-worktrees/feature/pAG-s23-remove-synthetic-deferred-receipts
target: develop
---

# Sprint AG.23 — Remove Synthetic Deferred Receipt Construction From Daemon Dispatch

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.23
worktree: ../atm-core-worktrees/feature/pAG-s23-remove-synthetic-deferred-receipts
branch: feature/pAG-s23-remove-synthetic-deferred-receipts
status: complete
estimated_scope: medium
```

## Goal

Delete mailbox-visible deferred-receipt synthesis from daemon dispatch so the
dispatcher returns delivery facts rather than creating semantic receipt
messages itself.

All line citations in this sprint are pre-ladder baseline references and must
be re-resolved against the actual branch tip immediately before execution; run
the existing `rg -n ...` validation first as the anti-staleness check.

## Hard Dependencies

- AG.22 merged

## Scope Boundary

- this sprint is the sole owner for deleting
  `persist_outcome_unknown_request(...)` and
  `resume_pending_replay(...)` from the retained production contract

## Exact Targets

- `crates/atm-daemon/src/composition.rs:292-315`
- `crates/atm-daemon/src/peer_transport.rs:324-338`
- `crates/atm-daemon/src/peer_transport.rs:647-664`
- `docs/plans/phase-AG/plan-phase-AG.md:239-249`
- `docs/plans/phase-AG/plan-phase-AG.md:723-726`
- source findings:
  - boundary-review item 2
  - `CROSSHOST-UNIFY-4` follow-on closure

## Specific Deletions Required

- `crates/atm-daemon/src/composition.rs:292-315`
  - delete daemon-startup replay framing that still treats deferred sender
    closure as a daemon-owned obligation
- `crates/atm-daemon/src/peer_transport.rs:324-338`
  - delete outcome-unknown replay persistence behavior from the transport path
    once AG.20/AG.22 have lifted replay ownership above transport
- `crates/atm-daemon/src/peer_transport.rs:657-664`
  - delete recovery text that still promises replay-owned handoff/receipt
    closure from inside transport
- `docs/plans/phase-AG/plan-phase-AG.md:239-249`
  - delete the daemon-owned final sender-inbox receipt obligation from the
    phase contract
- `docs/plans/phase-AG/plan-phase-AG.md:723-726`
  - delete the promise that bounded retry necessarily concludes by appending a
    final sender-inbox receipt

## Logic / Branches / State That Do Not Belong

- any daemon-startup or transport branch that owns sender-inbox receipt closure
- any replay/resume path that claims responsibility for final sender-facing
  delivery receipts
- any policy text that promises daemon-owned synthetic deferred receipts after
  transport failure

## Deliverables

- no daemon-owned sender-inbox deferred-receipt closure promise survives in
  code or plan text
- deferred/unknown result shaping lives in one shared policy layer outside
  transport and daemon startup
- replay/resume is no longer described or implemented as the owner of final
  sender receipt synthesis

## Required Work

- delete replay/transport text and hooks that still claim receipt ownership
- remove transport-owned outcome-unknown replay persistence if AG.20/AG.22 have
  not already deleted it
- rewrite phase text so deferred/terminal caller results are facts, not a
  promise of daemon-authored mailbox receipts

## Explicit Code Samples

```rust
match deliver_remote_send_request(...) ? {
    Delivered(response) => Ok(*response),
    Deferred(state) => Ok(shared_outcome_policy::deferred(state)?),
    OutcomeUnknown(state) => Ok(shared_outcome_policy::unknown(state)?),
    RejectedTerminal(error) => Err(error),
}
```

## Supporting types and staged removal

- remove in AG.23 if the cleanup can land in one patch:
  - `crates/atm-daemon/src/composition.rs:292-315`
    - `resume_startup_replay(...)` as a sender-receipt policy surface
    - retain only if it becomes a pure retained-work bootstrap with no receipt
      semantics
  - `crates/atm-daemon/src/peer_transport.rs:324-338`
    - `persist_outcome_unknown_request(...)`
  - `crates/atm-daemon/src/peer_transport.rs:788-789`
    - `PeerTransportRuntime::resume_pending_replay(...)`
    - retain only if replay ownership remains above transport and no sender
      receipt semantics survive

- if removal must be staged, deprecate in AG.23 and delete no later than the
  sprint that fully relocates replay ownership:
  - `persist_outcome_unknown_request(...)`
  - `resume_pending_replay(...)`

- retained in AG.23:
  - transport-level `RemoteDeliveryOutcomeUnknown` errors as facts only
  - no retained type whose meaning includes “final sender receipt”

## Exact Keep / Delete Decisions

### Canonical path to keep

- retain exactly one sender-facing result contract:
  - caller receives immediate confirmed/deferred/terminal facts
  - any later retry/resume ownership is policy-layer internal work, not a
    synthetic mailbox receipt guarantee emitted by transport or startup glue

### Startup / replay layer

- keep:
  - bounded retained-work bootstrap only if it is purely operational
- delete:
  - any startup replay framing that implies daemon-authored sender receipt
    closure

### Transport / recovery layer

- keep:
  - factual transport errors and recoveries
- delete:
  - any recovery text or helper that turns replay into a sender-receipt
    contract

### Plan / contract surfaces that must be rewritten

- `docs/plans/phase-AG/plan-phase-AG.md:239-249`
  - remove sender-inbox receipt obligation language
- `docs/plans/phase-AG/plan-phase-AG.md:723-726`
  - remove “final delivery/failure receipt lands in sender inbox” as a hard
    product promise

## This Sprint Does Not Close

- AG.24 owns request-shape preservation
- AG.25 owns live proof

## Acceptance Criteria

- there is no surviving daemon-owned deferred-receipt closure promise in code
  or plan text
- replay/resume no longer implies final sender mailbox receipt ownership
- caller-visible deferred/unknown/terminal behavior is expressed as one shared
  result contract, not as synthetic mailbox work

## Hard Merge Gate

- this sprint must deliver at least `-100` net LOC across `crates/` in its
  named target files and contribute to the AG.18-AG.25 ladder-wide aggregate
  reduction; any result above `-100` net LOC fails the sprint
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- every added line must be scrutinized for absolute necessity; lines added only
  to preserve parallel paths, socket-only semantics, or transport-local policy
  fail the sprint
- every production receipt-synthesis path must be enumerated and proven
  deleted or unreachable except for the retained behavior; any surviving
  alternate production path is a merge blocker
- every retained boundary and wire contract must stay compatible with a future
  HTTP transport phase; any new socket-only semantic, custom state machine, or
  transport-specific message shape is a merge blocker
- quality-mgr QA must independently sweep for any new synthetic mailbox
  receipt path introduced in dispatch or adjacent glue

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- `rg -n "resume_pending_replay|persist_outcome_unknown_request|final sender-inbox receipt|final delivery/failure receipt lands in sender inbox" crates/atm-daemon/src/composition.rs crates/atm-daemon/src/peer_transport.rs docs/plans/phase-AG/plan-phase-AG.md`
