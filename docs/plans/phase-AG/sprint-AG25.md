---
id: AG.25
title: Live Two Daemon Pair Proof For Unified Cross Host Delivery
status: complete
execution_status: not_started  # plan doc is complete/ready-for-review; code has not landed on any feature/pAG-sN branch yet
branch: feature/pAG-s25-live-two-daemon-proof
worktree: ../atm-core-worktrees/feature/pAG-s25-live-two-daemon-proof
target: develop
---

# Sprint AG.25 — Live Two Daemon Pair Proof For Unified Cross Host Delivery

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.25
worktree: ../atm-core-worktrees/feature/pAG-s25-live-two-daemon-proof
branch: feature/pAG-s25-live-two-daemon-proof
status: complete
estimated_scope: medium
```

## Goal

Prove the post-AG.18-AG.24 design works on real daemon pairs with one message
path, one ack path, and no hidden local fallback.

## Hard Dependencies

- AG.18 merged
- AG.19 merged
- AG.20 merged
- AG.21 merged
- AG.22 merged
- AG.23 merged
- AG.24 merged

## Exact Targets

- live validation rows owned by:
  - `CROSSHOST-UNIFY-8`
  - `CROSSHOST-UNIFY-9`
- `docs/plans/phase-AG/plan-phase-AG.md:321-341`
- `docs/plans/phase-AG/plan-phase-AG.md:765-798`
- `docs/plans/phase-AG/plan-phase-AG.md:854-898`
- `docs/plans/phase-AG/plan-phase-AG.md:1059-1098`
- `crates/atm/src/composition.rs:1099-1603`
- `crates/atm-daemon/src/peer_transport.rs:1156-1625`
- `crates/atm-daemon/src/tests/runtime_root.rs:335-496`

## Deliverables

- retained live evidence for:
  - localhost send / read / ack / reply-ack
  - self-IP send / read / ack / reply-ack
  - Mac-to-Mac send / read / ack / reply-ack
  - failed cross-host ack leaves source pending
  - no hidden local-mailbox fallback for remote targets
- final proof note naming exact daemon/client versions and commit under test
- aggregate ladder-wide LOC rollup from the pre-AG.15 baseline through the
  AG.24 merge tip proving the full cleanup line is net-negative

## Specific Deletions Required

- `docs/plans/phase-AG/plan-phase-AG.md:1059-1098`
  - delete any stale claim that AG.25 closes with proof before AG.18-AG.24
    are actually merged and unified
- `docs/plans/phase-AG/plan-phase-AG.md:854-898`
  - delete any stale other-Mac / Windows smoke closure wording that assumes
    proof rows are already green without retained current evidence
- `crates/atm/src/composition.rs:1099-1603`
  - delete proof-only test assumptions that still tolerate legacy split send
    semantics or loopback-only exceptions after the AG.18-AG.24 cleanup line
- `crates/atm-daemon/src/peer_transport.rs:1156-1625`
  - delete proof harness assumptions that no longer match the final retained
    transport boundary after AG.20-AG.24 cleanup
- `crates/atm-daemon/src/tests/runtime_root.rs:335-496`
  - delete stale runtime-root proof assertions that still encode pre-cleanup
    split response or fallback behavior
- no new production code path is allowed in this sprint except proof-only glue
  that is deleted before merge
- any temporary probe, harness, or diagnostic branch added to prove AG.18-AG.24
  must be removed from production modules before closeout

## Logic / Branches / State That Do Not Belong

- localhost-only or same-host-only production routing added just to make proof
  rows pass
- proof-specific alternate send/ack path that bypasses the canonical route
- socket-specific state machine additions that would have to be removed for a
  future HTTP transport
- any test/proof surface that still encodes stale pre-cleanup split semantics
- any plan text that marks proof rows complete without current retained
  artifacts

## Required Work

- rerun the full functional matrix only after AG.18-AG.24 merge
- verify both directions on real host pairs
- retain sender and receiver logs plus CLI JSON for every row
- re-audit proof/test surfaces after AG.24 so stale pre-cleanup assumptions are
  deleted before final proof claims

## Explicit Code Samples

```text
atm send <agent>@<team>.127.0.0.1 --requires-ack ...
atm send <agent>@<team>.<self-ip> --requires-ack ...
atm send <agent>@<team>.<other-mac-ip> --requires-ack ...
atm ack <message-id> <reply>
```

## This Sprint Does Not Close

- does not add any new product scope
- only closes live-proof items after AG.18-AG.24 are merged and green

## Supporting types and staged removal

- remove in AG.25 if any stale proof-only surfaces survive:
  - loopback/self-IP proof helpers in `crates/atm/src/composition.rs`
    that still encode legacy split send semantics
  - peer transport proof harness assertions in
    `crates/atm-daemon/src/peer_transport.rs`
    that still assume transport-owned policy/replay/request-shape behavior
  - runtime-root proof assertions that still assume split send/ack response
    families

- retained in AG.25:
  - only the minimum proof harness surface needed to validate the final
    retained production architecture
  - no retained helper whose only purpose is to preserve a superseded path

## Exact Keep / Delete Decisions

### Canonical path to keep

- retain exactly one proof target:
  - the post-AG.18-AG.24 production path
  - same path exercised via localhost, self-IP, other-Mac, and Windows/macOS
    rows

### Proof / harness layer

- keep:
  - proof helpers that exercise the retained production path without bypasses
  - artifact capture for CLI JSON, logs, and doctor output
- delete:
  - any helper or assertion that tolerates fallback/local-only behavior
  - any helper or assertion that encodes split send/ack transport semantics
  - any stale plan text claiming closure without current evidence

## Acceptance Criteria

- localhost and self-IP proofs are green on the unified architecture
- real cross-host Mac-to-Mac send and ack both work in both directions
- failed cross-host ack leaves source pending until confirmed delivery
- no daemon dispatch fallback writes a remote-target message to a local mailbox
- proof/test/docs surfaces contain no stale pre-cleanup assumptions about split
  paths or already-closed rows

## Hard Merge Gate

- AG.25 itself may not add net production code while closing proof gaps; if it
  touches `crates/`, the delta must be at most `-25` net LOC, and any result
  above `-25` net LOC fails the sprint
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- AG.25 must also report a ladder-wide rollup:
  - `git diff --stat <pre-ag15-baseline-sha>..<ag24-merge-tip-sha> -- crates/`
- the aggregate AG.18-AG.24 ladder-wide diff must be at least `-5000` net LOC
- the expected cleanup band for the AG.18-AG.24 ladder is `-5000` to `-20000`
  net LOC across `crates/`; anything below that band fails the cleanup goal
- every added line must be scrutinized for absolute necessity; lines added only
  to preserve parallel paths, socket-only semantics, or transport-local policy
  fail the sprint
- every production path touched by proof-driven fixes must be enumerated and
  proven collapsed to the single retained implementation; any surviving
  alternate production path is a merge blocker
- every retained boundary and wire contract must stay compatible with a future
  HTTP transport phase; any new socket-only semantic, custom state machine, or
  transport-specific message shape is a merge blocker
- quality-mgr QA must independently sweep for any new duplicate abstraction,
  wire shape, or parallel code path introduced by any proof-driven fix

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- retained live artifacts from both daemons and both CLIs for every executed
  proof row
- `rg -n "AG\\.25 is complete|other-Mac smoke lane is complete|Windows/macOS smoke lane is complete|SendRequestEnvelope|SendResponseEnvelope" docs/plans/phase-AG/plan-phase-AG.md crates/atm/src/composition.rs crates/atm-daemon/src/peer_transport.rs crates/atm-daemon/src/tests/runtime_root.rs`
